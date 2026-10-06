use crate::commands::Command;
use crate::commands::ConfigCommand;
use crate::commands::ConflictsCommand;
use crate::commands::ExecArgs;
use crate::commands::ExportArgs;
use crate::commands::InstallArgs;
use crate::commands::SetCommand;
use crate::commands::SettingKeyArgument;
use crate::commands::ShortcutArgs;
use crate::operation;
use application::ErrorMarker;
use application::conflicts::ExplainPathDependencies;
use application::conflicts::InspectModConflictsDependencies;
use application::conflicts::ListEffectiveConflictsDependencies;
use application::environment::InitializeEnvironmentDependencies;
use application::execution::ExecuteProgram;
use application::export::ExportEnvironmentDependencies;
use application::installation::InstallModDependencies;
use application::settings::GetSettingDependencies;
use application::settings::ListSettingsDependencies;
use application::settings::ResolvedSettings;
use application::settings::SetGameDirectoryDependencies;
use application::shortcut::CreateShortcutDependencies;
use domain::EnvironmentRoot;
use infrastructure_dependencies::LoadedSettings;
use infrastructure_dependencies::Resources;
use infrastructure_dependencies::SettingsLoadMode;
use rootcause::Result as RootResult;
use std::path::Path;
use std::path::PathBuf;
use tokio_util::sync::CancellationToken;

pub(crate) struct Invocation {
	pub(crate) operation_name: &'static str,
	pub(crate) quiet_success: bool,
	pub(crate) exit_status_family: ExitStatusFamily,
	pub(crate) composition: Composition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExitStatusFamily {
	Ordinary,
	Execution,
}

pub(crate) enum Composition {
	NewEnvironment(ComposeNewEnvironment),
	ExistingEnvironment(SettingsLoadMode, ComposeExistingEnvironment),
}

type ComposeNewEnvironment = Box<dyn FnOnce(&Resources) -> PreparedCommand>;

type ComposeExistingEnvironment = Box<dyn FnOnce(&Resources, LoadedSettings, &Path) -> PreparedCommand>;

pub(crate) enum PreparedCommand {
	InitializeEnvironment {
		dependencies: InitializeEnvironmentDependencies,
		game_install: Option<PathBuf>,
	},
	ListSettings {
		dependencies: ListSettingsDependencies,
		settings: ResolvedSettings,
	},
	GetSetting {
		dependencies: GetSettingDependencies,
		settings: ResolvedSettings,
		key: SettingKeyArgument,
	},
	SetGameDirectory {
		dependencies: SetGameDirectoryDependencies,
		value: PathBuf,
	},
	InstallMod {
		dependencies: InstallModDependencies,
		arguments: InstallArgs,
	},
	ListEffectiveConflicts {
		dependencies: ListEffectiveConflictsDependencies,
		compare_content: bool,
	},
	InspectModConflicts {
		dependencies: InspectModConflictsDependencies,
		mod_name: String,
		compare_content: bool,
	},
	ExplainPath {
		dependencies: ExplainPathDependencies,
		path: String,
		compare_content: bool,
	},
	ExportEnvironment {
		dependencies: ExportEnvironmentDependencies,
		arguments: ExportArgs,
	},
	CreateShortcut {
		dependencies: CreateShortcutDependencies,
		settings: ResolvedSettings,
		arguments: ShortcutArgs,
	},
	ExecuteProgram {
		execute_program: ExecuteProgram,
		force_cancellation: CancellationToken,
		arguments: ExecArgs,
	},
}

pub(crate) fn invocation(command: Command) -> Invocation {
	match command {
		Command::Init { game_install } => Invocation {
			operation_name: "initialize",
			quiet_success: false,
			exit_status_family: ExitStatusFamily::Ordinary,
			composition: Composition::NewEnvironment(Box::new(|resources| {
				PreparedCommand::InitializeEnvironment {
					dependencies: resources.initialize_environment_dependencies(),
					game_install,
				}
			})),
		},
		Command::Config {
			command: ConfigCommand::List,
		} => Invocation {
			operation_name: "config.list",
			quiet_success: false,
			exit_status_family: ExitStatusFamily::Ordinary,
			composition: Composition::ExistingEnvironment(
				SettingsLoadMode::ReadOnly,
				Box::new(|resources, loaded, _| PreparedCommand::ListSettings {
					dependencies: resources.list_settings_dependencies(),
					settings: loaded.resolved,
				}),
			),
		},
		Command::Config {
			command: ConfigCommand::Get { key },
		} => Invocation {
			operation_name: "config.get",
			quiet_success: false,
			exit_status_family: ExitStatusFamily::Ordinary,
			composition: Composition::ExistingEnvironment(
				SettingsLoadMode::ReadOnly,
				Box::new(move |resources, loaded, _| PreparedCommand::GetSetting {
					dependencies: resources.get_setting_dependencies(),
					settings: loaded.resolved,
					key,
				}),
			),
		},
		Command::Config {
			command: ConfigCommand::Set {
				command: SetCommand::GameDir { value },
			},
		} => Invocation {
			operation_name: "config.set.game_dir",
			quiet_success: false,
			exit_status_family: ExitStatusFamily::Ordinary,
			composition: Composition::ExistingEnvironment(
				SettingsLoadMode::Mutation,
				Box::new(|resources, loaded, _| PreparedCommand::SetGameDirectory {
					dependencies: resources.set_game_directory_dependencies(loaded),
					value,
				}),
			),
		},
		Command::Install(arguments) => Invocation {
			operation_name: "install",
			quiet_success: false,
			exit_status_family: ExitStatusFamily::Ordinary,
			composition: Composition::ExistingEnvironment(
				if arguments.dry_run {
					SettingsLoadMode::Inspection
				} else {
					SettingsLoadMode::Execution
				},
				Box::new(|resources, loaded, _| PreparedCommand::InstallMod {
					dependencies: resources.install_mod_dependencies(&loaded),
					arguments,
				}),
			),
		},
		Command::Conflicts {
			command: ConflictsCommand::List { compare_content },
		} => Invocation {
			operation_name: "conflicts.list",
			quiet_success: false,
			exit_status_family: ExitStatusFamily::Ordinary,
			composition: Composition::ExistingEnvironment(
				SettingsLoadMode::Inspection,
				Box::new(move |resources, loaded, _| PreparedCommand::ListEffectiveConflicts {
					dependencies: resources.list_effective_conflicts_dependencies(
						loaded.resolved.effective_binding,
					),
					compare_content,
				}),
			),
		},
		Command::Conflicts {
			command: ConflictsCommand::Inspect {
				mod_name,
				compare_content,
			},
		} => Invocation {
			operation_name: "conflicts.inspect",
			quiet_success: false,
			exit_status_family: ExitStatusFamily::Ordinary,
			composition: Composition::ExistingEnvironment(
				SettingsLoadMode::Inspection,
				Box::new(move |resources, loaded, _| PreparedCommand::InspectModConflicts {
					dependencies: resources
						.inspect_mod_conflicts_dependencies(loaded.resolved.effective_binding),
					mod_name,
					compare_content,
				}),
			),
		},
		Command::Conflicts {
			command: ConflictsCommand::Explain { path, compare_content },
		} => Invocation {
			operation_name: "conflicts.explain",
			quiet_success: false,
			exit_status_family: ExitStatusFamily::Ordinary,
			composition: Composition::ExistingEnvironment(
				SettingsLoadMode::Inspection,
				Box::new(move |resources, loaded, _| PreparedCommand::ExplainPath {
					dependencies: resources
						.explain_path_dependencies(loaded.resolved.effective_binding),
					path,
					compare_content,
				}),
			),
		},
		Command::Export(arguments) => Invocation {
			operation_name: "export",
			quiet_success: false,
			exit_status_family: ExitStatusFamily::Ordinary,
			composition: Composition::ExistingEnvironment(
				SettingsLoadMode::Execution,
				Box::new(|resources, loaded, _| PreparedCommand::ExportEnvironment {
					dependencies: resources
						.export_environment_dependencies(loaded.resolved.effective_binding),
					arguments,
				}),
			),
		},
		Command::Exec(arguments) => Invocation {
			operation_name: "exec",
			quiet_success: false,
			exit_status_family: ExitStatusFamily::Execution,
			composition: Composition::ExistingEnvironment(
				SettingsLoadMode::Execution,
				Box::new(|resources, loaded, startup| {
					let binding = loaded.resolved.effective_binding;
					let force_cancellation = CancellationToken::new();
					let execute_program = if arguments.hidden {
						resources
							.captured_execute_program(
								binding,
								startup.to_owned(),
								force_cancellation.clone(),
							)
							.0
					} else {
						resources.execute_program(
							binding,
							startup.to_owned(),
							force_cancellation.clone(),
						)
					};

					PreparedCommand::ExecuteProgram {
						execute_program,
						force_cancellation,
						arguments,
					}
				}),
			),
		},
		Command::Shortcut(arguments) => Invocation {
			operation_name: "shortcut",
			quiet_success: true,
			exit_status_family: ExitStatusFamily::Ordinary,
			composition: Composition::ExistingEnvironment(
				SettingsLoadMode::Execution,
				Box::new(|resources, loaded, startup| PreparedCommand::CreateShortcut {
					dependencies: resources.create_shortcut_dependencies(
						loaded.resolved.effective_binding.clone(),
						startup.to_owned(),
					),
					settings: loaded.resolved,
					arguments,
				}),
			),
		},
	}
}

pub(crate) async fn compose(
	root: &EnvironmentRoot,
	startup: &Path,
	composition: Composition,
) -> RootResult<PreparedCommand, ErrorMarker> {
	let resources = Resources::system(root.clone());

	match composition {
		Composition::NewEnvironment(compose_command) => Ok(compose_command(&resources)),
		Composition::ExistingEnvironment(settings_load_mode, compose_command) => {
			let loaded = resources
				.load_settings(settings_load_mode, &operation::ctrl_c_token())
				.await?;

			Ok(compose_command(&resources, loaded, startup))
		}
	}
}

#[cfg(test)]
mod tests {
	use super::PreparedCommand;
	use super::compose;
	use super::invocation;
	use crate::commands::SettingKeyArgument;
	use crate::commands::parse_from;
	use application::ErrorCode;
	use application::ErrorMarker;
	use domain::EnvironmentRoot;
	use infrastructure_dependencies::SettingsLoadMode;
	use rootcause::Result as RootResult;
	use std::error::Error;
	use std::ffi::OsString;
	use std::fs::create_dir_all;
	use std::fs::write;
	use std::path::Path;
	use tempfile::TempDir;

	type PreparesOwnForm = fn(&PreparedCommand) -> bool;

	async fn composed(
		arguments: &[&str],
		root: &Path,
	) -> Result<RootResult<PreparedCommand, ErrorMarker>, Box<dyn Error>> {
		let cli = parse_from(
			[OsString::from("mods")]
				.into_iter()
				.chain(arguments.iter().map(OsString::from)),
		)?;
		let root = EnvironmentRoot::new(root.to_owned()).map_err(|_| "environment root fixture")?;

		Ok(compose(&root, root.as_path(), invocation(cli.command).composition).await)
	}

	fn write_manifest(root: &Path, game: &Path) -> Result<(), Box<dyn Error>> {
		write(
			root.join("mods.toml"),
			format!("schema_version = 1\ngame_dir = '{}'\n", game.display()),
		)?;
		Ok(())
	}

	#[tokio::test]
	async fn every_command_composes_its_prepared_form_after_loading_settings_in_its_mode()
	-> Result<(), Box<dyn Error>> {
		let temp = TempDir::new()?;
		let game = temp.path().join("game");

		let valid_environment = temp.path().join("valid-environment");
		for directory in ["mods", "profile/saves", "overwrite", "temp"] {
			create_dir_all(valid_environment.join(directory))?;
		}
		for file in ["plugins.txt", "modlist.txt"] {
			write(valid_environment.join("profile").join(file), b"")?;
		}
		write(
			valid_environment.join("profile/Fallout.ini"),
			"[General]\nbUseMyGamesDirectory=1\nSLocalSavePath=Saves\\\n",
		)?;
		write_manifest(&valid_environment, &game)?;

		let unvalidated_layout = temp.path().join("unvalidated-layout");
		for directory in ["mods", "profile", "overwrite", "temp"] {
			create_dir_all(unvalidated_layout.join(directory))?;
		}
		write_manifest(&unvalidated_layout, &game)?;

		let unfinished_operation = temp.path().join("unfinished-operation");
		create_dir_all(unfinished_operation.join("temp"))?;
		write(unfinished_operation.join("temp/pending"), b"")?;

		let cases: [(&[&str], Option<SettingsLoadMode>, PreparesOwnForm); 13] = [
			(&["init", "--game-install", "game"], None, |prepared| {
				matches!(
					prepared,
					PreparedCommand::InitializeEnvironment { game_install: Some(path), .. }
						if path == Path::new("game")
				)
			}),
			(&["config", "list"], Some(SettingsLoadMode::ReadOnly), |prepared| {
				matches!(prepared, PreparedCommand::ListSettings { .. })
			}),
			(
				&["config", "get", "game-dir"],
				Some(SettingsLoadMode::ReadOnly),
				|prepared| {
					matches!(
						prepared,
						PreparedCommand::GetSetting {
							key: SettingKeyArgument::GameDir,
							..
						}
					)
				},
			),
			(
				&["config", "set", "game-dir", "game"],
				Some(SettingsLoadMode::Mutation),
				|prepared| {
					matches!(
						prepared,
						PreparedCommand::SetGameDirectory { value, .. }
							if value == Path::new("game")
					)
				},
			),
			(
				&["install", "archive.zip", "--dry-run"],
				Some(SettingsLoadMode::Inspection),
				|prepared| {
					matches!(
						prepared,
						PreparedCommand::InstallMod { arguments, .. }
							if arguments.dry_run && arguments.archive == Path::new("archive.zip")
					)
				},
			),
			(
				&["install", "archive.zip"],
				Some(SettingsLoadMode::Execution),
				|prepared| {
					matches!(
						prepared,
						PreparedCommand::InstallMod { arguments, .. }
							if !arguments.dry_run
					)
				},
			),
			(
				&["conflicts", "list", "--compare-content"],
				Some(SettingsLoadMode::Inspection),
				|prepared| {
					matches!(
						prepared,
						PreparedCommand::ListEffectiveConflicts {
							compare_content: true,
							..
						}
					)
				},
			),
			(
				&["conflicts", "inspect", "Visuals"],
				Some(SettingsLoadMode::Inspection),
				|prepared| {
					matches!(
						prepared,
						PreparedCommand::InspectModConflicts { mod_name, compare_content: false, .. }
							if mod_name == "Visuals"
					)
				},
			),
			(
				&["conflicts", "explain", "file.esp", "--compare-content"],
				Some(SettingsLoadMode::Inspection),
				|prepared| {
					matches!(
						prepared,
						PreparedCommand::ExplainPath { path, compare_content: true, .. }
							if path == "file.esp"
					)
				},
			),
			(&["export", "payload"], Some(SettingsLoadMode::Execution), |prepared| {
				matches!(
					prepared,
					PreparedCommand::ExportEnvironment { arguments, .. }
						if arguments.output == Path::new("payload")
				)
			}),
			(
				&["exec", "--", "tool.exe"],
				Some(SettingsLoadMode::Execution),
				|prepared| {
					matches!(
						prepared,
						PreparedCommand::ExecuteProgram { arguments, .. }
							if !arguments.hidden && arguments.command == ["tool.exe"]
					)
				},
			),
			(
				&["exec", "--hidden", "--", "tool.exe"],
				Some(SettingsLoadMode::Execution),
				|prepared| {
					matches!(
						prepared,
						PreparedCommand::ExecuteProgram { arguments, .. }
							if arguments.hidden
					)
				},
			),
			(
				&["shortcut", "--", "tool.exe"],
				Some(SettingsLoadMode::Execution),
				|prepared| {
					matches!(
						prepared,
						PreparedCommand::CreateShortcut { arguments, .. }
							if arguments.command == ["tool.exe"]
					)
				},
			),
		];
		for (arguments, expected_mode, prepares_own_form) in cases {
			let prepared = composed(arguments, &valid_environment)
				.await?
				.map_err(|report| format!("{arguments:?} did not compose: {report}"))?;
			let layout_failure = composed(arguments, &unvalidated_layout)
				.await?
				.err()
				.map(|report| report.current_context().code());
			let unfinished_operation_failure = composed(arguments, &unfinished_operation)
				.await?
				.err()
				.map(|report| report.current_context().code());

			let observed_mode = match (layout_failure, unfinished_operation_failure) {
				(None, None) => None,
				(Some(_), Some(ErrorCode::EnvironmentInvalid)) => Some(SettingsLoadMode::ReadOnly),
				(None, Some(ErrorCode::EnvironmentInvalid)) => Some(SettingsLoadMode::Inspection),
				(Some(_), Some(ErrorCode::ManualCleanupRequired)) => Some(SettingsLoadMode::Mutation),
				(None, Some(ErrorCode::ManualCleanupRequired)) => Some(SettingsLoadMode::Execution),
				failures => {
					return Err(format!("{arguments:?} failed unexpectedly: {failures:?}").into());
				}
			};
			assert_eq!(observed_mode, expected_mode, "{arguments:?}");
			assert!(prepares_own_form(&prepared), "{arguments:?}");
		}
		Ok(())
	}
}
