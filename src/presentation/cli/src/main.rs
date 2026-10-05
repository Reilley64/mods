#![forbid(unsafe_code)]
#![feature(fn_traits)]

mod commands;
mod conflict_output;
mod diagnostics;
mod error;
mod install_warning;
mod json_conflicts;
mod json_install;
mod json_output;
mod json_values;
mod operation;
mod output;
mod path_resolution;
mod publication;
mod runner;

use commands::Command;
use commands::parse_from;
use infrastructure_dependencies::Resources;
#[cfg(windows)]
use infrastructure_dependencies::detach_console;
#[cfg(windows)]
use infrastructure_dependencies::show_error;
use std::env::args_os;
use std::ffi::OsString;
use std::io::Write;
use std::io::stderr;
use std::io::stdout;
use std::process::exit;
use tokio_util::sync::CancellationToken;

const BUILD_COMMIT: &str = env!("BUILD_COMMIT");

#[tokio::main]
async fn main() {
	let arguments: Vec<OsString> = args_os().collect();
	let json_requested = arguments
		.iter()
		.skip(1)
		.take_while(|value| value != &&OsString::from("--"))
		.any(|value| value == "--json");
	let hidden = parse_from(arguments.clone())
		.ok()
		.is_some_and(|cli| matches!(cli.command, Command::Exec(exec) if exec.hidden));

	#[cfg(windows)]
	if hidden && detach_console().is_err() {
		// Detachment failed, so retain ordinary terminal reporting as a fallback.
		let message = "error: unable to detach the launcher console";
		if show_error(message).is_err() {
			eprintln!("{message}; unable to display the error dialog");
		}
		exit(125);
	}

	let result = runner::run_current_process(arguments, |root, startup| {
		let resources = Resources::system(root.clone());
		let install_mod = resources.install_mod_dependencies();
		let execution_force_cancellation = CancellationToken::new();
		let execute_program = if hidden {
			resources
				.captured_execution_dependencies(
					startup.to_owned(),
					execution_force_cancellation.clone(),
				)
				.0
		} else {
			resources.execute_program_dependencies(startup.to_owned(), execution_force_cancellation.clone())
		};

		Ok(runner::Dependencies {
			create_shortcut: resources.create_shortcut_dependencies(startup.to_owned()),
			execute_program,
			execution_force_cancellation,
			initialize_environment: resources.initialize_environment_dependencies(),
			list_settings: resources.list_settings_dependencies(),
			get_setting: resources.get_setting_dependencies(),
			set_game_directory: resources.set_game_directory_dependencies(),
			install_mod,
			list_effective_conflicts: resources.list_effective_conflicts_dependencies(),
			inspect_mod_conflicts: resources.inspect_mod_conflicts_dependencies(),
			explain_path: resources.explain_path_dependencies(),
		})
	})
	.await;
	let outcome = match result {
		Ok(outcome) => outcome,
		Err(error) => {
			#[cfg(windows)]
			if hidden {
				if show_error("error: unable to read startup directory").is_err() {
					// With no attached console and no available dialog, only exit status remains.
					exit(125);
				}
				exit(error.exit_code());
			}

			let status = error.exit_code();
			let stdout = stdout();
			let stderr = stderr();
			let publication_result = if json_requested {
				let mut stdout = stdout.lock();
				let mut stderr = stderr.lock();

				let value = json_output::clap_document(&error);
				let text = json_output::document(&value);

				let stream: &mut dyn Write = if status == 0 { &mut stdout } else { &mut stderr };
				stream.write_all(text.as_bytes())
					.and_then(|_| publication::flush(&mut stdout, &mut stderr))
			} else {
				error.print()
					.and_then(|_| publication::flush(&mut stdout.lock(), &mut stderr.lock()))
			};
			exit(publication::exit_status(status, &publication_result));
		}
	};

	#[cfg(windows)]
	if hidden {
		if let Some(message) = runner::hidden_failure_dialog(&outcome)
			&& show_error(&message).is_err()
		{
			// The execution log is retained, but no interactive reporting channel remains.
			exit(125);
		}

		exit(outcome.status as i32);
	}

	let stdout = stdout();
	let stderr = stderr();
	let mut stdout = stdout.lock();
	let mut stderr = stderr.lock();

	let publication_result = publication::publish(&outcome, &mut stdout, &mut stderr);
	let status = publication::exit_status(outcome.status as i32, &publication_result);
	if status != 0 {
		exit(status);
	}
	let _ = BUILD_COMMIT;
}
