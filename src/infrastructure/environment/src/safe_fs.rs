use application::ErrorMarker;
use cap_fs_ext::DirExt;
use cap_fs_ext::FollowSymlinks;
use cap_fs_ext::MetadataExt as LinkMetadataExt;
use cap_fs_ext::OpenOptionsFollowExt;
use cap_std::ambient_authority;
use cap_std::fs::Dir;
use cap_std::fs::File;
use cap_std::fs::Metadata;
#[cfg(windows)]
use cap_std::fs::MetadataExt;
use cap_std::fs::OpenOptions;
#[cfg(windows)]
use cap_std::fs::OpenOptionsExt;
use cap_std::fs::ReadDir;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use rootcause::report;
use same_file::Handle;
use std::collections::HashSet;
use std::ffi::OsStr;
use std::ffi::OsString;
use std::io;
use std::io::Read;
use std::io::Write;
use std::path::Component;
use std::path::Path;
#[cfg(windows)]
use std::path::PathBuf;
use tokio_util::sync::CancellationToken;
#[cfg(windows)]
use windows::Win32::Foundation::GENERIC_WRITE;
#[cfg(windows)]
use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
#[cfg(windows)]
use windows::Win32::Storage::FileSystem::FILE_FLAG_BACKUP_SEMANTICS;
#[cfg(windows)]
use windows::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;
#[cfg(windows)]
use windows::Win32::Storage::FileSystem::FILE_SHARE_READ;
#[cfg(windows)]
use windows::Win32::Storage::FileSystem::FILE_SHARE_WRITE;

#[derive(Debug)]
pub(crate) struct SafeFile {
	inner: File,
}

impl SafeFile {
	pub(crate) fn metadata(&self) -> Result<Metadata, io::Error> {
		self.inner.metadata().into_report()
	}

	pub(crate) fn read_chunk(&mut self, contents: &mut [u8]) -> Result<usize, io::Error> {
		self.inner.read(contents).into_report()
	}

	pub(crate) fn write_chunk(&mut self, contents: &[u8]) -> Result<(), io::Error> {
		self.inner.write_all(contents).into_report()
	}

	pub(crate) fn finish(&self) -> Result<(), io::Error> {
		let metadata = self.inner.metadata().into_report()?;
		if !metadata.is_file() || is_reparse(&metadata) || metadata.nlink() != 1 {
			return Err(report!(io::Error::other(
				"opened path is a reparse point, hard link, or non-regular file",
			)));
		}
		self.inner.sync_all().into_report()
	}
}

#[derive(Debug)]
pub(crate) struct SafeDir {
	inner: Dir,
}

impl SafeDir {
	pub(crate) fn open_absolute(path: &Path) -> Result<Self, io::Error> {
		Self::open_absolute_inner(path, false)
	}

	pub(crate) fn create_absolute(path: &Path) -> Result<Self, io::Error> {
		Self::open_absolute_inner(path, true)
	}

	fn open_absolute_inner(path: &Path, create: bool) -> Result<Self, io::Error> {
		let (mut current, components) = absolute_anchor(path)?;
		if components.is_empty() {
			return Err(report!(invalid_path()));
		}
		for name in &components {
			current = match current.open_dir_nofollow(name) {
				Ok(directory) => directory,
				Err(error) if create && error.kind() == io::ErrorKind::NotFound => {
					current.create_dir(name).into_report()?;
					sync_dir(&current)?;
					current.open_dir_nofollow(name).into_report()?
				}
				Err(error) => return Err(report!(error)),
			};
			validate_directory(&current)?;
		}
		Ok(Self { inner: current })
	}

	pub(crate) fn create_dir(&self, name: impl AsRef<Path>) -> Result<Self, io::Error> {
		let name = checked_component(name.as_ref())?;
		self.inner.create_dir(name).into_report()?;
		// The parent sync durably publishes the new child. It must complete before the child
		// handle is returned.
		self.sync()?;
		self.open_dir(name)
	}

	pub(crate) fn open_dir(&self, name: impl AsRef<Path>) -> Result<Self, io::Error> {
		let name = checked_component(name.as_ref())?;
		let inner = self.inner.open_dir_nofollow(name).into_report()?;
		validate_directory(&inner)?;
		Ok(Self { inner })
	}

	pub(crate) fn ensure_dir(&self, name: impl AsRef<Path>) -> Result<Self, io::Error> {
		let name = checked_component(name.as_ref())?;
		match self.create_dir(name) {
			Ok(directory) => Ok(directory),
			Err(error) if error.current_context().kind() == io::ErrorKind::AlreadyExists => {
				self.open_dir(name)
			}
			Err(error) => Err(error),
		}
	}

	pub(crate) fn entries(&self) -> Result<ReadDir, io::Error> {
		self.inner.entries().into_report()
	}

	pub(crate) fn entry_metadata(&self, name: impl AsRef<Path>) -> Result<Metadata, io::Error> {
		self.inner
			.symlink_metadata(checked_component(name.as_ref())?)
			.into_report()
	}

	pub(crate) fn symlink_metadata(&self, name: impl AsRef<Path>) -> Result<Metadata, io::Error> {
		let metadata = self.entry_metadata(name)?;
		if is_reparse(&metadata) {
			return Err(report!(io::Error::other("refusing to inspect a reparse point")));
		}
		Ok(metadata)
	}

	pub(crate) fn exists(&self, name: impl AsRef<Path>) -> Result<bool, io::Error> {
		match self.symlink_metadata(name) {
			Ok(_) => Ok(true),
			Err(error) if error.current_context().kind() == io::ErrorKind::NotFound => Ok(false),
			Err(error) => Err(error),
		}
	}

	pub(crate) fn create_new_file(&self, name: impl AsRef<Path>) -> Result<SafeFile, io::Error> {
		let name = checked_component(name.as_ref())?;
		let mut options = OpenOptions::new();
		options.write(true).create_new(true).follow(FollowSymlinks::No);
		let file = self.inner.open_with(name, &options).into_report()?;
		let metadata = file.metadata().into_report()?;
		if !metadata.is_file() || is_reparse(&metadata) || metadata.nlink() != 1 {
			return Err(report!(io::Error::other(
				"created path is a reparse point, hard link, or non-regular file",
			)));
		}
		Ok(SafeFile { inner: file })
	}

	pub(crate) fn write_new(&self, name: impl AsRef<Path>, contents: &[u8]) -> Result<(), io::Error> {
		let mut file = self.create_new_file(name)?;
		file.write_chunk(contents)?;
		file.finish()
	}

	pub(crate) fn open_regular(&self, name: impl AsRef<Path>) -> Result<SafeFile, io::Error> {
		let name = checked_component(name.as_ref())?;
		let mut options = OpenOptions::new();
		options.read(true).follow(FollowSymlinks::No);
		let file = self.inner.open_with(name, &options).into_report()?;
		let metadata = file.metadata().into_report()?;
		if !metadata.is_file() || is_reparse(&metadata) || metadata.nlink() != 1 {
			return Err(report!(io::Error::other(
				"opened path is a reparse point, hard link, or non-regular file",
			)));
		}
		Ok(SafeFile { inner: file })
	}

	pub(crate) fn sync_file(&self, name: impl AsRef<Path>) -> Result<(), io::Error> {
		let name = checked_component(name.as_ref())?;
		let mut options = OpenOptions::new();
		options.read(true).write(true).follow(FollowSymlinks::No);
		let file = self.inner.open_with(name, &options).into_report()?;
		let metadata = file.metadata().into_report()?;
		if !metadata.is_file() || is_reparse(&metadata) || metadata.nlink() != 1 {
			return Err(report!(io::Error::other(
				"opened path is a reparse point, hard link, or non-regular file",
			)));
		}
		file.sync_all().into_report()
	}

	pub(crate) fn rename_to(
		&self,
		source: impl AsRef<Path>,
		destination_dir: &Self,
		destination: impl AsRef<Path>,
	) -> Result<(), io::Error> {
		let source = checked_component(source.as_ref())?;
		self.symlink_metadata(source)?;
		self.inner
			.rename(source, &destination_dir.inner, checked_component(destination.as_ref())?)
			.into_report()
	}

	/// Renames one no-follow entry and makes the directory-entry change durable in both parents.
	///
	/// A cross-directory rename changes both parent directories. The destination parent is synced first
	/// so its new entry is durable before the source-parent sync makes removal of the old entry durable.
	/// A rename within one parent syncs that directory only once.
	pub(crate) fn rename_durable_to(
		&self,
		source: impl AsRef<Path>,
		destination_dir: &Self,
		destination: impl AsRef<Path>,
	) -> Result<(), io::Error> {
		let same_parent = identity(&self.inner)? == identity(&destination_dir.inner)?;
		self.rename_to(source, destination_dir, destination)?;
		destination_dir.sync()?;
		if !same_parent {
			self.sync()?;
		}
		Ok(())
	}

	pub(crate) fn remove_dir_all(&self, name: impl AsRef<Path>) -> Result<(), io::Error> {
		let name = checked_component(name.as_ref())?;
		let metadata = self.inner.symlink_metadata(name).into_report()?;
		if !metadata.is_dir() || is_reparse(&metadata) {
			return Err(report!(io::Error::other(
				"refusing to recursively remove a reparse point or non-directory",
			)));
		}
		self.inner.remove_dir_all(name).into_report()
	}

	pub(crate) fn sync(&self) -> Result<(), io::Error> {
		sync_dir(&self.inner)
	}

	pub(crate) fn is_ancestor_of(&self, child: &Self) -> Result<bool, io::Error> {
		let candidate = identity(&self.inner)?;
		let mut current = child.inner.try_clone().into_report()?;
		loop {
			let current_identity = identity(&current)?;
			if candidate == current_identity {
				return Ok(true);
			}
			let parent = current.open_parent_dir(ambient_authority()).into_report()?;
			if identity(&parent)? == current_identity {
				return Ok(false);
			}
			current = parent;
		}
	}
}

#[cfg(not(windows))]
fn absolute_anchor(path: &Path) -> Result<(Dir, Vec<OsString>), io::Error> {
	if !path.is_absolute() {
		return Err(report!(invalid_path()));
	}
	let mut components = Vec::new();
	for component in path.components() {
		match component {
			Component::RootDir => {}
			Component::Normal(name) => components.push(name.to_owned()),
			_ => return Err(report!(invalid_path())),
		}
	}
	Ok((
		Dir::open_ambient_dir("/", ambient_authority()).into_report()?,
		components,
	))
}

#[cfg(windows)]
fn absolute_anchor(path: &Path) -> Result<(Dir, Vec<OsString>), io::Error> {
	let mut path_components = path.components();
	let Some(Component::Prefix(prefix)) = path_components.next() else {
		return Err(report!(invalid_path()));
	};
	let prefix = prefix.as_os_str();
	if path_components.next() != Some(Component::RootDir) {
		return Err(report!(invalid_path()));
	}
	let mut anchor = PathBuf::from(prefix);
	anchor.push(Path::new(r"\"));
	let mut components = Vec::new();
	for component in path_components {
		let Component::Normal(name) = component else {
			return Err(report!(invalid_path()));
		};
		components.push(name.to_owned());
	}
	Ok((
		Dir::open_ambient_dir(anchor, ambient_authority()).into_report()?,
		components,
	))
}

fn validate_directory(directory: &Dir) -> Result<(), io::Error> {
	let metadata = directory.dir_metadata().into_report()?;
	if !metadata.is_dir() || is_reparse(&metadata) {
		return Err(report!(io::Error::other(
			"opened path is a reparse point or non-directory",
		)));
	}
	Ok(())
}

#[cfg(not(windows))]
pub(crate) fn is_reparse(metadata: &Metadata) -> bool {
	metadata.is_symlink()
}

#[cfg(windows)]
pub(crate) fn is_reparse(metadata: &Metadata) -> bool {
	has_reparse_attribute(metadata.file_attributes())
}

#[cfg(windows)]
fn has_reparse_attribute(attributes: u32) -> bool {
	attributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
}

#[cfg(not(windows))]
fn sync_dir(dir: &Dir) -> Result<(), io::Error> {
	dir.try_clone()
		.and_then(|directory| directory.into_std_file().sync_all())
		.into_report()
}

#[cfg(windows)]
fn sync_dir(dir: &Dir) -> Result<(), io::Error> {
	let mut options = OpenOptions::new();
	options.write(true)
		.access_mode(GENERIC_WRITE.0)
		.share_mode((FILE_SHARE_READ | FILE_SHARE_WRITE).0)
		.custom_flags((FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT).0)
		.follow(FollowSymlinks::No);
	let directory = dir.open_with(".", &options).into_report()?;
	let metadata = directory.metadata().into_report()?;
	if !metadata.is_dir() || is_reparse(&metadata) {
		return Err(report!(io::Error::other(
			"refusing to flush a reparse point or non-directory",
		)));
	}
	directory.sync_all().into_report()
}

fn identity(dir: &Dir) -> Result<Handle, io::Error> {
	Handle::from_file(dir.try_clone().into_report()?.into_std_file()).into_report()
}

fn checked_component(path: &Path) -> Result<&OsStr, io::Error> {
	if path.parent() == Some(Path::new(""))
		&& let Some(name) = path.file_name()
		&& name != OsStr::new(".")
		&& name != OsStr::new("..")
	{
		return Ok(name);
	}
	Err(report!(invalid_path()))
}

fn invalid_path() -> io::Error {
	io::Error::new(
		io::ErrorKind::InvalidInput,
		"capability operation requires one normal path component",
	)
}

pub(crate) const READ_CHUNK_BYTES: usize = 64 * 1024;
pub(crate) const MAX_DIRECTORY_ENTRIES: usize = 16_384;
// This matches the archive path component cap and bounds recursion through manually edited trees.
pub(crate) const MAX_TRAVERSAL_DEPTH: usize = 64;

pub(crate) struct EntryBudget {
	remaining: usize,
}

impl EntryBudget {
	pub(crate) fn new(limit: usize) -> Self {
		Self { remaining: limit }
	}

	pub(crate) fn consume(&mut self, directory_entries: &mut usize) -> Result<(), io::Error> {
		if self.remaining == 0 || *directory_entries == MAX_DIRECTORY_ENTRIES {
			return Err(report!(io::Error::new(
				io::ErrorKind::InvalidData,
				"directory traversal entry limit exceeded",
			)));
		}
		self.remaining -= 1;
		*directory_entries += 1;
		Ok(())
	}
}

pub(crate) fn read_bounded(
	directory: &SafeDir,
	name: impl AsRef<Path>,
	max_bytes: usize,
	context: ErrorMarker,
	cancellation: &CancellationToken,
) -> Result<Vec<u8>, ErrorMarker> {
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}
	let opened = directory.open_regular(name);
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}
	let mut file = opened.context(context.clone())?;
	let mut contents = Vec::with_capacity(max_bytes.min(READ_CHUNK_BYTES));
	let mut buffer = [0_u8; READ_CHUNK_BYTES];
	loop {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}
		let read = file.read_chunk(&mut buffer);
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}
		let read = read.context(context.clone())?;
		if read == 0 {
			break;
		}
		if contents.len().saturating_add(read) > max_bytes {
			return Err(
				report!(io::Error::new(io::ErrorKind::InvalidData, "file size limit exceeded",))
					.context(context),
			);
		}
		contents.extend_from_slice(&buffer[..read]);
	}
	Ok(contents)
}

pub(crate) fn validate_exact_entries(
	directory: &SafeDir,
	allowed: &[&str],
	cancellation: &CancellationToken,
) -> Result<(), ErrorMarker> {
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}
	let opened = directory.entries();
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}
	let mut entries = opened.context(ErrorMarker::environment_invalid(None))?;
	let mut remaining = allowed.iter().copied().collect::<HashSet<_>>();
	let mut observed = 0_usize;
	loop {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}
		let Some(entry) = entries.next() else {
			break;
		};
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}
		let entry = entry.into_report().context(ErrorMarker::environment_invalid(None))?;
		observed = observed.saturating_add(1);
		if observed > allowed.len() {
			return Err(report!(ErrorMarker::environment_invalid(None)));
		}
		let name = entry.file_name();
		let text = name
			.to_str()
			.ok_or_else(|| report!(ErrorMarker::environment_invalid(None)))?;
		if !remaining.remove(text) {
			return Err(report!(ErrorMarker::environment_invalid(None)));
		}
		let metadata = directory.symlink_metadata(&name);
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}
		metadata.context(ErrorMarker::environment_invalid(None))?;
	}
	Ok(())
}

pub(crate) const MAX_SYNC_ENTRIES: usize = 100_000;

pub(crate) fn sync_tree(
	directory: &SafeDir,
	io_context: ErrorMarker,
	cancellation: &CancellationToken,
) -> Result<(), ErrorMarker> {
	let mut budget = EntryBudget::new(MAX_SYNC_ENTRIES);
	sync_tree_inner(directory, io_context, cancellation, &mut budget, MAX_TRAVERSAL_DEPTH)
}

fn sync_tree_inner(
	directory: &SafeDir,
	io_context: ErrorMarker,
	cancellation: &CancellationToken,
	budget: &mut EntryBudget,
	remaining_depth: usize,
) -> Result<(), ErrorMarker> {
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}
	let opened = directory.entries();
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}
	let mut entries = opened.context(io_context.clone())?;
	let mut directory_entries = 0_usize;
	loop {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}
		let Some(entry) = entries.next() else {
			break;
		};
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}
		let entry = entry.into_report().context(io_context.clone())?;
		budget.consume(&mut directory_entries).context(io_context.clone())?;
		if remaining_depth == 0 {
			return Err(report!(io::Error::new(
				io::ErrorKind::InvalidData,
				"directory traversal depth limit exceeded",
			))
			.context(io_context));
		}
		let name = entry.file_name();
		let metadata = directory.symlink_metadata(&name);
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}
		let metadata = metadata.context(io_context.clone())?;
		if metadata.is_dir() {
			let child = directory.open_dir(&name);
			if cancellation.is_cancelled() {
				return Err(report!(ErrorMarker::operation_cancelled()));
			}
			sync_tree_inner(
				&child.context(io_context.clone())?,
				io_context.clone(),
				cancellation,
				budget,
				remaining_depth - 1,
			)?;
		} else if metadata.is_file() {
			let result = directory.sync_file(&name);
			if cancellation.is_cancelled() {
				return Err(report!(ErrorMarker::operation_cancelled()));
			}
			result.context(io_context.clone())?;
		} else {
			return Err(report!(io::Error::other("refusing to sync a special file")).context(io_context));
		}
	}
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}
	let result = directory.sync();
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}
	result.context(io_context)
}

#[cfg(all(test, windows))]
#[expect(
	clippy::expect_used,
	reason = "test fixture failures should report their exact setup step"
)]
#[expect(clippy::panic, reason = "unexpected Windows fixture errors must fail the test")]
mod tests {
	use super::SafeDir;
	use super::has_reparse_attribute;
	use super::identity;
	use std::fs;
	use std::io;
	use std::os::windows::fs::symlink_file;
	use tempfile::TempDir;
	use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

	#[test]
	fn relative_directory_sync_keeps_the_original_capability_and_identity() {
		let temp = TempDir::new().expect("temporary directory must be created");
		let path = temp.path().canonicalize().expect("temporary directory must resolve");
		let directory = SafeDir::open_absolute(&path).expect("safe directory must open");
		let before = identity(&directory.inner).expect("identity must be available");

		directory.sync().expect("directory flush must succeed");
		directory
			.write_new("after-sync", b"usable")
			.expect("original capability must remain usable");

		let after = identity(&directory.inner).expect("identity must remain available");
		assert_eq!(before, after);
	}

	#[test]
	fn safe_directory_handle_blocks_rename_until_drop() {
		let temp = TempDir::new().expect("temporary directory must be created");
		let source = temp.path().join("source");
		let destination = temp.path().join("destination");
		fs::create_dir(&source).expect("source directory must be created");
		let directory = SafeDir::open_absolute(&source.canonicalize().expect("source directory must resolve"))
			.expect("safe directory must open");

		assert!(fs::rename(&source, &destination).is_err());
		drop(directory);
		fs::rename(&source, &destination).expect("rename must succeed after capability drop");
	}

	#[test]
	fn file_reparse_points_are_rejected_by_every_safe_file_operation() {
		let temp = TempDir::new().expect("temporary directory must be created");
		let target = temp.path().join("target");
		let link = temp.path().join("link");
		fs::write(&target, b"outside").expect("target file must be created");
		match symlink_file(&target, &link) {
			Ok(()) => {
				let root = SafeDir::open_absolute(
					&temp.path().canonicalize().expect("temporary directory must resolve"),
				)
				.expect("safe directory must open");
				assert!(root.symlink_metadata("link").is_err());
				assert!(root.open_regular("link").is_err());
				assert!(root.sync_file("link").is_err());
				assert!(root.rename_to("link", &root, "renamed").is_err());
				assert_eq!(fs::read(target).expect("target must remain readable"), b"outside");
			}
			Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {}
			Err(error) => panic!("unexpected symlink creation error: {error}"),
		}
	}

	#[test]
	fn raw_reparse_attribute_is_rejected() {
		assert!(!has_reparse_attribute(0));
		assert!(has_reparse_attribute(FILE_ATTRIBUTE_REPARSE_POINT.0));
	}
}

#[cfg(test)]
#[expect(
	clippy::expect_used,
	reason = "test fixture failures should report their exact setup step"
)]
mod bounded_tests {
	use super::EntryBudget;
	use super::MAX_DIRECTORY_ENTRIES;
	use super::MAX_SYNC_ENTRIES;
	use super::MAX_TRAVERSAL_DEPTH;
	use super::READ_CHUNK_BYTES;
	use super::SafeDir;
	use super::read_bounded;
	use super::sync_tree_inner;
	use application::ErrorCode;
	use application::ErrorMarker;
	use std::fs;
	use std::io;
	use tempfile::TempDir;
	use tokio_util::sync::CancellationToken;

	#[test]
	fn resource_caps_are_deliberate() {
		assert_eq!(READ_CHUNK_BYTES, 64 * 1024);
		assert_eq!(MAX_DIRECTORY_ENTRIES, 16_384);
		assert_eq!(MAX_SYNC_ENTRIES, 100_000);
		assert_eq!(MAX_TRAVERSAL_DEPTH, 64);
	}

	#[test]
	fn entry_budget_enforces_total_and_per_directory_caps_with_io_cause() {
		let mut total = EntryBudget::new(1);
		let mut first_directory = 0;
		total.consume(&mut first_directory).expect("first entry must fit");
		let total_error = total
			.consume(&mut first_directory)
			.expect_err("total cap must reject entry");
		assert_eq!(total_error.current_context().kind(), io::ErrorKind::InvalidData);

		let mut per_directory = EntryBudget::new(MAX_DIRECTORY_ENTRIES + 1);
		let mut directory_entries = MAX_DIRECTORY_ENTRIES;
		let directory_error = per_directory
			.consume(&mut directory_entries)
			.expect_err("per-directory cap must reject entry");
		assert_eq!(directory_error.current_context().kind(), io::ErrorKind::InvalidData);
	}

	#[test]
	fn tree_sync_rejects_exhausted_depth_with_io_cause() {
		let temp = TempDir::new().expect("temporary directory must be created");
		fs::write(temp.path().join("entry"), b"contents").expect("fixture file must be written");
		let directory =
			SafeDir::open_absolute(&temp.path().canonicalize().expect("temporary directory must resolve"))
				.expect("safe directory must open");
		let mut budget = EntryBudget::new(MAX_SYNC_ENTRIES);

		let error = sync_tree_inner(
			&directory,
			ErrorMarker::environment_invalid(None),
			&CancellationToken::new(),
			&mut budget,
			0,
		)
		.expect_err("exhausted depth must reject a tree entry");

		assert!(error.iter_reports().any(|report| {
			report.downcast_current_context::<io::Error>()
				.is_some_and(|error| error.kind() == io::ErrorKind::InvalidData)
		}));
	}

	#[test]
	fn bounded_read_caps_cancels_and_preserves_io_causes() {
		let temp = TempDir::new().expect("temporary directory must be created");
		fs::write(temp.path().join("large"), b"12345").expect("fixture file must be written");
		let directory =
			SafeDir::open_absolute(&temp.path().canonicalize().expect("temporary directory must resolve"))
				.expect("safe directory must open");
		let cancellation = CancellationToken::new();

		let capped = read_bounded(
			&directory,
			"large",
			4,
			ErrorMarker::environment_invalid(None),
			&cancellation,
		)
		.expect_err("oversized file must be rejected");
		assert!(capped.iter_reports().any(|report| {
			report.downcast_current_context::<io::Error>()
				.is_some_and(|error| error.kind() == io::ErrorKind::InvalidData)
		}));

		let missing = read_bounded(
			&directory,
			"missing",
			4,
			ErrorMarker::environment_invalid(None),
			&cancellation,
		)
		.expect_err("missing file must preserve its IO error");
		assert!(missing.iter_reports().any(|report| {
			report.downcast_current_context::<io::Error>()
				.is_some_and(|error| error.kind() == io::ErrorKind::NotFound)
		}));

		cancellation.cancel();
		let cancelled = read_bounded(
			&directory,
			"large",
			4,
			ErrorMarker::environment_invalid(None),
			&cancellation,
		)
		.expect_err("cancelled read must stop");
		assert_eq!(cancelled.current_context().code(), ErrorCode::OperationCancelled);
	}

	#[test]
	fn durable_rename_moves_cross_parent_then_same_parent_entries() {
		let temp = TempDir::new().expect("temporary directory must be created");
		fs::create_dir(temp.path().join("source")).expect("source directory must be created");
		fs::create_dir(temp.path().join("destination")).expect("destination directory must be created");
		fs::write(temp.path().join("source/original"), b"contents").expect("source file must be written");
		let source = SafeDir::open_absolute(
			&temp.path()
				.join("source")
				.canonicalize()
				.expect("source directory must resolve"),
		)
		.expect("source directory must open");
		let destination = SafeDir::open_absolute(
			&temp.path()
				.join("destination")
				.canonicalize()
				.expect("destination directory must resolve"),
		)
		.expect("destination directory must open");

		source.rename_durable_to("original", &destination, "moved")
			.expect("cross-parent rename must become durable");
		assert!(!temp.path().join("source/original").exists());
		assert_eq!(
			fs::read(temp.path().join("destination/moved")).expect("moved file must read"),
			b"contents"
		);

		destination
			.rename_durable_to("moved", &destination, "renamed")
			.expect("same-parent rename must become durable");
		assert!(!temp.path().join("destination/moved").exists());
		assert_eq!(
			fs::read(temp.path().join("destination/renamed")).expect("renamed file must read"),
			b"contents"
		);
	}

	#[test]
	fn durable_rename_preserves_destination_when_source_validation_fails() {
		let temp = TempDir::new().expect("temporary directory must be created");
		fs::create_dir(temp.path().join("source")).expect("source directory must be created");
		fs::create_dir(temp.path().join("destination")).expect("destination directory must be created");
		fs::write(temp.path().join("destination/existing"), b"existing")
			.expect("destination file must be written");
		let source = SafeDir::open_absolute(
			&temp.path()
				.join("source")
				.canonicalize()
				.expect("source directory must resolve"),
		)
		.expect("source directory must open");
		let destination = SafeDir::open_absolute(
			&temp.path()
				.join("destination")
				.canonicalize()
				.expect("destination directory must resolve"),
		)
		.expect("destination directory must open");

		let error = source
			.rename_durable_to("missing", &destination, "existing")
			.expect_err("missing source must fail before the destination changes");
		assert_eq!(error.current_context().kind(), io::ErrorKind::NotFound);
		assert_eq!(
			fs::read(temp.path().join("destination/existing"))
				.expect("destination file must remain readable"),
			b"existing"
		);
	}
}
