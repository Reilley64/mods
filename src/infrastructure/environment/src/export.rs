use crate::EnvironmentAdapter;
use crate::PreparedLaunch;
use crate::files::read_optional;
use crate::profile::PROFILE_FILES;
use application::ErrorMarker;
use application::export::ExportFile;
use application::export::ExportListing;
use application::export::ExportProvider;
use application::export::ExportSelection;
use application::export::ExportSources;
use application::export::ListExportFiles;
use application::export::RetainedExport;
use application::export::ValidateExportDestination;
use application::export::WriteExport;
use application::ports::AdapterState;
use application::ports::EnvironmentPlan;
use application::ports::PortFuture;
use application::ports::StagedProfile;
use domain::DataRelativePath;
use domain::EnvironmentRoot;
use domain::ProviderIdentity;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use rootcause::report;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs::canonicalize;
use tokio::fs::copy;
use tokio::fs::create_dir;
use tokio::fs::create_dir_all;
use tokio::fs::metadata;
use tokio::fs::read_dir;
use tokio::fs::try_exists;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, PartialEq, Eq)]
struct ExportSource {
	path: PathBuf,
	entry: ExportFile,
}

/// The listed sources behind an [`ExportSources`] handle.
struct ExportSourceTable {
	sources: Vec<ExportSource>,
	include_saves: bool,
}

impl EnvironmentAdapter {
	pub fn validate_export_destination_port(&self, root: EnvironmentRoot) -> ValidateExportDestination {
		Arc::new(move |output| {
			let root = root.clone();

			Box::pin(async move { check_destination(&root, &output).await }) as PortFuture<_>
		})
	}

	/// The caller must stop source writers. Sources are not compared again before
	/// they are copied.
	pub fn list_export_files_port(&self) -> ListExportFiles {
		Arc::new(
			|plan: &EnvironmentPlan, staged: &StagedProfile, selection: ExportSelection, cancellation| {
				// The port borrows the plan, so the future owns a copy of the prepared environment.
				let prepared = plan.state.downcast_ref::<PreparedLaunch>().cloned();
				let staged = staged.directory.clone();

				Box::pin(async move {
					let prepared = prepared
						.ok_or_else(|| report!(ErrorMarker::environment_invalid(None)))?;

					let sources =
						list_sources(&prepared, &staged, selection, &cancellation).await?;

					Ok(ExportListing {
						files: sources.iter().map(|source| source.entry.clone()).collect(),
						sources: ExportSources(AdapterState::new(ExportSourceTable {
							sources,
							include_saves: selection.include_saves,
						})),
					})
				}) as PortFuture<_>
			},
		)
	}

	pub fn write_export_port(&self, root: EnvironmentRoot) -> WriteExport {
		Arc::new(move |sources: ExportSources, files, output, cancellation| {
			let root = root.clone();

			Box::pin(async move {
				let table: ExportSourceTable = sources
					.0
					.downcast()
					.ok_or_else(|| report!(ErrorMarker::environment_invalid(None)))?;

				write_output(&root, &output, &table, files, &cancellation).await
			}) as PortFuture<_>
		})
	}
}

/// Requires a new output directory outside the environment, so export never copies into its own sources.
async fn check_destination(root: &EnvironmentRoot, output: &Path) -> Result<(), ErrorMarker> {
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

/// Lists every file the game sees: Data winners, the staged profile INIs, the
/// other profile files, the invalidation archive, and optionally the game's own
/// Data files and saves.
async fn list_sources(
	prepared: &PreparedLaunch,
	staged: &Path,
	selection: ExportSelection,
	cancellation: &CancellationToken,
) -> Result<Vec<ExportSource>, ErrorMarker> {
	let mut sources = Vec::new();
	for (winner, file) in prepared.winners.iter().zip(&prepared.visible_files) {
		if winner.identity() == ProviderIdentity::SteamData && !selection.include_game_data {
			continue;
		}

		capture_file(
			&mut sources,
			file.physical_path.clone(),
			format!("Data/{}", file.path),
			ExportProvider::Data(winner.identity()),
			cancellation,
		)
		.await?;
	}

	let profile = &prepared.profile_directory;
	for name in PROFILE_FILES.into_iter().chain(["modlist.txt"]) {
		// Staged INIs carry the export routing.
		let path = if name.ends_with(".ini") {
			staged.join(name)
		} else {
			profile.join(name)
		};
		if read_optional(&path).await.context(ErrorMarker::io_failure())?.is_none() {
			continue;
		}

		capture_file(
			&mut sources,
			path,
			format!("profile/{name}"),
			ExportProvider::Profile,
			cancellation,
		)
		.await?;
	}

	capture_file(
		&mut sources,
		prepared.cache_directory.join("Fallout - Invalidation.bsa"),
		"Data/Fallout - Invalidation.bsa".to_owned(),
		ExportProvider::GeneratedInvalidation,
		cancellation,
	)
	.await?;

	if selection.include_saves {
		capture_saves(&mut sources, &profile.join("saves"), "profile/saves", cancellation).await?;
	}

	Ok(sources)
}

async fn capture_file(
	sources: &mut Vec<ExportSource>,
	path: PathBuf,
	destination: String,
	provider: ExportProvider,
	cancellation: &CancellationToken,
) -> Result<(), ErrorMarker> {
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}

	let file = metadata(&path).await.context(ErrorMarker::io_failure())?;
	if !file.is_file() {
		return Err(report!(ErrorMarker::environment_root_unsafe()));
	}

	let entry = ExportFile {
		source_id: sources.len(),
		path: DataRelativePath::new(destination).context(ErrorMarker::invalid_data_path())?,
		provider,
		bytes: file.len(),
	};

	sources.push(ExportSource { path, entry });
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
				cancellation,
			)
			.await?;
		}
	}
	Ok(())
}

async fn write_output(
	root: &EnvironmentRoot,
	output: &Path,
	table: &ExportSourceTable,
	files: Vec<ExportFile>,
	cancellation: &CancellationToken,
) -> Result<(), ErrorMarker> {
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}

	check_destination(root, output).await?;
	create_dir(output).await.context(ErrorMarker::io_failure())?;

	let result = copy_files(output, table, files, cancellation).await;
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
	table: &ExportSourceTable,
	files: Vec<ExportFile>,
	cancellation: &CancellationToken,
) -> Result<(), ErrorMarker> {
	create_dir(output.join("Data"))
		.await
		.context(ErrorMarker::io_failure())?;
	create_dir(output.join("profile"))
		.await
		.context(ErrorMarker::io_failure())?;
	if table.include_saves {
		create_dir(output.join("profile/saves"))
			.await
			.context(ErrorMarker::io_failure())?;
	}

	for entry in files {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}

		let source = table
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

		// The copy keeps whatever time the copy gives it: the source time on
		// Windows, the copy time elsewhere. Load-order times are a separate step.
		copy(&source.path, &destination)
			.await
			.context(ErrorMarker::io_failure())?;
	}
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::StagedProfileInis;
	use application::ports::AdapterState;
	use application::ports::EnvironmentPlan;
	use application::ports::InitializationPlan;
	use application::ports::InitializationProfileSources;
	use application::ports::LoadOrderTarget;
	use application::ports::ProfilePurpose;
	use application::ports::ProfileSource;
	use application::ports::StagedProfile;
	use domain::GameBinding;
	use domain::GameInstallationPath;
	use std::fs;
	#[cfg(unix)]
	use std::os::unix::fs::PermissionsExt;
	use std::time::Duration;
	use std::time::SystemTime;
	use std::time::UNIX_EPOCH;
	use tempfile::TempDir;

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

	async fn stage(
		root: &EnvironmentRoot,
		binding: &GameBinding,
	) -> Result<(EnvironmentPlan, StagedProfile), ErrorMarker> {
		let cancellation = CancellationToken::new();
		let prepared = EnvironmentAdapter.prepare_launch(root, binding, &cancellation).await?;
		let inis = EnvironmentAdapter
			.stage_profile_inis(root, &prepared, ProfilePurpose::Export, &cancellation)
			.await?;

		let staged = StagedProfile {
			directory: inis.path().to_owned(),
			state: AdapterState::new(inis),
		};
		let plan = EnvironmentPlan {
			providers: Vec::new(),
			state: AdapterState::new(prepared),
		};
		Ok((plan, staged))
	}

	async fn discard(staged: StagedProfile) -> Result<(), ErrorMarker> {
		staged.state
			.downcast::<StagedProfileInis>()
			.ok_or_else(|| report!(ErrorMarker::io_failure()))?
			.discard()
			.await
	}

	fn modified(path: &Path) -> Result<SystemTime, ErrorMarker> {
		fs::metadata(path)
			.context(ErrorMarker::io_failure())?
			.modified()
			.context(ErrorMarker::io_failure())
	}

	#[tokio::test]
	async fn listing_covers_the_game_view_and_base_data_and_saves_are_opt_in() -> Result<(), ErrorMarker> {
		let (_temp, root, binding, _output) = fixture().await?;
		let canonical =
			fs::read(root.as_path().join("profile/Fallout.ini")).context(ErrorMarker::io_failure())?;
		fs::write(root.as_path().join("profile/loadorder.txt"), b"Ignored.esp\r\n")
			.context(ErrorMarker::io_failure())?;
		let (plan, staged) = stage(&root, &binding).await?;
		let list = EnvironmentAdapter.list_export_files_port();

		for (saves, game_data) in [(false, false), (true, false), (false, true)] {
			let listing = list
				.call((
					&plan,
					&staged,
					ExportSelection {
						include_saves: saves,
						include_game_data: game_data,
					},
					CancellationToken::new(),
				))
				.await?;

			let paths: Vec<_> = listing.files.iter().map(|file| file.path.as_str()).collect();
			for expected in [
				"Data/Textures/nested/meta.toml",
				"Data/Opaque.bsa",
				"Data/Fallout - Invalidation.bsa",
				"profile/Fallout.ini",
				"profile/modlist.txt",
			] {
				assert!(paths.contains(&expected), "missing {expected}");
			}
			assert_eq!(paths.contains(&"Data/FalloutNV.esm"), game_data);
			assert_eq!(paths.contains(&"profile/saves/example.fos"), saves);
			assert!(paths.contains(&"profile/plugins.txt"));
			assert!(!paths.contains(&"profile/loadorder.txt"));
			let ini = listing
				.files
				.iter()
				.find(|file| file.path.as_str() == "profile/Fallout.ini")
				.ok_or_else(|| report!(ErrorMarker::io_failure()))?;
			assert_eq!(
				ini.bytes,
				fs::metadata(staged.directory.join("Fallout.ini"))
					.context(ErrorMarker::io_failure())?
					.len()
			);
		}

		discard(staged).await?;
		assert_eq!(
			fs::read(root.as_path().join("profile/Fallout.ini")).context(ErrorMarker::io_failure())?,
			canonical
		);
		assert_eq!(
			fs::read_dir(root.as_path().join("temp"))
				.context(ErrorMarker::io_failure())?
				.count(),
			0
		);
		Ok(())
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

		let (_plan, staged) = stage(&root, &binding).await?;

		for name in ["Fallout.ini", "FalloutPrefs.ini"] {
			let derived =
				fs::read_to_string(staged.directory.join(name)).context(ErrorMarker::io_failure())?;
			assert!(derived.contains(warning));
		}
		discard(staged).await
	}

	#[tokio::test]
	async fn existing_outputs_and_outputs_inside_the_environment_are_rejected() -> Result<(), ErrorMarker> {
		let (_temp, root, _binding, output) = fixture().await?;
		let validate = EnvironmentAdapter.validate_export_destination_port(root.clone());

		for inside in [root.as_path().join("export"), root.as_path().join("mods/export")] {
			assert!(validate.call((inside,)).await.is_err());
		}
		validate.call((output.clone(),)).await?;
		fs::create_dir(&output).context(ErrorMarker::io_failure())?;
		assert!(validate.call((output,)).await.is_err());
		Ok(())
	}

	#[tokio::test]
	async fn export_copies_sources_and_staged_inis() -> Result<(), ErrorMarker> {
		let (_temp, root, binding, output) = fixture().await?;
		let (plan, staged) = stage(&root, &binding).await?;
		let listing = EnvironmentAdapter
			.list_export_files_port()
			.call((
				&plan,
				&staged,
				ExportSelection {
					include_saves: true,
					include_game_data: false,
				},
				CancellationToken::new(),
			))
			.await?;

		EnvironmentAdapter
			.write_export_port(root.clone())
			.call((listing.sources, listing.files, output.clone(), CancellationToken::new()))
			.await?;

		assert_eq!(
			fs::read(output.join("Data/Opaque.bsa")).context(ErrorMarker::io_failure())?,
			b"opaque"
		);
		assert_eq!(
			fs::read(output.join("profile/Fallout.ini")).context(ErrorMarker::io_failure())?,
			fs::read(staged.directory.join("Fallout.ini")).context(ErrorMarker::io_failure())?
		);
		assert!(output.join("profile/saves/example.fos").is_file());
		let ini = fs::read_to_string(output.join("profile/Fallout.ini")).context(ErrorMarker::io_failure())?;
		assert!(ini.contains("SLocalSavePath=Saves\\"));
		assert!(ini.contains("sArchiveList=Fallout - Invalidation.bsa, Original.bsa"));
		discard(staged).await
	}

	/// Also a regression test for read-only sources: a copy keeps the read-only
	/// state, and setting its time must not need write access.
	#[tokio::test]
	async fn export_times_output_plugins_and_archives_in_load_order() -> Result<(), ErrorMarker> {
		let (_temp, root, binding, output) = fixture().await?;
		let plugin = root.as_path().join("overwrite/Mod.esp");
		let archives = [
			root.as_path().join("overwrite/Opaque.bsa"),
			root.as_path().join("overwrite/Mod - Main.bsa"),
		];
		load_order_fixture(&root)?;
		for path in archives.iter().chain([&plugin]) {
			set_read_only(path, true)?;
		}
		let (plan, staged) = stage(&root, &binding).await?;
		let listing = EnvironmentAdapter
			.list_export_files_port()
			.call((
				&plan,
				&staged,
				ExportSelection {
					include_saves: false,
					include_game_data: false,
				},
				CancellationToken::new(),
			))
			.await?;

		let timed = async {
			EnvironmentAdapter
				.write_export_port(root.clone())
				.call((listing.sources, listing.files, output.clone(), CancellationToken::new()))
				.await?;

			EnvironmentAdapter
				.set_load_order_times_port()
				.call((
					&plan,
					LoadOrderTarget::Export {
						output: output.clone(),
						include_game_data: false,
					},
					CancellationToken::new(),
				))
				.await
		}
		.await;

		let data = output.join("Data");
		let checked = timed.and_then(|()| {
			assert_eq!(
				fs::read(data.join("Mod.esp")).context(ErrorMarker::io_failure())?,
				b"plugin"
			);
			assert!(fs::metadata(data.join("Mod.esp"))
				.context(ErrorMarker::io_failure())?
				.permissions()
				.readonly());
			// The invalidation archive, the archive without a plugin, the game's
			// FalloutNV.esm (not exported), and then Mod.esp with its archive.
			for (name, position) in [
				("Fallout - Invalidation.bsa", 0),
				("Opaque.bsa", 1),
				("Mod.esp", 3),
				("Mod - Main.bsa", 3),
			] {
				assert_eq!(modified(&data.join(name))?, load_order_time(position), "{name}");
			}
			assert!(!data.join("FalloutNV.esm").exists());
			for other in [
				data.join("Textures/nested/meta.toml"),
				output.join("profile/Fallout.ini"),
			] {
				assert!(modified(&other)? > load_order_time(60 * 24), "{}", other.display());
			}
			Ok(())
		});
		for path in archives.iter().chain([&plugin]) {
			set_read_only(path, false)?;
		}
		for name in ["Opaque.bsa", "Mod - Main.bsa", "Mod.esp"] {
			if data.join(name).exists() {
				set_read_only(&data.join(name), false)?;
			}
		}
		discard(staged).await?;
		checked
	}

	#[tokio::test]
	async fn export_with_game_data_copies_and_times_the_base_game_plugin() -> Result<(), ErrorMarker> {
		let (_temp, root, binding, output) = fixture().await?;
		load_order_fixture(&root)?;
		let (plan, staged) = stage(&root, &binding).await?;
		let listing = EnvironmentAdapter
			.list_export_files_port()
			.call((
				&plan,
				&staged,
				ExportSelection {
					include_saves: false,
					include_game_data: true,
				},
				CancellationToken::new(),
			))
			.await?;

		let timed = async {
			EnvironmentAdapter
				.write_export_port(root.clone())
				.call((listing.sources, listing.files, output.clone(), CancellationToken::new()))
				.await?;

			EnvironmentAdapter
				.set_load_order_times_port()
				.call((
					&plan,
					LoadOrderTarget::Export {
						output: output.clone(),
						include_game_data: true,
					},
					CancellationToken::new(),
				))
				.await
		}
		.await;
		discard(staged).await?;
		timed?;

		let data = output.join("Data");
		assert_eq!(
			fs::read(data.join("FalloutNV.esm")).context(ErrorMarker::io_failure())?,
			b"base"
		);
		for (name, position) in [
			("Fallout - Invalidation.bsa", 0),
			("Opaque.bsa", 1),
			("FalloutNV.esm", 2),
			("Mod.esp", 3),
		] {
			assert_eq!(modified(&data.join(name))?, load_order_time(position), "{name}");
		}
		Ok(())
	}

	/// Adds a plugin with a matching archive and activates it after the game's master.
	fn load_order_fixture(root: &EnvironmentRoot) -> Result<(), ErrorMarker> {
		fs::write(root.as_path().join("overwrite/Mod.esp"), b"plugin").context(ErrorMarker::io_failure())?;
		fs::write(root.as_path().join("overwrite/Mod - Main.bsa"), b"archive")
			.context(ErrorMarker::io_failure())?;
		fs::write(
			root.as_path().join("profile/plugins.txt"),
			b"FalloutNV.esm\r\nMod.esp\r\n",
		)
		.context(ErrorMarker::io_failure())
	}

	/// The time of a load-order position: 2000-01-01T00:00:00Z plus one minute each.
	fn load_order_time(position: u64) -> SystemTime {
		UNIX_EPOCH + Duration::from_secs(946_684_800 + 60 * position)
	}

	/// Sets or clears the read-only state; the test restores it so the temporary
	/// directory can be removed on Windows.
	fn set_read_only(path: &Path, read_only: bool) -> Result<(), ErrorMarker> {
		let mut permissions = fs::metadata(path).context(ErrorMarker::io_failure())?.permissions();
		#[cfg(unix)]
		permissions.set_mode(if read_only { 0o444 } else { 0o644 });
		#[cfg(not(unix))]
		permissions.set_readonly(read_only);

		fs::set_permissions(path, permissions).context(ErrorMarker::io_failure())
	}
}
