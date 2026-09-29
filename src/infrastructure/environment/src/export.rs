use crate::EnvironmentAdapter;
use crate::PreparedExecution;
use crate::derived_profile::ProfileIniInputs;
use crate::export_publication::publish_no_replace;
use crate::hashing::sha256;
use crate::profile::MAX_PROFILE_BYTES;
use crate::profile::PROFILE_FILES;
use crate::safe_fs::EntryBudget;
use crate::safe_fs::MAX_TRAVERSAL_DEPTH;
use crate::safe_fs::READ_CHUNK_BYTES;
use crate::safe_fs::SafeDir;
use crate::safe_fs::SafeFile;
use crate::safe_fs::read_bounded;
use application::ErrorMarker;
use application::conflicts::ConflictContentRead;
use application::export::ExportFile;
use application::export::ExportProvider;
use application::export::PrepareExport;
use application::export::PreparedExport;
use application::export::RetainedExport;
use application::ports::PortFuture;
use domain::DataRelativePath;
use domain::EnvironmentRoot;
use domain::GameBinding;
use domain::ProfileIniPurpose;
use domain::ProviderIdentity;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use rootcause::report;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;
use tempfile::Builder;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, PartialEq, Eq)]
struct ExportSource {
	path: PathBuf,
	length: u64,
	modified: SystemTime,
	fingerprint: ConflictContentRead,
	derived: Option<Vec<u8>>,
	entry: ExportFile,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ExportSnapshot {
	execution: PreparedExecution,
	inis: ProfileIniInputs,
	sources: Vec<ExportSource>,
}

impl EnvironmentAdapter {
	/// The caller must validate the effective Game Binding and stop source writers.
	/// Observable changes are rejected; this is not a coherent concurrent snapshot.
	/// Preparing a dry-run inventory creates no output and reserves no destination.
	pub fn prepare_export_port(&self, root: EnvironmentRoot, binding: GameBinding) -> PrepareExport {
		Arc::new(move |output, include_saves, cancellation| {
			let root = root.clone();
			let binding = binding.clone();
			Box::pin(async move {
				validate_destination(&root, &binding, &output)?;
				let snapshot = capture(&root, &binding, include_saves, &cancellation)?;
				let files = snapshot.sources.iter().map(|source| source.entry.clone()).collect();
				let publish = Arc::new(move |files, cancellation| {
					let root = root.clone();
					let binding = binding.clone();
					let output = output.clone();
					let snapshot = snapshot.clone();
					Box::pin(async move {
						publish(
							&root,
							&binding,
							&output,
							include_saves,
							&snapshot,
							files,
							&cancellation,
						)
					}) as PortFuture<_>
				});
				Ok(PreparedExport { files, publish })
			}) as PortFuture<_>
		})
	}
}

fn validate_destination(root: &EnvironmentRoot, binding: &GameBinding, output: &Path) -> Result<(), ErrorMarker> {
	if !output.is_absolute() {
		return Err(report!(ErrorMarker::invalid_data_path()));
	}
	let parent = output
		.parent()
		.ok_or_else(|| report!(ErrorMarker::invalid_data_path()))?;
	let name = output
		.file_name()
		.and_then(|name| name.to_str())
		.ok_or_else(|| report!(ErrorMarker::invalid_data_path()))?;
	let relative = DataRelativePath::new(name.to_owned()).context(ErrorMarker::invalid_data_path())?;
	if relative.components().count() != 1 {
		return Err(report!(ErrorMarker::invalid_data_path()));
	}
	let parent = SafeDir::open_absolute(parent).context(ErrorMarker::environment_root_unsafe())?;
	if parent.exists(name).context(ErrorMarker::io_failure())? {
		return Err(report!(ErrorMarker::environment_already_initialized()));
	}
	for source in [root.as_path(), binding.game_directory().as_path()] {
		let source = SafeDir::open_absolute(source).context(ErrorMarker::environment_root_unsafe())?;
		if source
			.is_ancestor_of(&parent)
			.context(ErrorMarker::environment_root_unsafe())?
		{
			return Err(report!(ErrorMarker::environment_root_unsafe()));
		}
	}
	Ok(())
}

fn capture(
	root: &EnvironmentRoot,
	binding: &GameBinding,
	include_saves: bool,
	cancellation: &CancellationToken,
) -> Result<ExportSnapshot, ErrorMarker> {
	let execution = EnvironmentAdapter.prepare_execution(root, binding, cancellation)?;
	let profile =
		SafeDir::open_absolute(&execution.profile_directory).context(ErrorMarker::environment_invalid(None))?;
	let inis = ProfileIniInputs::read(&profile, binding.game_directory().as_path(), cancellation)?;
	let mut sources = Vec::new();
	for (winner, file) in execution.winners.iter().zip(&execution.visible_files) {
		if winner.identity() == ProviderIdentity::SteamData {
			continue;
		}
		capture_file(
			&mut sources,
			file.physical_path.clone(),
			format!("Data/{}", file.path),
			ExportProvider::Data(winner.identity()),
			None,
			cancellation,
		)?;
	}

	for name in PROFILE_FILES.into_iter().chain(["modlist.txt"]) {
		if !profile.exists(name).context(ErrorMarker::io_failure())? {
			continue;
		}
		let derived = if name.ends_with(".ini") {
			let bytes = read_bounded(
				&profile,
				name,
				MAX_PROFILE_BYTES,
				ErrorMarker::io_failure(),
				cancellation,
			)?;
			Some(inis.derive(name, &bytes, ProfileIniPurpose::Export)?)
		} else {
			None
		};
		capture_file(
			&mut sources,
			execution.profile_directory.join(name),
			format!("profile/{name}"),
			ExportProvider::Profile,
			derived,
			cancellation,
		)?;
	}

	capture_file(
		&mut sources,
		execution.cache_directory.join("Fallout - Invalidation.bsa"),
		"Data/Fallout - Invalidation.bsa".to_owned(),
		ExportProvider::GeneratedInvalidation,
		None,
		cancellation,
	)?;

	if include_saves {
		let mut budget = EntryBudget::new(100_000);
		capture_saves(
			&mut sources,
			&execution.profile_directory.join("saves"),
			"profile/saves",
			&mut budget,
			MAX_TRAVERSAL_DEPTH,
			cancellation,
		)?;
	}

	Ok(ExportSnapshot {
		execution,
		inis,
		sources,
	})
}

fn open_source(path: &Path) -> Result<SafeFile, ErrorMarker> {
	let parent = path.parent().ok_or_else(|| report!(ErrorMarker::invalid_data_path()))?;
	let name = path
		.file_name()
		.ok_or_else(|| report!(ErrorMarker::invalid_data_path()))?;
	SafeDir::open_absolute(parent)
		.context(ErrorMarker::environment_root_unsafe())?
		.open_regular(name)
		.context(ErrorMarker::environment_root_unsafe())
}

fn capture_file(
	sources: &mut Vec<ExportSource>,
	path: PathBuf,
	destination: String,
	provider: ExportProvider,
	derived: Option<Vec<u8>>,
	cancellation: &CancellationToken,
) -> Result<(), ErrorMarker> {
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}

	let file = open_source(&path)?;
	let metadata = file.metadata().context(ErrorMarker::io_failure())?;
	let modified = metadata.modified().context(ErrorMarker::io_failure())?.into_std();
	let fingerprint = sha256(file, cancellation)?;
	if !matches!(fingerprint, ConflictContentRead::Sha256(_)) {
		return Err(report!(ErrorMarker::environment_invalid(Some("export_source_changed"))));
	}
	let entry = ExportFile {
		source_id: sources.len(),
		path: DataRelativePath::new(destination).context(ErrorMarker::invalid_data_path())?,
		provider,
		bytes: derived.as_ref().map_or(metadata.len(), |bytes| bytes.len() as u64),
	};
	sources.push(ExportSource {
		path,
		length: metadata.len(),
		modified,
		fingerprint,
		derived,
		entry,
	});
	Ok(())
}

fn capture_saves(
	sources: &mut Vec<ExportSource>,
	path: &Path,
	destination: &str,
	budget: &mut EntryBudget,
	depth: usize,
	cancellation: &CancellationToken,
) -> Result<(), ErrorMarker> {
	if depth == 0 {
		return Err(report!(ErrorMarker::environment_invalid(None)));
	}

	let directory = SafeDir::open_absolute(path).context(ErrorMarker::environment_root_unsafe())?;
	let mut count = 0;
	let mut names = Vec::new();
	for entry in directory.entries().context(ErrorMarker::io_failure())? {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}
		budget.consume(&mut count).context(ErrorMarker::io_failure())?;
		let name = entry
			.context(ErrorMarker::io_failure())?
			.file_name()
			.into_string()
			.map_err(|_| report!(ErrorMarker::invalid_data_path()))?;
		names.push(name);
	}
	names.sort();
	for name in names {
		let metadata = directory
			.symlink_metadata(&name)
			.context(ErrorMarker::environment_root_unsafe())?;
		if metadata.is_dir() {
			capture_saves(
				sources,
				&path.join(&name),
				&format!("{destination}/{name}"),
				budget,
				depth - 1,
				cancellation,
			)?;
		} else if metadata.is_file() {
			capture_file(
				sources,
				path.join(&name),
				format!("{destination}/{name}"),
				ExportProvider::Profile,
				None,
				cancellation,
			)?;
		} else {
			return Err(report!(ErrorMarker::environment_root_unsafe()));
		}
	}
	Ok(())
}

fn publish(
	root: &EnvironmentRoot,
	binding: &GameBinding,
	output: &Path,
	include_saves: bool,
	snapshot: &ExportSnapshot,
	files: Vec<ExportFile>,
	cancellation: &CancellationToken,
) -> Result<(), ErrorMarker> {
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}

	validate_destination(root, binding, output)?;
	let parent = output
		.parent()
		.ok_or_else(|| report!(ErrorMarker::invalid_data_path()))?;
	let partial = Builder::new()
		.prefix(".mods-export-")
		.tempdir_in(parent)
		.context(ErrorMarker::io_failure())?
		.keep();

	let result = copy_and_publish(root, output, &partial, include_saves, snapshot, files, cancellation);
	result.map_err(|mut error| {
		error.children_mut().push(report!(RetainedExport { path: partial })
			.into_dynamic()
			.into_cloneable());
		error
	})
}

fn copy_and_publish(
	root: &EnvironmentRoot,
	output: &Path,
	partial: &Path,
	include_saves: bool,
	snapshot: &ExportSnapshot,
	files: Vec<ExportFile>,
	cancellation: &CancellationToken,
) -> Result<(), ErrorMarker> {
	let binding = &snapshot.execution.game_binding;
	let stage = SafeDir::open_absolute(partial).context(ErrorMarker::io_failure())?;
	stage.create_dir("Data").context(ErrorMarker::io_failure())?;
	let profile_stage = stage.create_dir("profile").context(ErrorMarker::io_failure())?;
	if include_saves {
		profile_stage.create_dir("saves").context(ErrorMarker::io_failure())?;
	}
	drop(profile_stage);

	for entry in files {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}
		let source = snapshot
			.sources
			.get(entry.source_id)
			.ok_or_else(|| report!(ErrorMarker::invalid_data_path()))?;
		if source.entry.bytes != entry.bytes
			|| source.entry.provider != entry.provider
			|| source.entry.path.comparison_key() != entry.path.comparison_key()
		{
			return Err(report!(ErrorMarker::invalid_data_path()));
		}
		let mut parent = SafeDir::open_absolute(partial).context(ErrorMarker::io_failure())?;
		let components: Vec<_> = entry.path.components().collect();
		for directory in components.iter().take(components.len() - 1) {
			parent = parent.ensure_dir(directory).context(ErrorMarker::io_failure())?;
		}
		let mut destination = parent
			.create_new_file(components[components.len() - 1])
			.context(ErrorMarker::io_failure())?;
		if let Some(bytes) = &source.derived {
			destination.write_chunk(bytes).context(ErrorMarker::io_failure())?;
		} else {
			let mut input = open_source(&source.path)?;
			let mut buffer = [0; READ_CHUNK_BYTES];
			loop {
				if cancellation.is_cancelled() {
					return Err(report!(ErrorMarker::operation_cancelled()));
				}
				let count = input.read_chunk(&mut buffer).context(ErrorMarker::io_failure())?;
				if count == 0 {
					break;
				}
				#[cfg(test)]
				tests::before_copy_write().context(ErrorMarker::io_failure())?;
				destination
					.write_chunk(&buffer[..count])
					.context(ErrorMarker::io_failure())?;
			}
		}
		#[cfg(test)]
		tests::before_set_modified().context(ErrorMarker::io_failure())?;
		destination
			.set_modified(source.modified)
			.context(ErrorMarker::io_failure())?;
		destination.finish().context(ErrorMarker::io_failure())?;
		drop(destination);
		if source.derived.is_none()
			&& sha256(
				parent.open_regular(components[components.len() - 1])
					.context(ErrorMarker::io_failure())?,
				cancellation,
			)? != source.fingerprint
		{
			return Err(report!(ErrorMarker::environment_invalid(Some("export_source_changed"))));
		}
	}

	if capture(root, binding, include_saves, cancellation)? != *snapshot {
		return Err(report!(ErrorMarker::environment_invalid(Some("export_source_changed"))));
	}

	validate_destination(root, binding, output)?;
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}

	drop(stage);
	publish_no_replace(partial, output).context(ErrorMarker::io_failure())?;
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;
	use application::export::ExportEnvironmentDependencies;
	use application::export::export_environment;
	use application::ports::InitializationPlan;
	use application::ports::InitializationProfileSources;
	use application::ports::ProfileSource;
	use domain::GameInstallationPath;
	use domain::SteamBuildId;
	use std::cell::Cell;
	use std::fs;
	use std::io;
	#[cfg(unix)]
	use std::os::unix::fs::symlink;
	use tempfile::TempDir;
	use tokio::runtime::Builder as RuntimeBuilder;

	thread_local! {
		static FAIL_WRITE_ON: Cell<Option<usize>> = const { Cell::new(None) };
		static FAIL_TIMESTAMP: Cell<bool> = const { Cell::new(false) };
	}

	pub(super) fn before_copy_write() -> Result<(), io::Error> {
		FAIL_WRITE_ON.with(|remaining| {
			if let Some(count) = remaining.get() {
				remaining.set(count.checked_sub(1));
				if count == 0 {
					return Err(report!(io::Error::new(
						io::ErrorKind::StorageFull,
						"injected export write failure"
					)));
				}
			}
			Ok(())
		})
	}

	pub(super) fn before_set_modified() -> Result<(), io::Error> {
		if FAIL_TIMESTAMP.with(|failure| failure.replace(false)) {
			return Err(report!(io::Error::new(
				io::ErrorKind::PermissionDenied,
				"injected export timestamp failure"
			)));
		}
		Ok(())
	}

	#[test]
	fn mid_copy_write_failure_keeps_partial_bytes_and_typed_retained_path() -> Result<(), ErrorMarker> {
		let (_temp, root, binding, output) = fixture()?;
		let payload = vec![0x5a; READ_CHUNK_BYTES * 3];
		fs::write(root.as_path().join("overwrite/Opaque.bsa"), &payload).context(ErrorMarker::io_failure())?;
		let cancellation = CancellationToken::new();
		let snapshot = capture(&root, &binding, false, &cancellation)?;
		let files = snapshot.sources.iter().map(|source| source.entry.clone()).collect();

		FAIL_WRITE_ON.with(|remaining| remaining.set(Some(1)));
		let Err(error) = publish(&root, &binding, &output, false, &snapshot, files, &cancellation) else {
			return Err(report!(ErrorMarker::io_failure()));
		};

		let retained = error
			.iter_reports()
			.find_map(|cause| cause.downcast_current_context::<RetainedExport>())
			.ok_or_else(|| report!(ErrorMarker::io_failure()))?;
		assert!(!output.exists());
		assert!(retained.path.is_dir());
		assert_eq!(
			fs::read(retained.path.join("Data/Opaque.bsa")).context(ErrorMarker::io_failure())?,
			payload[..READ_CHUNK_BYTES]
		);
		assert_eq!(
			fs::read(root.as_path().join("overwrite/Opaque.bsa")).context(ErrorMarker::io_failure())?,
			payload
		);
		assert!(error.iter_reports().any(|cause| cause
			.downcast_current_context::<io::Error>()
			.is_some_and(|error| error.kind() == io::ErrorKind::StorageFull)));
		Ok(())
	}

	#[test]
	fn timestamp_failure_keeps_copied_bytes_without_final_publication() -> Result<(), ErrorMarker> {
		let (_temp, root, binding, output) = fixture()?;
		let cancellation = CancellationToken::new();
		let snapshot = capture(&root, &binding, false, &cancellation)?;
		let files = snapshot.sources.iter().map(|source| source.entry.clone()).collect();

		FAIL_TIMESTAMP.with(|failure| failure.set(true));
		let Err(error) = publish(&root, &binding, &output, false, &snapshot, files, &cancellation) else {
			return Err(report!(ErrorMarker::io_failure()));
		};

		let retained = error
			.iter_reports()
			.find_map(|cause| cause.downcast_current_context::<RetainedExport>())
			.ok_or_else(|| report!(ErrorMarker::io_failure()))?;
		assert!(!output.exists());
		assert!(retained.path.is_dir());
		assert_eq!(
			fs::read(retained.path.join("Data/Opaque.bsa")).context(ErrorMarker::io_failure())?,
			b"opaque"
		);
		assert!(error.iter_reports().any(|cause| cause
			.downcast_current_context::<io::Error>()
			.is_some_and(|error| error.kind() == io::ErrorKind::PermissionDenied)));
		assert!(!retained.path.join("profile/Fallout.ini").exists());
		Ok(())
	}

	fn fixture() -> Result<(TempDir, EnvironmentRoot, GameBinding, PathBuf), ErrorMarker> {
		let temp = TempDir::new().context(ErrorMarker::io_failure())?;
		let parent = temp.path().canonicalize().context(ErrorMarker::io_failure())?;
		let root = EnvironmentRoot::new(parent.join("environment"))
			.context(ErrorMarker::environment_root_unsafe())?;
		let game = parent.join("game");
		fs::create_dir_all(game.join("Data")).context(ErrorMarker::io_failure())?;
		fs::write(game.join("Data/FalloutNV.esm"), b"base").context(ErrorMarker::io_failure())?;
		let binding = GameBinding::new(
			GameInstallationPath::new(game).context(ErrorMarker::game_install_invalid())?,
			SteamBuildId::new(1).context(ErrorMarker::game_install_invalid())?,
		);
		EnvironmentAdapter.publish(
			&root,
			InitializationPlan {
				game_binding: binding.clone(),
				profile_sources: InitializationProfileSources {
					files: PROFILE_FILES
						.into_iter()
						.map(|name| ProfileSource { name, contents: None })
						.collect(),
					fallout_default_ini: b"[Archive]\r\nsArchiveList=Original.bsa\r\n".to_vec(),
				},
			},
			&CancellationToken::new(),
		)?;
		fs::create_dir_all(root.as_path().join("overwrite/Textures/nested"))
			.context(ErrorMarker::io_failure())?;
		fs::write(root.as_path().join("overwrite/Textures/nested/meta.toml"), b"ordinary")
			.context(ErrorMarker::io_failure())?;
		fs::write(root.as_path().join("overwrite/Opaque.bsa"), b"opaque").context(ErrorMarker::io_failure())?;
		fs::write(root.as_path().join("profile/saves/example.fos"), b"save")
			.context(ErrorMarker::io_failure())?;
		Ok((temp, root, binding, parent.join("output")))
	}

	#[test]
	fn dry_run_is_read_only_and_saves_are_opt_in() -> Result<(), ErrorMarker> {
		RuntimeBuilder::new_current_thread()
			.build()
			.context(ErrorMarker::io_failure())?
			.block_on(async {
				let (_temp, root, binding, output) = fixture()?;
				let canonical = fs::read(root.as_path().join("profile/Fallout.ini"))
					.context(ErrorMarker::io_failure())?;
				let cache = fs::metadata(root.as_path().join("cache/Fallout - Invalidation.bsa"))
					.context(ErrorMarker::io_failure())?
					.modified()
					.context(ErrorMarker::io_failure())?;
				for saves in [false, true] {
					let result = export_environment(
						ExportEnvironmentDependencies {
							prepare_export: EnvironmentAdapter
								.prepare_export_port(root.clone(), binding.clone()),
						},
						output.clone(),
						saves,
						true,
						CancellationToken::new(),
					)
					.await
					.context(ErrorMarker::io_failure())?;
					assert!(!output.exists());
					assert!(result
						.files
						.iter()
						.any(|file| file.path.as_str() == "Data/Textures/nested/meta.toml"));
					assert!(result
						.files
						.iter()
						.any(|file| file.path.as_str() == "Data/Opaque.bsa"));
					assert!(!result
						.files
						.iter()
						.any(|file| file.path.as_str().ends_with("FalloutNV.esm")));
					assert_eq!(
						result.files
							.iter()
							.any(|file| file.path.as_str() == "profile/saves/example.fos"),
						saves
					);
				}
				assert_eq!(
					fs::read(root.as_path().join("profile/Fallout.ini"))
						.context(ErrorMarker::io_failure())?,
					canonical
				);
				assert_eq!(
					fs::metadata(root.as_path().join("cache/Fallout - Invalidation.bsa"))
						.context(ErrorMarker::io_failure())?
						.modified()
						.context(ErrorMarker::io_failure())?,
					cache
				);
				assert_eq!(
					fs::read_dir(root.as_path().join("temp"))
						.context(ErrorMarker::io_failure())?
						.count(),
					0
				);
				Ok(())
			})
	}

	#[test]
	fn changed_sources_and_existing_or_overlapping_destinations_fail() -> Result<(), ErrorMarker> {
		let (_temp, root, binding, output) = fixture()?;
		assert!(validate_destination(&root, &binding, &root.as_path().join("export")).is_err());
		assert!(
			validate_destination(&root, &binding, &binding.game_directory().as_path().join("export"))
				.is_err()
		);
		let cancellation = CancellationToken::new();
		let snapshot = capture(&root, &binding, false, &cancellation)?;
		fs::write(root.as_path().join("overwrite/Opaque.bsa"), b"changed").context(ErrorMarker::io_failure())?;
		let files = snapshot.sources.iter().map(|source| source.entry.clone()).collect();
		let failure = publish(&root, &binding, &output, false, &snapshot, files, &cancellation);
		assert!(failure.is_err());
		assert!(!output.exists());
		fs::create_dir(&output).context(ErrorMarker::io_failure())?;
		assert!(validate_destination(&root, &binding, &output).is_err());
		Ok(())
	}

	#[cfg(not(windows))]
	#[test]
	fn completed_stage_preserves_bytes_times_and_reports_unsupported_publication() -> Result<(), ErrorMarker> {
		let (_temp, root, binding, output) = fixture()?;
		let cancellation = CancellationToken::new();
		let snapshot = capture(&root, &binding, true, &cancellation)?;
		let files = snapshot.sources.iter().map(|source| source.entry.clone()).collect();
		let Err(error) = publish(&root, &binding, &output, true, &snapshot, files, &cancellation) else {
			return Err(report!(ErrorMarker::io_failure()));
		};
		let retained = error
			.iter_reports()
			.find_map(|cause| cause.downcast_current_context::<RetainedExport>())
			.ok_or_else(|| report!(ErrorMarker::io_failure()))?;
		assert!(!output.exists());
		for source in &snapshot.sources {
			let staged = retained.path.join(source.entry.path.as_str());
			assert_eq!(
				fs::metadata(&staged)
					.context(ErrorMarker::io_failure())?
					.modified()
					.context(ErrorMarker::io_failure())?,
				source.modified
			);
			if let Some(bytes) = &source.derived {
				assert_eq!(fs::read(&staged).context(ErrorMarker::io_failure())?, *bytes);
			} else {
				assert_eq!(
					fs::read(&staged).context(ErrorMarker::io_failure())?,
					fs::read(&source.path).context(ErrorMarker::io_failure())?
				);
			}
		}
		let ini = fs::read_to_string(retained.path.join("profile/Fallout.ini"))
			.context(ErrorMarker::io_failure())?;
		assert!(ini.contains("SLocalSavePath=Saves\\"));
		assert!(ini.contains("sArchiveList=Fallout - Invalidation.bsa, Original.bsa"));
		Ok(())
	}

	#[test]
	fn cancellation_retains_owned_stage_without_creating_final_output() -> Result<(), ErrorMarker> {
		let (_temp, root, binding, output) = fixture()?;
		let cancellation = CancellationToken::new();
		let snapshot = capture(&root, &binding, false, &cancellation)?;
		let partial = output.with_file_name("partial");
		fs::create_dir(&partial).context(ErrorMarker::io_failure())?;
		cancellation.cancel();
		let files = snapshot.sources.iter().map(|source| source.entry.clone()).collect();
		let result = copy_and_publish(&root, &output, &partial, false, &snapshot, files, &cancellation);
		assert!(result.is_err());
		assert!(partial.is_dir());
		assert!(!output.exists());
		Ok(())
	}

	#[cfg(unix)]
	#[test]
	fn selected_save_links_are_rejected() -> Result<(), ErrorMarker> {
		let (_temp, root, binding, _output) = fixture()?;
		symlink(
			root.as_path().join("profile/Fallout.ini"),
			root.as_path().join("profile/saves/link"),
		)
		.context(ErrorMarker::io_failure())?;
		assert!(capture(&root, &binding, true, &CancellationToken::new()).is_err());
		Ok(())
	}
}
