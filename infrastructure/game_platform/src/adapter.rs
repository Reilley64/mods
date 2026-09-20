#[cfg(test)]
use crate::known_folders::KnownFolderPaths;
use crate::registry::bethesda_hints;
use crate::registry::steam_roots;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub(super) enum KnownFolderSource {
	System,
	#[cfg(test)]
	Fixed(KnownFolderPaths),
}

#[derive(Debug, Clone)]
pub struct GamePlatformAdapter {
	pub(super) steam_roots: Arc<Vec<PathBuf>>,
	pub(super) bethesda_hints: Arc<Vec<PathBuf>>,
	pub(super) known_folders: KnownFolderSource,
}

impl GamePlatformAdapter {
	pub fn system() -> Self {
		Self {
			steam_roots: Arc::new(steam_roots()),
			bethesda_hints: Arc::new(bethesda_hints()),
			known_folders: KnownFolderSource::System,
		}
	}
}
