use crate::Resources;
use application::installation::InstallArchiveDependencies;
use application::installation::InstallModDependencies;
use domain::GameBinding;
use infrastructure_nexus::NexusAdapter;
use infrastructure_settings::LoadedSettings;

impl Resources {
	pub fn install_mod_dependencies(&self, loaded: &LoadedSettings) -> InstallModDependencies {
		let binding = loaded.resolved.effective_binding.clone();
		let api_key = loaded.nexus_api_key().map(str::to_owned);
		let adapter = NexusAdapter::new(self.root.clone(), move || Ok(api_key.clone()));

		InstallModDependencies {
			install_archive: self.install_archive_dependencies(binding),
			download_mod: adapter.download_mod_port(),
		}
	}

	pub fn install_archive_dependencies(&self, binding: GameBinding) -> InstallArchiveDependencies {
		InstallArchiveDependencies {
			report_progress: None,
			scan_environment_conflicts: self
				.environment
				.scan_environment_conflicts_port(self.root.clone(), binding.clone()),
			read_conflict_content: self
				.environment
				.read_conflict_content_port(self.root.clone(), binding.clone()),
			load_installation_state: self
				.environment
				.load_installation_state_port(self.root.clone(), binding.clone()),
			assess_installation: self
				.environment
				.assess_installation_port(self.root.clone(), binding.clone()),
			index_archive: self.archive.index_port(),
			read_game_version: self.game_platform.game_version_port(),
			read_xnvse_version: self.game_platform.xnvse_version_port(),
			begin_installation: self
				.environment
				.begin_installation_port(self.root.clone(), binding.clone()),
			extract_approved_files: self.archive.extract_port(),
		}
	}
}
