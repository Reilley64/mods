#![feature(fn_traits)]

mod cache;
mod download_mod;
mod http;
mod ports;
mod source;
mod types;

use crate::download_mod::DownloadDependencies;
use crate::download_mod::download_mod;
use crate::types::NexusApiKey;
use application::ErrorMarker;
use application::ports::DownloadMod;
use application::ports::PortFuture;
use domain::EnvironmentRoot;
use rootcause::Result;
use std::sync::Arc;

#[derive(Clone)]
pub struct NexusAdapter {
	root: EnvironmentRoot,
	load_key: Arc<dyn Fn() -> Result<Option<String>, ErrorMarker> + Send + Sync>,
}
impl NexusAdapter {
	pub fn new(
		root: EnvironmentRoot,
		load_key: impl Fn() -> Result<Option<String>, ErrorMarker> + Send + Sync + 'static,
	) -> Self {
		Self {
			root,
			load_key: Arc::new(load_key),
		}
	}

	pub fn download_mod_port(&self) -> DownloadMod {
		let cache_root = self.root.clone();
		let download_root = self.root.clone();
		let load_key = self.load_key.clone();
		let dependencies = DownloadDependencies {
			parse_source: Arc::new(|source, file| {
				Box::pin(async move { source::parse(&source, file) }) as PortFuture<_>
			}),
			load_key: Arc::new(move || {
				let result = load_key().map(|key| key.map(NexusApiKey::new));
				Box::pin(async move { result }) as PortFuture<_>
			}),
			read_cache: Arc::new(move |request, cancellation| {
				let root = cache_root.clone();
				Box::pin(async move { cache::read(&root, &request, &cancellation).await })
					as PortFuture<_>
			}),
			resolve_mod: Arc::new(|request, key, cancellation| {
				Box::pin(async move { http::resolve(request, key, cancellation).await })
					as PortFuture<_>
			}),
			download: Arc::new(move |provenance, key, cancellation| {
				let root = download_root.clone();
				Box::pin(async move { http::download(root, provenance, key, cancellation).await })
					as PortFuture<_>
			}),
		};

		Arc::new(move |source, cancellation| {
			let dependencies = dependencies.clone();
			Box::pin(
				async move { download_mod(dependencies, source.url, source.file_id, cancellation).await },
			) as PortFuture<_>
		})
	}
}

#[cfg(test)]
#[expect(
	clippy::expect_used,
	reason = "synthetic adapter fixture setup and outcome assertions"
)]
mod tests {
	use super::NexusAdapter;
	use crate::cache::CompletedMetadata;
	use application::installation::DownloadModOutput;
	use application::installation::NexusProvenance;
	use application::installation::RemoteModSource;
	use domain::EnvironmentRoot;
	use std::fs;
	use std::sync::Arc;
	use std::sync::atomic::AtomicBool;
	use std::sync::atomic::Ordering;
	use tempfile::TempDir;
	use tokio_util::sync::CancellationToken;

	#[tokio::test]
	async fn download_port_returns_exact_completed_archive_without_loading_credentials() {
		let temp = TempDir::new().expect("root");
		let root = EnvironmentRoot::new(temp.path().canonicalize().expect("canonical root")).expect("root");
		let entry = root.as_path().join("cache/downloads/newvegas-42-7");
		fs::create_dir_all(&entry).expect("cache");
		fs::write(entry.join("archive"), b"completed").expect("archive");
		let provenance = NexusProvenance {
			game_domain: "newvegas".into(),
			mod_id: 42,
			file_id: 7,
			file_version: "01-beta".into(),
			mod_version: "2".into(),
			mod_name: "Page".into(),
			file_name: "File".into(),
		};
		fs::write(
			entry.join("provenance.toml"),
			toml::to_string(&CompletedMetadata::from_provenance(&provenance, 9)).expect("metadata"),
		)
		.expect("write");
		let credentials_read = Arc::new(AtomicBool::new(false));
		let adapter = NexusAdapter::new(root, {
			let read = credentials_read.clone();
			move || {
				read.store(true, Ordering::SeqCst);
				Ok(None)
			}
		});
		let port = adapter.download_mod_port();

		let output = port
			.call((
				RemoteModSource {
					url: "https://www.nexusmods.com/newvegas/mods/42?file_id=7".into(),
					file_id: None,
				},
				CancellationToken::new(),
			))
			.await
			.expect("cached download");

		let downloaded = if let DownloadModOutput::Downloaded(downloaded) = output {
			Some(downloaded)
		} else {
			None
		}
		.expect("explicit cached file does not require selection");
		assert_eq!(downloaded.archive.as_path(), entry.join("archive"));
		assert_eq!(downloaded.suggested_name, "File");
		assert_eq!(downloaded.provenance, Some(provenance));
		assert!(!credentials_read.load(Ordering::SeqCst));
		assert!(port
			.call((
				RemoteModSource {
					url: "https://example.test/unsupported".into(),
					file_id: None
				},
				CancellationToken::new()
			))
			.await
			.is_err());
		assert!(!credentials_read.load(Ordering::SeqCst));
	}
}
