use cap_fs_ext::DirExt;
use cap_fs_ext::FollowSymlinks;
use cap_fs_ext::OpenOptionsFollowExt;
use cap_std::ambient_authority;
use cap_std::fs::Dir;
use cap_std::fs::Metadata;
#[cfg(windows)]
use cap_std::fs::MetadataExt;
use cap_std::fs::OpenOptions;
#[cfg(windows)]
use cap_std::fs::OpenOptionsExt;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use rootcause::report;
use same_file::Handle;
use std::ffi::OsStr;
use std::ffi::OsString;
use std::io;
use std::io::Read;
use std::io::Write;
use std::path::Component;
use std::path::Path;
#[cfg(windows)]
use std::path::PathBuf;
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

	pub(crate) fn entries(&self) -> Result<Vec<OsString>, io::Error> {
		self.inner
			.entries()
			.into_report()?
			.map(|entry| entry.map(|entry| entry.file_name()))
			.collect::<io::Result<Vec<_>>>()
			.into_report()
	}

	pub(crate) fn symlink_metadata(&self, name: impl AsRef<Path>) -> Result<Metadata, io::Error> {
		let metadata = self
			.inner
			.symlink_metadata(checked_component(name.as_ref())?)
			.into_report()?;
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

	pub(crate) fn write_new(&self, name: impl AsRef<Path>, contents: &[u8]) -> Result<(), io::Error> {
		let name = checked_component(name.as_ref())?;
		let mut options = OpenOptions::new();
		options.write(true).create_new(true).follow(FollowSymlinks::No);
		let mut file = self.inner.open_with(name, &options).into_report()?;
		let metadata = file.metadata().into_report()?;
		if !metadata.is_file() || is_reparse(&metadata) {
			return Err(report!(io::Error::other(
				"created path is a reparse point or non-regular file",
			)));
		}
		file.write_all(contents).into_report()?;
		file.sync_all().into_report()
	}

	pub(crate) fn read_regular(&self, name: impl AsRef<Path>) -> Result<Vec<u8>, io::Error> {
		let name = checked_component(name.as_ref())?;
		let mut options = OpenOptions::new();
		options.read(true).follow(FollowSymlinks::No);
		let mut file = self.inner.open_with(name, &options).into_report()?;
		let metadata = file.metadata().into_report()?;
		if !metadata.is_file() || is_reparse(&metadata) {
			return Err(report!(io::Error::other(
				"opened path is a reparse point or non-regular file",
			)));
		}
		let mut contents = Vec::new();
		file.read_to_end(&mut contents).into_report()?;
		Ok(contents)
	}

	pub(crate) fn sync_file(&self, name: impl AsRef<Path>) -> Result<(), io::Error> {
		let name = checked_component(name.as_ref())?;
		let mut options = OpenOptions::new();
		options.read(true).write(true).follow(FollowSymlinks::No);
		let file = self.inner.open_with(name, &options).into_report()?;
		let metadata = file.metadata().into_report()?;
		if !metadata.is_file() || is_reparse(&metadata) {
			return Err(report!(io::Error::other(
				"opened path is a reparse point or non-regular file",
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

	pub(crate) fn remove_file(&self, name: impl AsRef<Path>) -> Result<(), io::Error> {
		let name = checked_component(name.as_ref())?;
		let metadata = self.symlink_metadata(name)?;
		if !metadata.is_file() {
			return Err(report!(io::Error::other("refusing to remove a non-regular file")));
		}
		self.inner.remove_file(name).into_report()
	}

	pub(crate) fn remove_dir(&self, name: impl AsRef<Path>) -> Result<(), io::Error> {
		self.inner.remove_dir(checked_component(name.as_ref())?).into_report()
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
		match component {
			Component::Normal(name) => components.push(name.to_owned()),
			_ => return Err(report!(invalid_path())),
		}
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

#[cfg(all(test, windows))]
#[expect(
	clippy::expect_used,
	reason = "test fixture failures should report their exact setup step"
)]
#[expect(clippy::panic, reason = "unexpected Windows fixture errors must fail the test")]
mod tests {
	use super::*;
	use std::fs;
	use std::os::windows::fs::symlink_file;
	use tempfile::TempDir;

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
				assert!(root.read_regular("link").is_err());
				assert!(root.sync_file("link").is_err());
				assert!(root.remove_file("link").is_err());
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
