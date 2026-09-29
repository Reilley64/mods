use crate::EnvironmentAdapter;
use crate::PreparedExecution;
use crate::derived_profile::ProfileIniInputs;
use crate::files::read_optional;
use crate::profile::PROFILE_FILES;
use application::ErrorMarker;
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
use std::io;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;
use tokio::fs::OpenOptions;
use tokio::fs::canonicalize;
use tokio::fs::copy;
use tokio::fs::create_dir;
use tokio::fs::create_dir_all;
use tokio::fs::metadata;
use tokio::fs::read_dir;
use tokio::fs::try_exists;
use tokio::fs::write;
use tokio::task::spawn_blocking;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, PartialEq, Eq)]
struct ExportSource {
	path: PathBuf,
	modified: SystemTime,
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
	/// Sources are not compared again before they are copied.
	/// Preparing a dry-run inventory creates no output and reserves no destination.
	pub fn prepare_export_port(&self, root: EnvironmentRoot, binding: GameBinding) -> PrepareExport {
		Arc::new(move |output, include_saves, cancellation| {
			let root = root.clone();
			let binding = binding.clone();
			Box::pin(async move {
				validate_destination(&root, &output).await?;

				let snapshot = capture(&root, &binding, include_saves, &cancellation).await?;
				let files = snapshot.sources.iter().map(|source| source.entry.clone()).collect();

				let publish = Arc::new(move |files, cancellation| {
					let root = root.clone();
					let output = output.clone();
					let snapshot = snapshot.clone();
					Box::pin(async move {
						publish(&root, &output, include_saves, &snapshot, files, &cancellation)
							.await
					}) as PortFuture<_>
				});
				Ok(PreparedExport { files, publish })
			}) as PortFuture<_>
		})
	}
}

/// Requires a new output directory outside the environment, so export never copies into its own sources.
async fn validate_destination(root: &EnvironmentRoot, output: &Path) -> Result<(), ErrorMarker> {
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

	if try_exists(output).await.context(ErrorMarker::io_failure())? {
		return Err(report!(ErrorMarker::environment_already_initialized()));
	}

	let parent = canonicalize(parent)
		.await
		.context(ErrorMarker::environment_root_unsafe())?;
	let root = canonicalize(root.as_path())
		.await
		.context(ErrorMarker::environment_root_unsafe())?;

	if parent.starts_with(root) {
		return Err(report!(ErrorMarker::environment_root_unsafe()));
	}
	Ok(())
}

async fn capture(
	root: &EnvironmentRoot,
	binding: &GameBinding,
	include_saves: bool,
	cancellation: &CancellationToken,
) -> Result<ExportSnapshot, ErrorMarker> {
	let execution = EnvironmentAdapter
		.prepare_execution(root, binding, cancellation)
		.await?;
	let profile = &execution.profile_directory;
	let inis = ProfileIniInputs::read(profile, binding.game_directory().as_path(), cancellation).await?;

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
		)
		.await?;
	}

	for name in PROFILE_FILES.into_iter().chain(["modlist.txt"]) {
		let Some(bytes) = read_optional(&profile.join(name))
			.await
			.context(ErrorMarker::io_failure())?
		else {
			continue;
		};

		let derived = if name.ends_with(".ini") {
			Some(inis.derive(name, &bytes, ProfileIniPurpose::Export)?)
		} else {
			None
		};

		capture_file(
			&mut sources,
			profile.join(name),
			format!("profile/{name}"),
			ExportProvider::Profile,
			derived,
			cancellation,
		)
		.await?;
	}

	capture_file(
		&mut sources,
		execution.cache_directory.join("Fallout - Invalidation.bsa"),
		"Data/Fallout - Invalidation.bsa".to_owned(),
		ExportProvider::GeneratedInvalidation,
		None,
		cancellation,
	)
	.await?;

	if include_saves {
		capture_saves(&mut sources, &profile.join("saves"), "profile/saves", cancellation).await?;
	}

	Ok(ExportSnapshot {
		execution,
		inis,
		sources,
	})
}

async fn capture_file(
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

	let metadata = metadata(&path).await.context(ErrorMarker::io_failure())?;
	if !metadata.is_file() {
		return Err(report!(ErrorMarker::environment_root_unsafe()));
	}

	let modified = metadata.modified().context(ErrorMarker::io_failure())?;

	let entry = ExportFile {
		source_id: sources.len(),
		path: DataRelativePath::new(destination).context(ErrorMarker::invalid_data_path())?,
		provider,
		bytes: derived.as_ref().map_or(metadata.len(), |bytes| bytes.len() as u64),
	};

	sources.push(ExportSource {
		path,
		modified,
		derived,
		entry,
	});
	Ok(())
}

async fn capture_saves(
	sources: &mut Vec<ExportSource>,
	path: &Path,
	destination: &str,
	cancellation: &CancellationToken,
) -> Result<(), ErrorMarker> {
	let mut entries = read_dir(path).await.context(ErrorMarker::io_failure())?;
	let mut names = Vec::new();
	while let Some(entry) = entries.next_entry().await.context(ErrorMarker::io_failure())? {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}

		let name = entry
			.file_name()
			.into_string()
			.map_err(|_| report!(ErrorMarker::invalid_data_path()))?;
		names.push(name);
	}
	names.sort();

	for name in names {
		let child = path.join(&name);
		if metadata(&child)
			.await
			.context(ErrorMarker::environment_root_unsafe())?
			.is_dir()
		{
			Box::pin(capture_saves(
				sources,
				&child,
				&format!("{destination}/{name}"),
				cancellation,
			))
			.await?;
		} else {
			capture_file(
				sources,
				child,
				format!("{destination}/{name}"),
				ExportProvider::Profile,
				None,
				cancellation,
			)
			.await?;
		}
	}
	Ok(())
}

async fn publish(
	root: &EnvironmentRoot,
	output: &Path,
	include_saves: bool,
	snapshot: &ExportSnapshot,
	files: Vec<ExportFile>,
	cancellation: &CancellationToken,
) -> Result<(), ErrorMarker> {
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}

	validate_destination(root, output).await?;
	create_dir(output).await.context(ErrorMarker::io_failure())?;

	let result = copy_files(output, include_saves, snapshot, files, cancellation).await;
	result.map_err(|mut error| {
		error.children_mut().push(report!(RetainedExport {
			path: output.to_owned()
		})
		.into_dynamic()
		.into_cloneable());
		error
	})
}

/// Writes every selected file directly into the output directory, in order.
async fn copy_files(
	output: &Path,
	include_saves: bool,
	snapshot: &ExportSnapshot,
	files: Vec<ExportFile>,
	cancellation: &CancellationToken,
) -> Result<(), ErrorMarker> {
	create_dir(output.join("Data"))
		.await
		.context(ErrorMarker::io_failure())?;
	create_dir(output.join("profile"))
		.await
		.context(ErrorMarker::io_failure())?;
	if include_saves {
		create_dir(output.join("profile/saves"))
			.await
			.context(ErrorMarker::io_failure())?;
	}

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

		let destination = entry
			.path
			.components()
			.fold(output.to_path_buf(), |path, component| path.join(component));

		if let Some(parent) = destination.parent() {
			create_dir_all(parent).await.context(ErrorMarker::io_failure())?;
		}

		if let Some(bytes) = &source.derived {
			write(&destination, bytes).await.context(ErrorMarker::io_failure())?;
		} else {
			copy(&source.path, &destination)
				.await
				.context(ErrorMarker::io_failure())?;
		}

		set_modified(&destination, source.modified).await?;
	}
	Ok(())
}

/// Gives the copy the source modification time, which the game uses to order archives and plugins.
async fn set_modified(path: &Path, modified: SystemTime) -> Result<(), ErrorMarker> {
	let file = OpenOptions::new()
		.write(true)
		.open(path)
		.await
		.context(ErrorMarker::io_failure())?
		.into_std()
		.await;

	spawn_blocking(move || file.set_modified(modified))
		.await
		.map_err(io::Error::other)
		.context(ErrorMarker::io_failure())?
		.context(ErrorMarker::io_failure())
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
	use std::fs;
	use tempfile::TempDir;
	use tokio::runtime::Builder as RuntimeBuilder;

	async fn fixture() -> Result<(TempDir, EnvironmentRoot, GameBinding, PathBuf), ErrorMarker> {
		let temp = TempDir::new().context(ErrorMarker::io_failure())?;
		let parent = temp.path().canonicalize().context(ErrorMarker::io_failure())?;
		let root = EnvironmentRoot::new(parent.join("environment"))
			.context(ErrorMarker::environment_root_unsafe())?;
		let game = parent.join("game");
		fs::create_dir_all(game.join("Data")).context(ErrorMarker::io_failure())?;
		fs::write(game.join("Data/FalloutNV.esm"), b"base").context(ErrorMarker::io_failure())?;
		let binding =
			GameBinding::new(GameInstallationPath::new(game).context(ErrorMarker::game_install_invalid())?);
		EnvironmentAdapter
			.publish(
				&root,
				InitializationPlan {
					game_binding: binding.clone(),
					profile_sources: InitializationProfileSources {
						files: PROFILE_FILES
							.into_iter()
							.map(|name| ProfileSource { name, contents: None })
							.collect(),
						fallout_default_ini: b"[Archive]\r\nsArchiveList=Original.bsa\r\n"
							.to_vec(),
					},
				},
				&CancellationToken::new(),
			)
			.await?;
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
				let (_temp, root, binding, output) = fixture().await?;
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

	#[tokio::test]
	async fn export_keeps_the_game_multi_line_warning_in_derived_inis() -> Result<(), ErrorMarker> {
		let (_temp, root, binding, _output) = fixture().await?;
		let warning = concat!(
			"SMasterMismatchWarning=One of the files that \"%s\" is dependent on has changed since the last save.\r\n",
			"This may result in errors. Saving again will clear this message\r\n",
			"but not necessarily fix any errors.\r\n"
		);
		let profile = root.as_path().join("profile");
		let fallout = format!(
			"[General]\r\n{warning}bUseMyGamesDirectory=1\r\nSLocalSavePath=Saves\\\r\n[Archive]\r\nsArchiveList=Original.bsa\r\n"
		);
		fs::write(profile.join("Fallout.ini"), fallout).context(ErrorMarker::io_failure())?;
		fs::write(profile.join("FalloutPrefs.ini"), format!("[General]\r\n{warning}"))
			.context(ErrorMarker::io_failure())?;

		let snapshot = capture(&root, &binding, false, &CancellationToken::new()).await?;

		for name in ["profile/Fallout.ini", "profile/FalloutPrefs.ini"] {
			let derived = snapshot
				.sources
				.iter()
				.find(|source| source.entry.path.as_str() == name)
				.and_then(|source| source.derived.as_ref())
				.ok_or_else(|| report!(ErrorMarker::io_failure()))?;
			let derived = String::from_utf8(derived.clone()).context(ErrorMarker::io_failure())?;
			assert!(derived.contains(warning));
		}
		Ok(())
	}

	#[tokio::test]
	async fn existing_outputs_and_outputs_inside_the_environment_are_rejected() -> Result<(), ErrorMarker> {
		let (_temp, root, _binding, output) = fixture().await?;
		assert!(validate_destination(&root, &root.as_path().join("export"))
			.await
			.is_err());
		assert!(validate_destination(&root, &root.as_path().join("mods/export"))
			.await
			.is_err());
		validate_destination(&root, &output).await?;
		fs::create_dir(&output).context(ErrorMarker::io_failure())?;
		assert!(validate_destination(&root, &output).await.is_err());
		Ok(())
	}

	#[tokio::test]
	async fn export_writes_bytes_and_times_directly_to_the_output() -> Result<(), ErrorMarker> {
		let (_temp, root, binding, output) = fixture().await?;
		let cancellation = CancellationToken::new();
		let snapshot = capture(&root, &binding, true, &cancellation).await?;
		let files = snapshot.sources.iter().map(|source| source.entry.clone()).collect();

		publish(&root, &output, true, &snapshot, files, &cancellation).await?;

		for source in &snapshot.sources {
			let written = output.join(source.entry.path.as_str());
			assert_eq!(
				fs::metadata(&written)
					.context(ErrorMarker::io_failure())?
					.modified()
					.context(ErrorMarker::io_failure())?,
				source.modified
			);
			if let Some(bytes) = &source.derived {
				assert_eq!(fs::read(&written).context(ErrorMarker::io_failure())?, *bytes);
			} else {
				assert_eq!(
					fs::read(&written).context(ErrorMarker::io_failure())?,
					fs::read(&source.path).context(ErrorMarker::io_failure())?
				);
			}
		}
		let ini = fs::read_to_string(output.join("profile/Fallout.ini")).context(ErrorMarker::io_failure())?;
		assert!(ini.contains("SLocalSavePath=Saves\\"));
		assert!(ini.contains("sArchiveList=Fallout - Invalidation.bsa, Original.bsa"));
		Ok(())
	}
}
