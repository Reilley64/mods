use crate::PathMapping;
use application::ports::ProfileWarning;
use domain::DataRelativePath;
use domain::canonical_profile_routing_valid;
use domain::case_fold_key;
use domain::profile_test_file_slots;
use rootcause::Result;
use rootcause::report;
use std::collections::HashMap;
use std::collections::HashSet;
use std::error::Error;
use std::fmt;
use std::path::Path;

const PROFILE_FILES: [&str; 8] = [
	"Fallout.ini",
	"FalloutPrefs.ini",
	"FalloutCustom.ini",
	"GECKCustom.ini",
	"GECKPrefs.ini",
	"plugins.txt",
	"loadorder.txt",
	"Plugins.fnvviewsettings",
];
const INVALIDATION_ARCHIVE: &str = "Fallout - Invalidation.bsa";

/// Decoded canonical text. The caller must reject decoding failures and use the
/// Windows active code page for plugins.txt and UTF-8 for loadorder.txt.
#[derive(Debug, Clone, Copy)]
pub struct ProfileText<'a> {
	pub name: &'a str,
	pub text: &'a str,
}

/// An analytical Data winner not observed runtime visibility.
#[derive(Debug, Clone)]
pub struct VisibleProfileFile {
	pub path: DataRelativePath,
}

/// Resolved Fallout-specific directories and decoded state; no save contents.
pub struct ProfileConfigurationInput<'a> {
	pub files: &'a [ProfileText<'a>],
	pub visible_files: &'a [VisibleProfileFile],
	pub profile_directory: &'a Path,
	pub documents_directory: &'a Path,
	pub local_app_data_directory: &'a Path,
	pub data_directory: &'a Path,
	pub cache_directory: &'a Path,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActivationSource {
	BaseGame,
	PluginsFile,
	NamFile,
	TestFile { file: String, key: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectivePlugin {
	pub path: DataRelativePath,
	pub activation_sources: Vec<ActivationSource>,
}

#[derive(Debug)]
pub struct ProfileConfiguration {
	pub profile_directories: Vec<PathMapping>,
	pub profile_files: Vec<PathMapping>,
	pub saves: PathMapping,
	pub invalidation_mapping: PathMapping,
	pub plugins: Vec<EffectivePlugin>,
	pub warnings: Vec<ProfileWarning>,
}

/// A canonical file diagnostic. Line zero identifies a missing key or input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileConfigurationError {
	pub file: String,
	pub line: usize,
	pub value: String,
	pub expected: &'static str,
}

impl fmt::Display for ProfileConfigurationError {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "{}:{}: expected {}", self.file, self.line, self.expected)
	}
}
impl Error for ProfileConfigurationError {}

/// Builds canonical profile mappings and an advisory analytical plugin projection.
/// The projection does not establish runtime visibility, activation, or order and
/// does not edit canonical state or select the files used by profile mappings.
///
/// # Errors
///
/// Rejects malformed plugin lists, invalid routing/invalidation keys, duplicate
/// inputs.
pub fn build_profile_configuration(
	input: ProfileConfigurationInput<'_>,
) -> Result<ProfileConfiguration, ProfileConfigurationError> {
	let mut profile_texts = HashMap::new();
	for file in input.files {
		let key = case_fold_key(file.name);
		if !PROFILE_FILES.iter().any(|name| name.eq_ignore_ascii_case(file.name))
			|| profile_texts.insert(key, file.text).is_some()
		{
			return Err(report!(ProfileConfigurationError {
				file: file.name.into(),
				line: 0,
				value: file.name.into(),
				expected: "one supported canonical Profile State file"
			}));
		}
	}

	let mut test_files = Vec::new();
	for name in PROFILE_FILES.iter().take(5) {
		let text = profile_texts.get(&case_fold_key(name)).copied().unwrap_or_default();
		if ["Fallout.ini", "FalloutPrefs.ini", "FalloutCustom.ini"].contains(name)
			&& !canonical_profile_routing_valid(name, text)
		{
			return Err(report!(ProfileConfigurationError {
				file: (*name).into(),
				line: 0,
				value: String::new(),
				expected: "canonical normal Saves routing without conflicting overrides"
			}));
		}
		for (slot, value) in profile_test_file_slots(text).into_iter().enumerate() {
			if let Some(plugin) = value.and_then(plugin_path) {
				test_files.push((
					plugin.comparison_key().to_owned(),
					ActivationSource::TestFile {
						file: (*name).into(),
						key: format!("sTestFile{}", slot + 1),
					},
				));
			}
		}
	}

	let mut visible = HashMap::new();
	for file in input.visible_files {
		if visible.insert(file.path.comparison_key(), file).is_some() {
			return Err(report!(ProfileConfigurationError {
				file: "Data".into(),
				line: 0,
				value: file.path.to_string(),
				expected: "unique effective Data file"
			}));
		}
	}

	let mut warnings = Vec::new();
	let mut explicitly_enabled_plugins = HashSet::new();
	let mut ordered_plugins = Vec::new();
	for name in ["plugins.txt", "loadorder.txt"] {
		let text = profile_texts.get(name).copied().unwrap_or_default();
		if text.replace("\r\n", "").contains(['\r', '\n']) {
			return Err(report!(ProfileConfigurationError {
				file: name.into(),
				line: 0,
				value: String::new(),
				expected: "CRLF line endings"
			}));
		}
		let mut seen = HashSet::new();
		let mut duplicate_warnings = HashSet::new();
		for (index, line) in text.split("\r\n").enumerate() {
			if line.is_empty() || line.starts_with('#') {
				continue;
			}
			let Some(path) = plugin_path(line) else {
				return Err(report!(ProfileConfigurationError {
					file: name.into(),
					line: index + 1,
					value: line.into(),
					expected: "bare .esm or .esp filename"
				}));
			};
			let key = path.comparison_key().to_owned();
			if !seen.insert(key.clone()) {
				if duplicate_warnings.insert(key) {
					warnings.push(ProfileWarning::Duplicate {
						file: name.into(),
						plugin: line.into(),
					});
				}
				continue;
			}
			let Some(file) = visible.get(key.as_str()) else {
				warnings.push(ProfileWarning::Unavailable {
					file: name.into(),
					plugin: line.into(),
				});
				continue;
			};
			if name == "plugins.txt" {
				explicitly_enabled_plugins.insert(key);
			} else {
				ordered_plugins.push(*file);
			}
		}
	}

	let listed: HashSet<_> = ordered_plugins.iter().map(|file| file.path.comparison_key()).collect();
	let unlisted: Vec<_> = input
		.visible_files
		.iter()
		.filter(|file| {
			plugin_path(file.path.as_str()).is_some() && !listed.contains(file.path.comparison_key())
		})
		.collect();
	for file in &unlisted {
		warnings.push(ProfileWarning::Unlisted {
			plugin: file.path.to_string(),
		});
	}
	ordered_plugins.extend(unlisted);
	if let Some(index) = ordered_plugins
		.iter()
		.position(|file| file.path.comparison_key() == "falloutnv.esm")
	{
		let base = ordered_plugins.remove(index);
		ordered_plugins.insert(0, base);
	}

	let mut plugins = Vec::new();
	for file in ordered_plugins {
		let key = file.path.comparison_key();
		let mut activation_sources = Vec::new();
		if key == "falloutnv.esm" {
			activation_sources.push(ActivationSource::BaseGame);
		}
		if explicitly_enabled_plugins.contains(key) {
			activation_sources.push(ActivationSource::PluginsFile);
		}
		if let Some((stem, _)) = key.rsplit_once('.')
			&& visible.contains_key(format!("{stem}.nam").as_str())
		{
			activation_sources.push(ActivationSource::NamFile);
		}
		activation_sources.extend(test_files
			.iter()
			.filter(|(plugin, _)| plugin == key)
			.map(|(_, source)| source.clone()));
		plugins.push(EffectivePlugin {
			path: file.path.clone(),
			activation_sources,
		});
	}
	warnings.push(ProfileWarning::LoadOrderNotEnforced);

	let profile_files = PROFILE_FILES
		.iter()
		.enumerate()
		.map(|(index, name)| PathMapping {
			source: input.profile_directory.join(name),
			destination: if index < 5 {
				input.documents_directory.join(name)
			} else {
				input.local_app_data_directory.join(name)
			},
		})
		.collect();
	// Upstream requires each destination parent in its tree, but does not require
	// a directory link's source to exist. Identity links add virtual containers
	// without creating user directories or routing unrelated writes into Profile State.
	let mut profile_directories = Vec::new();
	if let Some(parent) = input.documents_directory.parent() {
		profile_directories.push(PathMapping {
			source: parent.to_owned(),
			destination: parent.to_owned(),
		});
	}
	for directory in [input.documents_directory, input.local_app_data_directory] {
		profile_directories.push(PathMapping {
			source: directory.to_owned(),
			destination: directory.to_owned(),
		});
	}

	Ok(ProfileConfiguration {
		profile_directories,
		profile_files,
		saves: PathMapping {
			source: input.profile_directory.join("saves"),
			destination: input.documents_directory.join("__mods_saves"),
		},
		invalidation_mapping: PathMapping {
			source: input.cache_directory.join(INVALIDATION_ARCHIVE),
			destination: input.data_directory.join(INVALIDATION_ARCHIVE),
		},
		plugins,
		warnings,
	})
}

fn plugin_path(name: &str) -> Option<DataRelativePath> {
	if name.trim() != name {
		return None;
	}
	let path = DataRelativePath::new(name.to_owned()).ok()?;
	let key = path.comparison_key();
	if path.components().count() != 1 || !(key.ends_with(".esm") || key.ends_with(".esp")) {
		return None;
	}
	Some(path)
}

#[cfg(test)]
mod tests {
	use super::*;
	const VALID: &str = "[Archive]\r\nbInvalidateOlderFiles=1\r\nSInvalidationFile=\r\nsArchiveList=Fallout - Invalidation.bsa, DLC.bsa\r\n[General]\r\nbUseMyGamesDirectory=1\r\nSLocalSavePath=Saves\\\r\n";

	fn build(
		files: &[ProfileText<'_>],
		visible: &[VisibleProfileFile],
	) -> Result<ProfileConfiguration, ProfileConfigurationError> {
		build_profile_configuration(ProfileConfigurationInput {
			files,
			visible_files: visible,
			profile_directory: Path::new("/environment/profile"),
			documents_directory: Path::new("/documents/FalloutNV"),
			local_app_data_directory: Path::new("/local/FalloutNV"),
			data_directory: Path::new("/game/Data"),
			cache_directory: Path::new("/environment/cache"),
		})
	}
	fn visible(name: &str) -> VisibleProfileFile {
		VisibleProfileFile {
			path: DataRelativePath::new(name.to_owned()).unwrap_or_else(|error| unreachable!("{error}")),
		}
	}

	#[test]
	fn derives_lenient_order_and_all_activation_sources_without_rewriting() -> Result<(), ProfileConfigurationError>
	{
		let files = [
			ProfileText {
				name: "Fallout.ini",
				text: VALID,
			},
			ProfileText {
				name: "plugins.txt",
				text: "Absent.esp\r\nB.esp\r\nb.ESP\r\n",
			},
			ProfileText {
				name: "loadorder.txt",
				text: "B.esp\r\nAbsent.esp\r\nb.esp\r\n",
			},
			ProfileText {
				name: "GECKCustom.ini",
				text: "[General]\nsTestFile1=A.esp\nsTestFile1=B.esp\n",
			},
		];
		let output = build(
			&files,
			&[
				visible("A.esp"),
				visible("B.esp"),
				visible("FalloutNV.esm"),
				visible("A.nam"),
			],
		)?;
		assert_eq!(
			output.plugins.iter().map(|p| p.path.as_str()).collect::<Vec<_>>(),
			["FalloutNV.esm", "B.esp", "A.esp"]
		);
		assert_eq!(output.plugins[0].activation_sources, [ActivationSource::BaseGame]);
		assert_eq!(
			output.plugins[1].activation_sources,
			[
				ActivationSource::PluginsFile,
				ActivationSource::TestFile {
					file: "GECKCustom.ini".into(),
					key: "sTestFile1".into()
				}
			]
		);
		assert_eq!(output.plugins[2].activation_sources, [ActivationSource::NamFile]);
		assert_eq!(output.warnings.len(), 7);
		assert_eq!(files[1].text, "Absent.esp\r\nB.esp\r\nb.ESP\r\n");
		Ok(())
	}

	#[test]
	fn emits_named_mappings_save_route_and_reserved_archive_mapping() -> Result<(), ProfileConfigurationError> {
		let output = build(
			&[ProfileText {
				name: "Fallout.ini",
				text: VALID,
			}],
			&[],
		)?;
		assert_eq!(
			output.profile_directories
				.iter()
				.map(|mapping| mapping.destination.as_path())
				.collect::<Vec<_>>(),
			[
				Path::new("/documents"),
				Path::new("/documents/FalloutNV"),
				Path::new("/local/FalloutNV")
			]
		);
		assert!(output
			.profile_directories
			.iter()
			.all(|mapping| mapping.source == mapping.destination));
		assert_eq!(output.profile_files.len(), 8);
		assert_eq!(output.saves.source, Path::new("/environment/profile/saves"));
		assert_eq!(output.saves.destination, Path::new("/documents/FalloutNV/__mods_saves"));
		assert_eq!(
			output.invalidation_mapping.source,
			Path::new("/environment/cache/Fallout - Invalidation.bsa")
		);
		assert_eq!(
			output.invalidation_mapping.destination,
			Path::new("/game/Data/Fallout - Invalidation.bsa")
		);
		Ok(())
	}

	#[test]
	fn rejects_invalid_keys_overrides_and_reserved_data_path() {
		for text in [VALID.replace("=1", "=0"), format!("{VALID}SLocalSavePath=Saves\\\n")] {
			assert!(build(
				&[ProfileText {
					name: "Fallout.ini",
					text: &text
				}],
				&[]
			)
			.is_err());
		}
		assert!(build(
			&[
				ProfileText {
					name: "Fallout.ini",
					text: VALID
				},
				ProfileText {
					name: "FalloutPrefs.ini",
					text: "[Other]\nSLocalSavePath=wrong"
				}
			],
			&[]
		)
		.is_err());
		assert!(build(
			&[ProfileText {
				name: "Fallout.ini",
				text: VALID
			}],
			&[visible("fallout - INVALIDATION.BSA")]
		)
		.is_ok());
	}

	#[test]
	fn unlisted_plugins_keep_input_collection_order() -> Result<(), ProfileConfigurationError> {
		let output = build(
			&[ProfileText {
				name: "Fallout.ini",
				text: VALID,
			}],
			&[visible("z.esp"), visible("B.esp"), visible("a.esp")],
		)?;
		assert_eq!(
			output.plugins
				.iter()
				.map(|plugin| plugin.path.as_str())
				.collect::<Vec<_>>(),
			["z.esp", "B.esp", "a.esp"]
		);
		assert!(output.plugins.iter().all(|plugin| plugin.activation_sources.is_empty()));
		Ok(())
	}

	#[test]
	fn unavailable_duplicate_warnings_are_distinct_and_include_canonical_line_errors()
	-> Result<(), ProfileConfigurationError> {
		let output = build(
			&[
				ProfileText {
					name: "Fallout.ini",
					text: VALID,
				},
				ProfileText {
					name: "plugins.txt",
					text: "Gone.esp\r\ngone.esp\r\nGONE.esp\r\n",
				},
			],
			&[],
		)?;
		assert_eq!(output.warnings.len(), 3);
		let result = build(
			&[
				ProfileText {
					name: "Fallout.ini",
					text: VALID,
				},
				ProfileText {
					name: "plugins.txt",
					text: "# comment\r\n../bad.esp\r\n",
				},
			],
			&[],
		);
		let Err(error) = result else {
			unreachable!("invalid list must fail")
		};
		assert_eq!(error.current_context().file, "plugins.txt");
		assert_eq!(error.current_context().line, 2);
		Ok(())
	}

	#[test]
	fn test_file_assignments_settle_independently_before_execution_eligibility()
	-> Result<(), ProfileConfigurationError> {
		let fallout = format!("{VALID}sTestFile1=First.esp\nsTestFile2=First.esp\nsTestFile2=Light.esl\n");
		let output = build(
			&[
				ProfileText {
					name: "Fallout.ini",
					text: &fallout,
				},
				ProfileText {
					name: "GECKCustom.ini",
					text: "[general]\nsTESTfile1=Second.ESP\nsTestFile01=First.esp\n",
				},
			],
			&[visible("First.esp"), visible("Second.esp"), visible("Light.esl")],
		)?;
		assert_eq!(output.plugins.len(), 2);
		assert_eq!(
			output.plugins[0].activation_sources,
			[ActivationSource::TestFile {
				file: "Fallout.ini".into(),
				key: "sTestFile1".into()
			}]
		);
		assert_eq!(
			output.plugins[1].activation_sources,
			[ActivationSource::TestFile {
				file: "GECKCustom.ini".into(),
				key: "sTestFile1".into()
			}]
		);
		Ok(())
	}

	#[test]
	fn rejects_non_bare_and_unsupported_entries() {
		for text in ["*A.esp\r\n", "sub/A.esp\r\n", "A.esl\r\n", "A.esp\n", " A.esp\r\n"] {
			assert!(build(
				&[
					ProfileText {
						name: "Fallout.ini",
						text: VALID
					},
					ProfileText {
						name: "plugins.txt",
						text
					}
				],
				&[]
			)
			.is_err());
		}
	}
}
