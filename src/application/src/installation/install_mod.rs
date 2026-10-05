use crate::installation::DownloadModFile;
use crate::installation::DownloadModOutput;
use crate::installation::InstallArchiveDependencies;
use crate::installation::InstallArchiveOutput;
use crate::installation::InstallArchiveSource;
use crate::installation::ModSource;
use crate::installation::install_archive;
use crate::ports::DownloadMod;
use crate::ports::InstallationStateAccess;
use domain::FomodChoice;
use domain::ModName;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use std::fmt;
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub struct InstallModDependencies {
	pub install_archive: InstallArchiveDependencies,
	pub download_mod: DownloadMod,
}

#[derive(Debug, Clone)]
pub enum InstallModOutput {
	Archive(InstallArchiveOutput),
	SelectionRequired(Vec<DownloadModFile>),
}

#[derive(Debug, Clone, Copy)]
pub struct InstallModError;
impl fmt::Display for InstallModError {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		formatter.write_str("failed to install mod")
	}
}

#[tracing::instrument(skip_all)]
pub async fn install_mod(
	dependencies: InstallModDependencies,
	source: ModSource,
	mod_name: Option<ModName>,
	replace: bool,
	choices: Vec<FomodChoice>,
	dry_run: bool,
	cancellation: CancellationToken,
) -> Result<InstallModOutput, InstallModError> {
	let archive = match source {
		ModSource::Local(archive) => InstallArchiveSource::Local(archive),
		ModSource::Remote(source) => {
			dependencies
				.install_archive
				.load_installation_state
				.call((InstallationStateAccess::Preview, cancellation.clone()))
				.await
				.context(InstallModError)?;

			match dependencies
				.download_mod
				.call((source, cancellation.clone()))
				.await
				.context(InstallModError)?
			{
				DownloadModOutput::Downloaded(downloaded) => {
					InstallArchiveSource::Downloaded(downloaded)
				}
				DownloadModOutput::SelectionRequired(files) => {
					return Ok(InstallModOutput::SelectionRequired(files));
				}
			}
		}
	};

	let output = install_archive(
		dependencies.install_archive,
		archive,
		mod_name,
		replace,
		choices,
		dry_run,
		cancellation,
	)
	.await
	.context(InstallModError)?;

	Ok(InstallModOutput::Archive(output))
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "synthetic fixture setup and outcome assertions")]
mod tests {
	use super::*;
	use crate::ErrorCode;
	use crate::ErrorMarker;
	use crate::conflicts::EnvironmentConflictScan;
	use crate::installation::ArchiveIndex;
	use crate::installation::DownloadedMod;
	use crate::installation::IndexedInstaller;
	use crate::installation::InstallationAssessment;
	use crate::installation::InstallationState;
	use crate::installation::NexusProvenance;
	use crate::installation::RemoteModSource;
	use crate::ports::InstallationChange;
	use crate::ports::PortFuture;
	use domain::ArchiveIdentity;
	use domain::ArchivePath;
	use domain::DataRelativePath;
	use domain::GameBinding;
	use domain::GameInstallationPath;
	use domain::InstallCandidate;
	use domain::InstallCandidateOrigin;
	use domain::InstallationPhase;
	use domain::Sha256Digest;
	use rootcause::report;
	use std::collections::HashMap;
	use std::env::temp_dir;
	use std::sync::Arc;
	use std::sync::Mutex;

	fn archive() -> ArchivePath {
		ArchivePath::new(temp_dir().join("completed-archive")).expect("archive")
	}
	fn remote() -> ModSource {
		ModSource::Remote(RemoteModSource {
			url: "https://provider.example/mod/42".into(),
			file_id: Some(7),
		})
	}
	fn dependencies(events: Arc<Mutex<Vec<&'static str>>>) -> InstallModDependencies {
		InstallModDependencies {
			download_mod: Arc::new({
				let events = events.clone();
				move |source, _| {
					assert_eq!(source.file_id, Some(7));
					events.lock().expect("events").push("download");
					Box::pin(async {
						Ok(DownloadModOutput::SelectionRequired(vec![DownloadModFile {
							file_id: 7,
							name: "File".into(),
							version: "1".into(),
							category: "Main".into(),
						}]))
					}) as PortFuture<_>
				}
			}),
			install_archive: InstallArchiveDependencies {
				report_progress: None,
				load_installation_state: Arc::new({
					let events = events.clone();
					move |access, _| {
						events.lock().expect("events").push(
							if access == InstallationStateAccess::Preview {
								"preview_state"
							} else {
								"mutation_state"
							},
						);
						Box::pin(async {
							Ok(InstallationState {
								game_binding: GameBinding::new(
									GameInstallationPath::new(
										temp_dir().join("game"),
									)
									.expect("game"),
								),
								installed_mods: Vec::new(),
								unlisted_mod_names: Vec::new(),
								current_winners: HashMap::new(),
								file_dependencies: HashMap::new(),
							})
						}) as PortFuture<_>
					}
				}),
				index_archive: Arc::new(move |received, _, _| {
					assert_eq!(received, archive());
					events.lock().expect("events").push("index");
					Box::pin(async { Err(report!(ErrorMarker::unsafe_archive())) }) as PortFuture<_>
				}),
				scan_environment_conflicts: Arc::new(|_| {
					Box::pin(async { Err(report!(ErrorMarker::io_failure())) }) as PortFuture<_>
				}),
				read_conflict_content: Arc::new(|_, _| {
					Box::pin(async { Err(report!(ErrorMarker::io_failure())) }) as PortFuture<_>
				}),
				assess_installation: Arc::new(|_, _| {
					Box::pin(async { Err(report!(ErrorMarker::io_failure())) }) as PortFuture<_>
				}),
				read_game_version: Arc::new(|_, _| {
					Box::pin(async { Err(report!(ErrorMarker::io_failure())) }) as PortFuture<_>
				}),
				read_xnvse_version: Arc::new(|_, _| {
					Box::pin(async { Err(report!(ErrorMarker::io_failure())) }) as PortFuture<_>
				}),
				begin_installation: Arc::new(|_, _| {
					Box::pin(async { Err(report!(ErrorMarker::io_failure())) }) as PortFuture<_>
				}),
				extract_approved_files: Arc::new(|_, _, _, _, _, _| {
					Box::pin(async { Err(report!(ErrorMarker::io_failure())) }) as PortFuture<_>
				}),
			},
		}
	}
	#[tokio::test]
	async fn downloaded_archive_without_provider_metadata_installs_without_nexus_publication() {
		let events = Arc::new(Mutex::new(Vec::new()));
		let mut deps = dependencies(events.clone());
		deps.download_mod = Arc::new(|_, _| {
			Box::pin(async {
				Ok(DownloadModOutput::Downloaded(DownloadedMod {
					archive: archive(),
					suggested_name: "Independent download".into(),
					provenance: None,
				}))
			}) as PortFuture<_>
		});
		deps.install_archive.index_archive = Arc::new(|_, _, _| {
			Box::pin(async {
				Ok(ArchiveIndex {
					identity: ArchiveIdentity::DataArchive {
						archive_sha256: Sha256Digest::new("a".repeat(64)).expect("hash"),
						package_root: String::new(),
					},
					installer: IndexedInstaller::Plain {
						candidates: vec![InstallCandidate {
							candidate_id: 1,
							origin: InstallCandidateOrigin::Required,
							phase: InstallationPhase::Required,
							declared_priority: 0,
							descriptor_order: 0,
							source_member: "Data/file.txt".into(),
							destination: DataRelativePath::new("file.txt".into())
								.expect("destination"),
						}],
						warnings: Vec::new(),
					},
				})
			}) as PortFuture<_>
		});
		deps.install_archive.assess_installation = Arc::new(|_, _| {
			Box::pin(async { Ok(InstallationAssessment { overlaps: Vec::new() }) }) as PortFuture<_>
		});
		deps.install_archive.scan_environment_conflicts = Arc::new(|_| {
			Box::pin(async {
				Ok(EnvironmentConflictScan {
					providers: Vec::new(),
					problems: Vec::new(),
				})
			}) as PortFuture<_>
		});
		deps.install_archive.begin_installation = Arc::new({
			let events = events.clone();
			move |approved, _| {
				assert!(approved.nexus.is_none());
				assert_eq!(approved.plan.mod_name.as_str(), "Independent download");
				events.lock().expect("events").push("begin_without_nexus");
				let events = events.clone();
				Box::pin(async move {
					Ok(InstallationChange {
						begin_file: Arc::new(|_, _| {
							Box::pin(async { Err(report!(ErrorMarker::io_failure())) })
								as PortFuture<_>
						}),
						finish: Arc::new(move |_| {
							events.lock().expect("events").push("published");
							Box::pin(async { Ok(()) }) as PortFuture<_>
						}),
					})
				}) as PortFuture<_>
			}
		});
		deps.install_archive.extract_approved_files =
			Arc::new(|_, _, _, _, _, _| Box::pin(async { Ok(()) }) as PortFuture<_>);

		let output = install_mod(deps, remote(), None, false, vec![], false, CancellationToken::new())
			.await
			.expect("provider-neutral installation");

		assert!(matches!(
			output,
			InstallModOutput::Archive(InstallArchiveOutput::Installed(_))
		));
		assert_eq!(
			*events.lock().expect("events"),
			["preview_state", "mutation_state", "begin_without_nexus", "published"]
		);
	}

	#[tokio::test]
	async fn local_input_never_crosses_download_seam() {
		let events = Arc::new(Mutex::new(Vec::new()));
		let error = install_mod(
			dependencies(events.clone()),
			ModSource::Local(archive()),
			None,
			false,
			vec![],
			false,
			CancellationToken::new(),
		)
		.await
		.expect_err("index failure");
		assert_eq!(*events.lock().expect("events"), ["mutation_state", "index"]);
		assert_eq!(error.current_context().to_string(), "failed to install mod");
		assert!(error.iter_reports().any(|report| report
			.downcast_current_context::<ErrorMarker>()
			.is_some_and(|marker| marker.code() == ErrorCode::UnsafeArchive)));
	}
	#[tokio::test]
	async fn remote_selection_preflights_without_entering_installer() {
		let events = Arc::new(Mutex::new(Vec::new()));
		let result = install_mod(
			dependencies(events.clone()),
			remote(),
			None,
			false,
			vec![],
			true,
			CancellationToken::new(),
		)
		.await
		.expect("selection");
		assert!(matches!(result, InstallModOutput::SelectionRequired(files) if files[0].file_id == 7));
		assert_eq!(*events.lock().expect("events"), ["preview_state", "download"]);
	}
	#[tokio::test]
	async fn complete_remote_archive_enters_existing_installer() {
		let events = Arc::new(Mutex::new(Vec::new()));
		let mut deps = dependencies(events.clone());
		deps.download_mod = Arc::new({
			let events = events.clone();
			move |_, _| {
				events.lock().expect("events").push("download");
				Box::pin(async {
					Ok(DownloadModOutput::Downloaded(DownloadedMod {
						suggested_name: "File".into(),
						archive: archive(),
						provenance: Some(NexusProvenance {
							game_domain: "newvegas".into(),
							mod_id: 42,
							file_id: 7,
							file_version: "1".into(),
							mod_version: "2".into(),
							mod_name: "Page".into(),
							file_name: "File".into(),
						}),
					}))
				}) as PortFuture<_>
			}
		});
		assert!(
			install_mod(deps, remote(), None, false, vec![], true, CancellationToken::new())
				.await
				.is_err()
		);
		assert_eq!(
			*events.lock().expect("events"),
			["preview_state", "download", "preview_state", "index"]
		);
	}
	#[tokio::test]
	async fn failed_preflight_stops_before_download() {
		let events = Arc::new(Mutex::new(Vec::new()));
		let mut deps = dependencies(events.clone());
		deps.install_archive.load_installation_state = Arc::new(|_, _| {
			Box::pin(async { Err(report!(ErrorMarker::game_install_invalid())) }) as PortFuture<_>
		});
		assert!(
			install_mod(deps, remote(), None, false, vec![], true, CancellationToken::new())
				.await
				.is_err()
		);
		assert!(events.lock().expect("events").is_empty());
	}
}
