use crate::ports::DownloadNexusArchive;
use crate::ports::LoadNexusApiKey;
use crate::ports::ParseNexusSource;
use crate::ports::ReadNexusCache;
use crate::ports::ResolveNexusMod;
use application::ErrorMarker;
use application::installation::DownloadModFile;
use application::installation::DownloadModOutput;
use application::installation::NexusProvenance;
use rootcause::Result;
use rootcause::report;
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub(crate) struct DownloadDependencies {
	pub parse_source: ParseNexusSource,
	pub load_key: LoadNexusApiKey,
	pub read_cache: ReadNexusCache,
	pub resolve_mod: ResolveNexusMod,
	pub download: DownloadNexusArchive,
}
pub(crate) async fn download_mod(
	dependencies: DownloadDependencies,
	source: String,
	file_id: Option<u64>,
	cancellation: CancellationToken,
) -> Result<DownloadModOutput, ErrorMarker> {
	let mut request = dependencies.parse_source.call((source, file_id)).await?;
	if request.file_id.is_some()
		&& let Some(cached) = dependencies
			.read_cache
			.call((request.clone(), cancellation.clone()))
			.await?
	{
		return Ok(DownloadModOutput::Downloaded(cached));
	}

	let key = dependencies
		.load_key
		.call(())
		.await?
		.ok_or_else(|| report!(ErrorMarker::nexus_premium_required()))?;
	let metadata = dependencies
		.resolve_mod
		.call((request.clone(), key.clone(), cancellation.clone()))
		.await?;

	let selected = if let Some(file_id) = request.file_id {
		metadata.files
			.iter()
			.find(|file| file.file_id == file_id && file.available)
			.ok_or_else(|| report!(ErrorMarker::nexus_unavailable()))?
	} else {
		let mut main_files = metadata.files.iter().filter(|file| file.available && file.main);
		let first = main_files.next();
		if first.is_none() || main_files.next().is_some() {
			return Ok(DownloadModOutput::SelectionRequired(
				metadata.files
					.into_iter()
					.filter(|file| file.available)
					.map(|file| DownloadModFile {
						file_id: file.file_id,
						name: file.name,
						version: file.version,
						category: file.category,
					})
					.collect(),
			));
		}
		first.ok_or_else(|| report!(ErrorMarker::nexus_unavailable()))?
	};
	request.file_id = Some(selected.file_id);
	let provenance = NexusProvenance {
		game_domain: request.game_domain.clone(),
		mod_id: request.mod_id,
		file_id: selected.file_id,
		file_version: selected.version.clone(),
		mod_version: metadata.version,
		mod_name: metadata.name,
		file_name: selected.name.clone(),
	};

	if let Some(mut cached) = dependencies.read_cache.call((request, cancellation.clone())).await? {
		cached.suggested_name = if provenance.file_name.is_empty() {
			provenance.mod_name.clone()
		} else {
			provenance.file_name.clone()
		};
		cached.provenance = Some(provenance);
		return Ok(DownloadModOutput::Downloaded(cached));
	}

	let archive = dependencies.download.call((provenance, key, cancellation)).await?;

	Ok(DownloadModOutput::Downloaded(archive))
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "synthetic fixture setup and outcome assertions")]
mod tests {
	use super::*;
	use crate::types::NexusApiKey;
	use crate::types::NexusFile;
	use crate::types::NexusMod;
	use crate::types::NexusRequest;
	use application::ErrorCode;
	use application::installation::DownloadedMod;
	use application::ports::PortFuture;
	use domain::ArchivePath;
	use std::env::temp_dir;
	use std::sync::Arc;
	use std::sync::Mutex;
	use std::sync::atomic::AtomicU64;
	use std::sync::atomic::Ordering;

	fn file(id: u64, main: bool) -> NexusFile {
		NexusFile {
			file_id: id,
			name: format!("File {id}"),
			version: "01.20-beta".into(),
			category: if main { "MAIN" } else { "OPTIONAL" }.into(),
			available: true,
			main,
		}
	}
	fn acquired(id: u64) -> DownloadedMod {
		DownloadedMod {
			suggested_name: format!("File {id}"),
			archive: ArchivePath::new(temp_dir().join(format!("nexus-{id}"))).expect("archive"),
			provenance: Some(NexusProvenance {
				game_domain: "newvegas".into(),
				mod_id: 42,
				file_id: id,
				file_version: "01.20-beta".into(),
				mod_version: "2.0".into(),
				mod_name: "Page".into(),
				file_name: format!("File {id}"),
			}),
		}
	}
	fn dependencies(
		files: Vec<NexusFile>,
		calls: Arc<Mutex<Vec<&'static str>>>,
		cached: Option<u64>,
	) -> DownloadDependencies {
		DownloadDependencies {
			parse_source: Arc::new(|_, file_id| {
				Box::pin(async move {
					Ok(NexusRequest {
						game_domain: "newvegas".into(),
						mod_id: 42,
						file_id,
					})
				}) as PortFuture<_>
			}),
			load_key: Arc::new({
				let calls = calls.clone();
				move || {
					calls.lock().expect("calls").push("key");
					Box::pin(async { Ok(Some(NexusApiKey::new("synthetic-key".into()))) })
						as PortFuture<_>
				}
			}),
			read_cache: Arc::new({
				let calls = calls.clone();
				move |request, _| {
					calls.lock().expect("calls").push("cache");
					Box::pin(async move {
						Ok(cached.filter(|id| Some(*id) == request.file_id).map(acquired))
					}) as PortFuture<_>
				}
			}),
			resolve_mod: Arc::new({
				let calls = calls.clone();
				move |_, _, _| {
					calls.lock().expect("calls").push("resolve");
					let files = files.clone();
					Box::pin(async move {
						Ok(NexusMod {
							name: "Page".into(),
							version: "2.0".into(),
							files,
						})
					}) as PortFuture<_>
				}
			}),
			download: Arc::new(move |provenance, _, _| {
				calls.lock().expect("calls").push("download");
				Box::pin(async move {
					let mut output = acquired(provenance.file_id);
					output.provenance = Some(provenance);
					Ok(output)
				}) as PortFuture<_>
			}),
		}
	}
	#[tokio::test]
	async fn fresh_metadata_supplies_suggested_name_without_application_provider_logic() {
		let calls = Arc::new(Mutex::new(Vec::new()));
		let mut main = file(7, true);
		main.name.clear();
		let result = download_mod(
			dependencies(vec![main], calls, Some(7)),
			"source".into(),
			None,
			CancellationToken::new(),
		)
		.await
		.expect("cached selection");
		assert!(
			matches!(result, DownloadModOutput::Downloaded(value) if value.suggested_name == "Page" && value.provenance.as_ref().is_some_and(|source| source.file_name.is_empty()))
		);
	}

	#[tokio::test]
	async fn exact_explicit_cache_hit_never_loads_credentials_or_resolves() {
		let calls = Arc::new(Mutex::new(Vec::new()));
		let result = download_mod(
			dependencies(vec![], calls.clone(), Some(7)),
			"source".into(),
			Some(7),
			CancellationToken::new(),
		)
		.await
		.expect("cached");
		assert!(
			matches!(result, DownloadModOutput::Downloaded(value) if value.provenance.as_ref().is_some_and(|source| source.file_id == 7))
		);
		assert_eq!(*calls.lock().expect("calls"), ["cache"]);
	}
	#[tokio::test]
	async fn page_resolves_fresh_then_reuses_exact_bytes() {
		let calls = Arc::new(Mutex::new(Vec::new()));
		let result = download_mod(
			dependencies(vec![file(7, true), file(8, false)], calls.clone(), Some(7)),
			"source".into(),
			None,
			CancellationToken::new(),
		)
		.await
		.expect("cached");
		assert!(
			matches!(result, DownloadModOutput::Downloaded(value) if value.provenance.as_ref().is_some_and(|source| source.file_version == "01.20-beta" && source.mod_version == "2.0"))
		);
		assert_eq!(*calls.lock().expect("calls"), ["key", "resolve", "cache"]);
	}
	#[tokio::test]
	async fn zero_or_multiple_main_files_require_explicit_choice_with_available_details() {
		for files in [vec![file(8, false)], vec![file(7, true), file(8, true)]] {
			let calls = Arc::new(Mutex::new(Vec::new()));
			let result = download_mod(
				dependencies(files.clone(), calls.clone(), None),
				"source".into(),
				None,
				CancellationToken::new(),
			)
			.await
			.expect("selection");
			assert!(
				matches!(result, DownloadModOutput::SelectionRequired(values) if values == files.into_iter().map(|file| DownloadModFile { file_id: file.file_id, name: file.name, version: file.version, category: file.category }).collect::<Vec<_>>())
			);
			assert_eq!(*calls.lock().expect("calls"), ["key", "resolve"]);
		}
	}
	#[tokio::test]
	async fn repeated_page_invocations_select_changed_main_without_session_state() {
		let calls = Arc::new(Mutex::new(Vec::new()));
		let mut deps = dependencies(vec![], calls, Some(7));
		let current = Arc::new(AtomicU64::new(7));
		deps.resolve_mod = Arc::new({
			let current = current.clone();
			move |_, _, _| {
				let id = current.load(Ordering::SeqCst);
				Box::pin(async move {
					Ok(NexusMod {
						name: "Page".into(),
						version: "2".into(),
						files: vec![file(id, true)],
					})
				}) as PortFuture<_>
			}
		});
		for id in [7, 8] {
			current.store(id, Ordering::SeqCst);
			let result = download_mod(deps.clone(), "source".into(), None, CancellationToken::new())
				.await
				.expect("fresh");
			assert!(
				matches!(result, DownloadModOutput::Downloaded(value) if value.provenance.as_ref().is_some_and(|source| source.file_id == id))
			);
		}
	}
	#[tokio::test]
	async fn resolution_failure_never_uses_stale_cache_and_missing_key_is_specific() {
		let calls = Arc::new(Mutex::new(Vec::new()));
		let mut deps = dependencies(vec![], calls.clone(), Some(7));
		deps.resolve_mod = Arc::new(|_, _, _| {
			Box::pin(async { Err(report!(ErrorMarker::nexus_rate_limited())) }) as PortFuture<_>
		});
		let error = download_mod(deps.clone(), "source".into(), None, CancellationToken::new())
			.await
			.expect_err("resolution failure");
		assert!(error.iter_reports().any(|r| r
			.downcast_current_context::<ErrorMarker>()
			.is_some_and(|m| m.code() == ErrorCode::NexusRateLimited)));
		assert_eq!(*calls.lock().expect("calls"), ["key"]);
		deps.load_key = Arc::new(|| Box::pin(async { Ok(None) }) as PortFuture<_>);
		let error = download_mod(deps, "source".into(), None, CancellationToken::new())
			.await
			.expect_err("missing key");
		assert!(error.iter_reports().any(|r| r
			.downcast_current_context::<ErrorMarker>()
			.is_some_and(|m| m.code() == ErrorCode::NexusPremiumRequired)));
	}
	#[tokio::test]
	async fn explicit_optional_file_is_selected_and_unavailable_file_is_rejected() {
		let calls = Arc::new(Mutex::new(Vec::new()));
		let deps = dependencies(vec![file(7, true), file(8, false)], calls, None);
		let result = download_mod(deps.clone(), "source".into(), Some(8), CancellationToken::new())
			.await
			.expect("optional");
		assert!(
			matches!(result, DownloadModOutput::Downloaded(value) if value.provenance.as_ref().is_some_and(|source| source.file_id == 8))
		);
		assert!(download_mod(deps, "source".into(), Some(9), CancellationToken::new())
			.await
			.is_err());
	}
}
