#[cfg(test)]
use crate::known_folders::KnownFolderPaths;
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
			steam_roots: Arc::new(crate::registry::steam_roots()),
			bethesda_hints: Arc::new(crate::registry::bethesda_hints()),
			known_folders: KnownFolderSource::System,
		}
	}
}

#[cfg(test)]
pub(crate) mod test_support {
	use super::*;

	pub(crate) fn adapter(
		steam_roots: Vec<PathBuf>,
		bethesda_hints: Vec<PathBuf>,
		documents: PathBuf,
		local_app_data: PathBuf,
	) -> GamePlatformAdapter {
		GamePlatformAdapter {
			steam_roots: Arc::new(steam_roots),
			bethesda_hints: Arc::new(bethesda_hints),
			known_folders: KnownFolderSource::Fixed(KnownFolderPaths {
				documents,
				local_app_data,
			}),
		}
	}
}
