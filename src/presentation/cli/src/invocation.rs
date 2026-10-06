use crate::commands::Command;
use crate::commands::ConfigCommand;
use crate::commands::ConflictsCommand;
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
	NewEnvironment(fn(&Resources) -> CommandDependencies),
	ExistingEnvironment(SettingsLoadMode, ComposeExistingEnvironment),
}

type ComposeExistingEnvironment = Box<dyn FnOnce(&Resources, LoadedSettings, &Path) -> CommandDependencies>;

pub(crate) enum CommandDependencies {
	InitializeEnvironment(InitializeEnvironmentDependencies),
	ListSettings {
		dependencies: ListSettingsDependencies,
		settings: ResolvedSettings,
	},
	GetSetting {
		dependencies: GetSettingDependencies,
		settings: ResolvedSettings,
	},
	SetGameDirectory(SetGameDirectoryDependencies),
	InstallMod(InstallModDependencies),
	ListEffectiveConflicts(ListEffectiveConflictsDependencies),
	InspectModConflicts(InspectModConflictsDependencies),
	ExplainPath(ExplainPathDependencies),
	ExportEnvironment(ExportEnvironmentDependencies),
	CreateShortcut {
		dependencies: CreateShortcutDependencies,
		settings: ResolvedSettings,
	},
	ExecuteProgram {
		execute_program: ExecuteProgram,
		force_cancellation: CancellationToken,
	},
}

pub(crate) fn invocation(command: &Command) -> Invocation {
	match command {
		Command::Init { .. } => Invocation {
			operation_name: "initialize",
			quiet_success: false,
			exit_status_family: ExitStatusFamily::Ordinary,
			composition: Composition::NewEnvironment(|resources| {
				CommandDependencies::InitializeEnvironment(
					resources.initialize_environment_dependencies(),
				)
			}),
		},
		Command::Config {
			command: ConfigCommand::List,
		} => Invocation {
			operation_name: "config.list",
			quiet_success: false,
			exit_status_family: ExitStatusFamily::Ordinary,
			composition: Composition::ExistingEnvironment(
				SettingsLoadMode::ReadOnly,
				Box::new(|resources, loaded, _| CommandDependencies::ListSettings {
					dependencies: resources.list_settings_dependencies(),
					settings: loaded.resolved,
				}),
			),
		},
		Command::Config {
			command: ConfigCommand::Get { .. },
		} => Invocation {
			operation_name: "config.get",
			quiet_success: false,
			exit_status_family: ExitStatusFamily::Ordinary,
			composition: Composition::ExistingEnvironment(
				SettingsLoadMode::ReadOnly,
				Box::new(|resources, loaded, _| CommandDependencies::GetSetting {
					dependencies: resources.get_setting_dependencies(),
					settings: loaded.resolved,
				}),
			),
		},
		Command::Config {
			command: ConfigCommand::Set { .. },
		} => Invocation {
			operation_name: "config.set.game_dir",
			quiet_success: false,
			exit_status_family: ExitStatusFamily::Ordinary,
			composition: Composition::ExistingEnvironment(
				SettingsLoadMode::Mutation,
				Box::new(|resources, loaded, _| {
					CommandDependencies::SetGameDirectory(
						resources.set_game_directory_dependencies(loaded),
					)
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
				Box::new(|resources, loaded, _| {
					CommandDependencies::InstallMod(resources.install_mod_dependencies(&loaded))
				}),
			),
		},
		Command::Conflicts {
			command: ConflictsCommand::List { .. },
		} => Invocation {
			operation_name: "conflicts.list",
			quiet_success: false,
			exit_status_family: ExitStatusFamily::Ordinary,
			composition: Composition::ExistingEnvironment(
				SettingsLoadMode::Inspection,
				Box::new(|resources, loaded, _| {
					CommandDependencies::ListEffectiveConflicts(
						resources.list_effective_conflicts_dependencies(
							loaded.resolved.effective_binding,
						),
					)
				}),
			),
		},
		Command::Conflicts {
			command: ConflictsCommand::Inspect { .. },
		} => Invocation {
			operation_name: "conflicts.inspect",
			quiet_success: false,
			exit_status_family: ExitStatusFamily::Ordinary,
			composition: Composition::ExistingEnvironment(
				SettingsLoadMode::Inspection,
				Box::new(|resources, loaded, _| {
					CommandDependencies::InspectModConflicts(
						resources.inspect_mod_conflicts_dependencies(
							loaded.resolved.effective_binding,
						),
					)
				}),
			),
		},
		Command::Conflicts {
			command: ConflictsCommand::Explain { .. },
		} => Invocation {
			operation_name: "conflicts.explain",
			quiet_success: false,
			exit_status_family: ExitStatusFamily::Ordinary,
			composition: Composition::ExistingEnvironment(
				SettingsLoadMode::Inspection,
				Box::new(|resources, loaded, _| {
					CommandDependencies::ExplainPath(
						resources.explain_path_dependencies(loaded.resolved.effective_binding),
					)
				}),
			),
		},
		Command::Export(_) => Invocation {
			operation_name: "export",
			quiet_success: false,
			exit_status_family: ExitStatusFamily::Ordinary,
			composition: Composition::ExistingEnvironment(
				SettingsLoadMode::Execution,
				Box::new(|resources, loaded, _| {
					CommandDependencies::ExportEnvironment(
						resources.export_environment_dependencies(
							loaded.resolved.effective_binding,
						),
					)
				}),
			),
		},
		Command::Exec(arguments) => {
			let captured_output = arguments.hidden;
			Invocation {
				operation_name: "exec",
				quiet_success: false,
				exit_status_family: ExitStatusFamily::Execution,
				composition: Composition::ExistingEnvironment(
					SettingsLoadMode::Execution,
					Box::new(move |resources, loaded, startup| {
						let binding = loaded.resolved.effective_binding;
						let force_cancellation = CancellationToken::new();
						let execute_program = if captured_output {
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

						CommandDependencies::ExecuteProgram {
							execute_program,
							force_cancellation,
						}
					}),
				),
			}
		}
		Command::Shortcut(_) => Invocation {
			operation_name: "shortcut",
			quiet_success: true,
			exit_status_family: ExitStatusFamily::Ordinary,
			composition: Composition::ExistingEnvironment(
				SettingsLoadMode::Execution,
				Box::new(|resources, loaded, startup| CommandDependencies::CreateShortcut {
					dependencies: resources.create_shortcut_dependencies(
						loaded.resolved.effective_binding.clone(),
						startup.to_owned(),
					),
					settings: loaded.resolved,
				}),
			),
		},
	}
}

pub(crate) async fn compose(
	root: &EnvironmentRoot,
	startup: &Path,
	composition: Composition,
) -> RootResult<CommandDependencies, ErrorMarker> {
	let resources = Resources::system(root.clone());

	match composition {
		Composition::NewEnvironment(compose_dependencies) => Ok(compose_dependencies(&resources)),
		Composition::ExistingEnvironment(settings_load_mode, compose_dependencies) => {
			let loaded = resources
				.load_settings(settings_load_mode, &operation::ctrl_c_token())
				.await?;
			Ok(compose_dependencies(&resources, loaded, startup))
		}
	}
}

#[cfg(test)]
mod tests {
	use super::compose;
	use super::invocation;
	use crate::commands::parse_from;
	use application::ErrorCode;
	use domain::EnvironmentRoot;
	use infrastructure_dependencies::SettingsLoadMode;
	use std::error::Error;
	use std::ffi::OsString;
	use std::fs::create_dir_all;
	use std::fs::write;
	use std::path::Path;
	use tempfile::TempDir;

	async fn composition_failure(arguments: &[&str], root: &Path) -> Result<Option<ErrorCode>, Box<dyn Error>> {
		let cli = parse_from(
			[OsString::from("mods")]
				.into_iter()
				.chain(arguments.iter().map(OsString::from)),
		)?;
		let root = EnvironmentRoot::new(root.to_owned()).map_err(|_| "environment root fixture")?;

		let composed = compose(&root, root.as_path(), invocation(&cli.command).composition).await;

		Ok(composed.err().map(|report| report.current_context().code()))
	}

	#[tokio::test]
	async fn every_command_composes_after_loading_settings_in_its_mode() -> Result<(), Box<dyn Error>> {
		let temp = TempDir::new()?;
		let unvalidated_layout = temp.path().join("unvalidated-layout");
		for directory in ["mods", "profile", "overwrite", "temp"] {
			create_dir_all(unvalidated_layout.join(directory))?;
		}
		write(
			unvalidated_layout.join("mods.toml"),
			format!(
				"schema_version = 1\ngame_dir = '{}'\n",
				temp.path().join("game").display()
			),
		)?;
		let unfinished_operation = temp.path().join("unfinished-operation");
		create_dir_all(unfinished_operation.join("temp"))?;
		write(unfinished_operation.join("temp/pending"), b"")?;

		for (arguments, expected) in [
			(&["init"][..], None),
			(&["config", "list"], Some(SettingsLoadMode::ReadOnly)),
			(&["config", "get", "game-dir"], Some(SettingsLoadMode::ReadOnly)),
			(&["config", "set", "game-dir", "game"], Some(SettingsLoadMode::Mutation)),
			(
				&["install", "archive.zip", "--dry-run"],
				Some(SettingsLoadMode::Inspection),
			),
			(&["install", "archive.zip"], Some(SettingsLoadMode::Execution)),
			(&["conflicts", "list"], Some(SettingsLoadMode::Inspection)),
			(&["conflicts", "inspect", "Visuals"], Some(SettingsLoadMode::Inspection)),
			(
				&["conflicts", "explain", "file.esp"],
				Some(SettingsLoadMode::Inspection),
			),
			(&["export", "payload"], Some(SettingsLoadMode::Execution)),
			(&["exec", "--", "tool.exe"], Some(SettingsLoadMode::Execution)),
			(
				&["exec", "--hidden", "--", "tool.exe"],
				Some(SettingsLoadMode::Execution),
			),
			(&["shortcut", "--", "tool.exe"], Some(SettingsLoadMode::Execution)),
		] {
			let layout_failure = composition_failure(arguments, &unvalidated_layout).await?;
			let unfinished_operation_failure =
				composition_failure(arguments, &unfinished_operation).await?;

			let observed = match (layout_failure, unfinished_operation_failure) {
				(None, None) => None,
				(Some(_), Some(ErrorCode::EnvironmentInvalid)) => Some(SettingsLoadMode::ReadOnly),
				(None, Some(ErrorCode::EnvironmentInvalid)) => Some(SettingsLoadMode::Inspection),
				(Some(_), Some(ErrorCode::ManualCleanupRequired)) => Some(SettingsLoadMode::Mutation),
				(None, Some(ErrorCode::ManualCleanupRequired)) => Some(SettingsLoadMode::Execution),
				failures => {
					return Err(format!("{arguments:?} failed unexpectedly: {failures:?}").into());
				}
			};
			assert_eq!(observed, expected, "{arguments:?}");
		}
		Ok(())
	}
}
