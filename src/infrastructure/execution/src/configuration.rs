use crate::ExecutionError;
use domain::ModName;
use domain::ProviderIdentity;
use domain::ProviderReference;
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

/// Validated mapping decisions. Analytical winner computation remains owned by mods.
#[derive(Debug)]
pub struct ViewConfiguration {
	data_overlays: Vec<(PathMapping, bool)>,
	pub(crate) files: Vec<PathMapping>,
	pub(crate) directories: Vec<PathMapping>,
	pub(crate) saves: PathMapping,
}

pub(crate) trait ConfigureView {
	fn clear_bypasses(&mut self) -> Result<(), ExecutionError>;
	fn create_target(&mut self, mapping: &PathMapping, recursive: bool) -> Result<(), ExecutionError>;
	fn link_file(&mut self, mapping: &PathMapping) -> Result<(), ExecutionError>;
	fn link_directory(&mut self, mapping: &PathMapping, recursive: bool) -> Result<(), ExecutionError>;
}

impl ViewConfiguration {
	/// Maps enabled Data roots in priority order, independently of the Output Target.
	///
	/// `winners` remains validated but does not restrict runtime visibility. Root
	/// metadata and Tombstones retain their analytical rules outside this adapter.
	/// Physical Steam entries remain the fallback. Profile mappings and saves are
	/// caller-owned; this adapter does not generate game configuration.
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
		profile_directories: Vec<PathMapping>,
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
		let target_identity = target.identity.clone();
		let mut enabled_providers: Vec<_> = providers
			.iter()
			.filter(|provider| provider.enabled && provider.identity != ProviderIdentity::SteamData)
			.collect();
		enabled_providers.sort_by_key(|provider| provider.identity.rank());
		let data_overlays: Vec<_> = enabled_providers
			.into_iter()
			.map(|provider| {
				(
					PathMapping {
						source: provider.root.clone(),
						destination: data_directory.clone(),
					},
					provider.identity == target_identity,
				)
			})
			.collect();

		let mut keys = HashSet::new();
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
			if !valid_path(&provider.root.join(relative)) || !valid_path(&data_directory.join(relative)) {
				return Err(report!(ExecutionError));
			}
		}
		for mapping in profile_files
			.iter()
			.chain(&profile_directories)
			.chain(data_overlays.iter().map(|(mapping, _)| mapping))
			.chain([&saves])
		{
			if !valid_path(&mapping.source) || !valid_path(&mapping.destination) {
				return Err(report!(ExecutionError));
			}
		}

		Ok(Self {
			data_overlays,
			files: profile_files,
			directories: profile_directories,
			saves,
		})
	}

	pub(crate) fn apply(&self, native: &mut impl ConfigureView) -> Result<(), ExecutionError> {
		native.clear_bypasses()?;
		for (mapping, is_target) in &self.data_overlays {
			if *is_target {
				native.create_target(mapping, true)?;
			} else {
				native.link_directory(mapping, true)?;
			}
		}
		for mapping in &self.directories {
			native.link_directory(mapping, false)?;
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
		Directory(PathMapping, bool),
	}
	#[derive(Default)]
	struct Recorder {
		calls: Vec<Call>,
		fail_clear: bool,
		fail_directory: bool,
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
		fn link_directory(&mut self, mapping: &PathMapping, recursive: bool) -> Result<(), ExecutionError> {
			self.calls.push(Call::Directory(mapping.clone(), recursive));
			if self.fail_directory {
				return Err(report!(ExecutionError));
			}
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
	struct Fixture {
		providers: Vec<ProviderRoot>,
		winners: Vec<ProviderReference>,
		data_directory: PathBuf,
		profile_directories: Vec<PathMapping>,
		profile_files: Vec<PathMapping>,
		saves: PathMapping,
	}
	impl Fixture {
		fn new() -> StdResult<Self, Box<dyn Error>> {
			let low = ModName::new("Low".to_owned()).map_err(|_| "name")?;
			let high = ModName::new("High".to_owned()).map_err(|_| "name")?;
			let disabled = ModName::new("Disabled".to_owned()).map_err(|_| "name")?;
			Ok(Self {
				providers: vec![
					ProviderRoot {
						identity: ProviderIdentity::Overwrite,
						root: root().join("overwrite"),
						enabled: true,
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
						identity: ProviderIdentity::SteamData,
						root: root().join("Data"),
						enabled: true,
					},
					ProviderRoot {
						identity: ProviderIdentity::DataMod {
							mod_name: disabled,
							priority: ModPriority::new(3),
						},
						root: root().join("disabled"),
						enabled: false,
					},
					ProviderRoot {
						identity: ProviderIdentity::DataMod {
							mod_name: low,
							priority: ModPriority::new(1),
						},
						root: root().join("low"),
						enabled: true,
					},
				],
				winners: vec![ProviderReference::DataMod {
					mod_name: high,
					priority: ModPriority::new(2),
					original_path: DataRelativePath::new("textures/file.mohidden".to_owned())
						.map_err(|_| "path")?,
					participation_reason: ParticipationReason::EnabledMod,
				}],
				data_directory: root().join("Data"),
				profile_directories: vec![
					mapping("profile/local", "user/local"),
					mapping("profile/documents", "user/documents"),
				],
				profile_files: vec![
					mapping("profile/game.ini", "user/game.ini"),
					mapping("profile/plugins.txt", "user/plugins.txt"),
					mapping("profile/invalidation.bsa", "Data/invalidation.bsa"),
				],
				saves: mapping("profile/saves", "Data/__mods_saves"),
			})
		}
		fn configure(self, selected: Option<ModName>) -> Result<ViewConfiguration, ExecutionError> {
			ViewConfiguration::new(
				self.data_directory,
				self.providers,
				self.winners,
				selected,
				self.profile_directories,
				self.profile_files,
				self.saves,
			)
		}
	}

	#[test]
	fn ordered_overlays_keep_target_at_ordinary_rank_and_profile_calls_unchanged() -> StdResult<(), Box<dyn Error>>
	{
		for selected in [None, Some("Low"), Some("High")] {
			let target = selected.unwrap_or("overwrite").to_lowercase();
			let selection = selected
				.map(|name| ModName::new(name.to_owned()))
				.transpose()
				.map_err(|_| "name")?;
			let configuration = Fixture::new()?.configure(selection).map_err(|_| "configuration")?;
			let mut recorder = Recorder::default();
			configuration.apply(&mut recorder).map_err(|_| "apply")?;
			let mut expected = vec![Call::Clear];
			for source in ["low", "high", "overwrite"] {
				let mapping = mapping(source, "Data");
				if source == target {
					expected.push(Call::Target(mapping, true));
				} else {
					expected.push(Call::Directory(mapping, true));
				}
			}
			expected.extend([
				Call::Directory(mapping("profile/local", "user/local"), false),
				Call::Directory(mapping("profile/documents", "user/documents"), false),
				Call::File(mapping("profile/game.ini", "user/game.ini")),
				Call::File(mapping("profile/plugins.txt", "user/plugins.txt")),
				Call::File(mapping("profile/invalidation.bsa", "Data/invalidation.bsa")),
				Call::Target(mapping("profile/saves", "Data/__mods_saves"), true),
			]);
			assert_eq!(recorder.calls, expected);
		}
		Ok(())
	}

	#[test]
	fn empty_winners_still_map_enabled_roots() -> StdResult<(), Box<dyn Error>> {
		let mut fixture = Fixture::new()?;
		fixture.winners.clear();
		let configuration = fixture.configure(None).map_err(|_| "configuration")?;
		let mut recorder = Recorder::default();
		configuration.apply(&mut recorder).map_err(|_| "apply")?;
		assert_eq!(
			&recorder.calls[..4],
			&[
				Call::Clear,
				Call::Directory(mapping("low", "Data"), true),
				Call::Directory(mapping("high", "Data"), true),
				Call::Target(mapping("overwrite", "Data"), true),
			]
		);
		Ok(())
	}

	#[test]
	fn disabled_or_missing_output_target_is_rejected() -> StdResult<(), Box<dyn Error>> {
		for name in ["Disabled", "Missing"] {
			let selected = ModName::new(name.to_owned()).map_err(|_| "name")?;
			assert!(Fixture::new()?.configure(Some(selected)).is_err());
		}
		let mut fixture = Fixture::new()?;
		fixture.providers[0].enabled = false;
		assert!(fixture.configure(None).is_err());
		Ok(())
	}

	#[test]
	fn configuration_stops_on_first_native_error() -> StdResult<(), Box<dyn Error>> {
		let configuration = Fixture::new()?.configure(None).map_err(|_| "configuration")?;
		let mut recorder = Recorder {
			fail_clear: true,
			..Default::default()
		};
		assert!(configuration.apply(&mut recorder).is_err());
		assert_eq!(recorder.calls, vec![Call::Clear]);
		let mut recorder = Recorder {
			fail_directory: true,
			..Default::default()
		};
		assert!(configuration.apply(&mut recorder).is_err());
		assert_eq!(
			recorder.calls,
			vec![Call::Clear, Call::Directory(mapping("low", "Data"), true)]
		);
		Ok(())
	}

	#[test]
	fn invalid_and_duplicate_winner_references_are_rejected() -> StdResult<(), Box<dyn Error>> {
		let mut fixture = Fixture::new()?;
		fixture.winners.push(fixture.winners[0].clone());
		assert!(fixture.configure(None).is_err());
		let mut fixture = Fixture::new()?;
		fixture.providers[1].enabled = false;
		assert!(fixture.configure(None).is_err());
		let mut fixture = Fixture::new()?;
		fixture.providers.remove(1);
		assert!(fixture.configure(None).is_err());
		let mut fixture = Fixture::new()?;
		if let ProviderReference::DataMod { priority, .. } = &mut fixture.winners[0] {
			*priority = ModPriority::new(99);
		}
		assert!(fixture.configure(None).is_err());
		Ok(())
	}

	#[test]
	fn duplicate_provider_identities_and_names_are_rejected() -> StdResult<(), Box<dyn Error>> {
		let mut fixture = Fixture::new()?;
		fixture.providers.push(fixture.providers[0].clone());
		assert!(fixture.configure(None).is_err());
		let mut fixture = Fixture::new()?;
		let mut duplicate = fixture.providers[1].clone();
		if let ProviderIdentity::DataMod { priority, .. } = &mut duplicate.identity {
			*priority = ModPriority::new(99);
		}
		fixture.providers.push(duplicate);
		assert!(fixture.configure(None).is_err());
		Ok(())
	}

	#[test]
	fn mapping_paths_remain_validated_without_winners() -> StdResult<(), Box<dyn Error>> {
		for invalid in [PathBuf::from("relative"), root().join("bad\0path")] {
			for field in 0..8 {
				let mut fixture = Fixture::new()?;
				fixture.winners.clear();
				match field {
					0 => fixture.data_directory = invalid.clone(),
					1 => fixture.providers[3].root = invalid.clone(),
					2 => fixture.profile_directories[0].source = invalid.clone(),
					3 => fixture.profile_directories[0].destination = invalid.clone(),
					4 => fixture.profile_files[0].source = invalid.clone(),
					5 => fixture.profile_files[0].destination = invalid.clone(),
					6 => fixture.saves.source = invalid.clone(),
					_ => fixture.saves.destination = invalid.clone(),
				}
				assert!(fixture.configure(None).is_err(), "field {field}: {invalid:?}");
			}
		}
		Ok(())
	}
}
