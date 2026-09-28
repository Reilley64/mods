mod cache;
mod http;
mod source;

use application::nexus::AcquireNexusDependencies;
use application::ports::LoadNexusApiKey;
use application::ports::PortFuture;
use domain::EnvironmentRoot;
use std::sync::Arc;

#[derive(Clone)]
pub struct NexusAdapter {
	root: EnvironmentRoot,
}
impl NexusAdapter {
	pub fn new(root: EnvironmentRoot) -> Self {
		Self { root }
	}
	pub fn acquisition_dependencies(&self, load_key: LoadNexusApiKey) -> AcquireNexusDependencies {
		let cache_root = self.root.clone();
		let download_root = self.root.clone();
		AcquireNexusDependencies {
			parse_source: Arc::new(|source, file| {
				Box::pin(async move { source::parse(&source, file) }) as PortFuture<_>
			}),
			load_key,
			read_cache: Arc::new(move |request, cancellation| {
				let root = cache_root.clone();
				Box::pin(async move { cache::read(&root, &request, &cancellation) }) as PortFuture<_>
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
		}
	}
}
