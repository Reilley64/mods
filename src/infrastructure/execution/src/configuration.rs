use crate::ExecutionError;
use domain::ModName;
use domain::ProviderIdentity;
use domain::ProviderReference;
use domain::case_fold_key;
use rootcause::Result;
use rootcause::report;
use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;

/// A validated provider root supplied by execution composition.
#[derive(Debug, Clone)]
pub struct ProviderRoot {
	pub identity: ProviderIdentity,
	pub root: PathBuf,
	pub enabled: bool,
}

/// A named Profile State file or caller-owned save-directory mapping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathMapping {
	pub source: PathBuf,
	pub destination: PathBuf,
}

/// Validated mapping decisions. Winner computation remains owned by mods, not usvfs.
#[derive(Debug)]
pub struct ViewConfiguration {
	pub(crate) data_target: PathMapping,
	pub(crate) files: Vec<PathMapping>,
	pub(crate) directories: Vec<PathMapping>,
	pub(crate) saves: PathMapping,
}

pub(crate) trait ConfigureView {
	fn clear_bypasses(&mut self) -> Result<(), ExecutionError>;
	fn create_target(&mut self, mapping: &PathMapping, recursive: bool) -> Result<(), ExecutionError>;
	fn link_file(&mut self, mapping: &PathMapping) -> Result<(), ExecutionError>;
	fn link_directory(&mut self, mapping: &PathMapping) -> Result<(), ExecutionError>;
}

impl ViewConfiguration {
	/// Translates already-resolved winners without changing their Mod Priority.
	///
	/// `winners` must come from mods' effective namespace computation. Omitted files
	/// do not hide physical Steam entries. `profile_files` and `saves` contain paths
	/// chosen by #31; this adapter does not generate game configuration.
	///
	/// # Errors
	///
	/// Rejects missing/disabled output targets, inconsistent provider references,
	/// duplicate winners, and non-absolute or NUL-containing mapping paths.
	pub fn new(
		data_directory: PathBuf,
		providers: Vec<ProviderRoot>,
		winners: Vec<ProviderReference>,
		output_target: Option<ModName>,
		profile_files: Vec<PathMapping>,
		saves: PathMapping,
	) -> Result<Self, ExecutionError> {
		let mut identities = HashSet::new();
		let mut names = HashSet::new();
		for provider in &providers {
			if let ProviderIdentity::DataMod { mod_name, .. } = &provider.identity
				&& !names.insert(mod_name.clone())
			{
				return Err(report!(ExecutionError));
			}
			if !identities.insert(provider.identity.clone()) || !valid_path(&provider.root) {
				return Err(report!(ExecutionError));
			}
		}

		let target = providers.iter().find(|provider| {
			if !provider.enabled {
				return false;
			}
			match (&provider.identity, &output_target) {
				(ProviderIdentity::Overwrite, None) => true,
				(ProviderIdentity::DataMod { mod_name, .. }, Some(selected)) => mod_name == selected,
				_ => false,
			}
		});
		let Some(target) = target else {
			return Err(report!(ExecutionError));
		};
		let data_target = PathMapping {
			source: target.root.clone(),
			destination: data_directory.clone(),
		};

		let mut keys = HashSet::new();
		let mut directory_keys = HashSet::new();
		let mut directories = Vec::new();
		let mut files = Vec::with_capacity(winners.len() + profile_files.len());
		for winner in winners {
			let identity = winner.identity();
			let Some(provider) = providers
				.iter()
				.find(|provider| provider.identity == identity && provider.enabled)
			else {
				return Err(report!(ExecutionError));
			};
			if !keys.insert(winner.original_path().comparison_key().to_owned()) {
				return Err(report!(ExecutionError));
			}
			let relative = winner.original_path().as_str();
			let mut parents: Vec<_> = Path::new(relative)
				.ancestors()
				.skip(1)
				.filter(|path| !path.as_os_str().is_empty())
				.collect();
			parents.reverse();
			for parent in parents {
				if directory_keys.insert(case_fold_key(&parent.to_string_lossy())) {
					directories.push(PathMapping {
						source: provider.root.join(parent),
						destination: data_directory.join(parent),
					});
				}
			}
			files.push(PathMapping {
				source: provider.root.join(relative),
				destination: data_directory.join(relative),
			});
		}
		files.extend(profile_files);
		for mapping in files.iter().chain([&data_target, &saves]) {
			if !valid_path(&mapping.source) || !valid_path(&mapping.destination) {
				return Err(report!(ExecutionError));
			}
		}

		Ok(Self {
			data_target,
			files,
			directories,
			saves,
		})
	}

	pub(crate) fn apply(&self, native: &mut impl ConfigureView) -> Result<(), ExecutionError> {
		native.clear_bypasses()?;
		// Nonrecursive: selecting a low-priority target must not remap its files last.
		native.create_target(&self.data_target, false)?;
		for mapping in &self.directories {
			native.link_directory(mapping)?;
		}
		for mapping in &self.files {
			native.link_file(mapping)?;
		}
		native.create_target(&self.saves, true)?;
		Ok(())
	}
}

fn valid_path(path: &Path) -> bool {
	path.is_absolute() && !path.as_os_str().as_encoded_bytes().contains(&0)
}

#[cfg(test)]
mod tests {
	use super::*;
	use domain::DataRelativePath;
	use domain::ModPriority;
	use domain::ParticipationReason;
	use std::error::Error;
	use std::result::Result as StdResult;

	#[derive(Debug, PartialEq, Eq)]
	enum Call {
		Clear,
		Target(PathMapping, bool),
		File(PathMapping),
		Directory(PathMapping),
	}
	#[derive(Default)]
	struct Recorder {
		calls: Vec<Call>,
		fail_clear: bool,
	}
	impl ConfigureView for Recorder {
		fn clear_bypasses(&mut self) -> Result<(), ExecutionError> {
			self.calls.push(Call::Clear);
			if self.fail_clear {
				return Err(report!(ExecutionError));
			}
			Ok(())
		}
		fn create_target(&mut self, mapping: &PathMapping, recursive: bool) -> Result<(), ExecutionError> {
			self.calls.push(Call::Target(mapping.clone(), recursive));
			Ok(())
		}
		fn link_file(&mut self, mapping: &PathMapping) -> Result<(), ExecutionError> {
			self.calls.push(Call::File(mapping.clone()));
			Ok(())
		}
		fn link_directory(&mut self, mapping: &PathMapping) -> Result<(), ExecutionError> {
			self.calls.push(Call::Directory(mapping.clone()));
			Ok(())
		}
	}

	fn root() -> PathBuf {
		if cfg!(windows) {
			PathBuf::from("C:/mods-fixture")
		} else {
			PathBuf::from("/mods-fixture")
		}
	}
	fn mapping(source: &str, destination: &str) -> PathMapping {
		PathMapping {
			source: root().join(source),
			destination: root().join(destination),
		}
	}
	fn fixture(selected: Option<ModName>, enabled: bool) -> StdResult<ViewConfiguration, Box<dyn Error>> {
		let low = ModName::new("Low".to_owned()).map_err(|_| "name")?;
		let high = ModName::new("High".to_owned()).map_err(|_| "name")?;
		Ok(ViewConfiguration::new(
			root().join("Data"),
			vec![
				ProviderRoot {
					identity: ProviderIdentity::DataMod {
						mod_name: low,
						priority: ModPriority::new(1),
					},
					root: root().join("low"),
					enabled,
				},
				ProviderRoot {
					identity: ProviderIdentity::DataMod {
						mod_name: high.clone(),
						priority: ModPriority::new(2),
					},
					root: root().join("high"),
					enabled: true,
				},
				ProviderRoot {
					identity: ProviderIdentity::Overwrite,
					root: root().join("overwrite"),
					enabled: true,
				},
			],
			vec![ProviderReference::DataMod {
				mod_name: high,
				priority: ModPriority::new(2),
				original_path: DataRelativePath::new("textures/file.mohidden".to_owned())
					.map_err(|_| "path")?,
				participation_reason: ParticipationReason::EnabledMod,
			}],
			selected,
			vec![mapping("profile/game.ini", "user/game.ini")],
			mapping("profile/saves", "Data/__mods_saves"),
		)
		.map_err(|_| "invalid configuration")?)
	}

	#[test]
	fn output_selection_does_not_change_winners_or_skip_mohidden() -> StdResult<(), Box<dyn Error>> {
		let selected = fixture(Some(ModName::new("Low".to_owned()).map_err(|_| "name")?), true)?;
		let mut recorder = Recorder::default();
		selected.apply(&mut recorder).map_err(|_| "configuration failed")?;
		assert_eq!(
			recorder.calls,
			vec![
				Call::Clear,
				Call::Target(mapping("low", "Data"), false),
				Call::Directory(mapping("high/textures", "Data/textures")),
				Call::File(mapping("high/textures/file.mohidden", "Data/textures/file.mohidden")),
				Call::File(mapping("profile/game.ini", "user/game.ini")),
				Call::Target(mapping("profile/saves", "Data/__mods_saves"), true)
			]
		);
		let default = fixture(None, true)?;
		assert_eq!(default.data_target.source, root().join("overwrite"));
		assert_eq!(default.files, selected.files);
		Ok(())
	}

	#[test]
	fn disabled_or_missing_output_target_is_rejected() -> StdResult<(), Box<dyn Error>> {
		assert!(fixture(Some(ModName::new("Low".to_owned()).map_err(|_| "name")?), false).is_err());
		assert!(fixture(Some(ModName::new("Missing".to_owned()).map_err(|_| "name")?), true).is_err());
		Ok(())
	}

	#[test]
	fn configuration_stops_on_first_native_error() -> StdResult<(), Box<dyn Error>> {
		let configuration = fixture(None, true)?;
		let mut recorder = Recorder {
			fail_clear: true,
			..Default::default()
		};
		assert!(configuration.apply(&mut recorder).is_err());
		assert_eq!(recorder.calls, vec![Call::Clear]);
		Ok(())
	}
}
