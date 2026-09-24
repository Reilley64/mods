use crate::Resources;
use application::installation::InstallArchiveDependencies;

impl Resources {
	pub fn install_archive_dependencies(&self) -> InstallArchiveDependencies {
		InstallArchiveDependencies {
			report_progress: None,
			scan_environment_conflicts: self.environment.scan_environment_conflicts_port(self.root.clone()),
			read_conflict_content: self.environment.read_conflict_content_port(self.root.clone()),
			load_installation_state: self.environment.load_installation_state_port(self.root.clone()),
			assess_installation: self.environment.assess_installation_port(self.root.clone()),
			index_archive: self.archive.index_port(),
			read_game_version: self.game_platform.game_version_port(),
			read_xnvse_version: self.game_platform.xnvse_version_port(),
			begin_installation: self.environment.begin_installation_port(self.root.clone()),
			extract_approved_files: self.archive.extract_port(),
		}
	}
}
