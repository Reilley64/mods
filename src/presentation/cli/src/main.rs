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
mod operation;
mod output;
mod path_resolution;
mod publication;
mod runner;

use infrastructure_dependencies::Resources;
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
	let result = runner::run_current_process(arguments, |root, startup| {
		let resources = Resources::system(root.clone());
		let install_archive = resources.install_archive_dependencies();
		let execution_force_cancellation = CancellationToken::new();
		Ok(runner::Dependencies {
			execute_program: resources
				.execute_program_dependencies(startup.to_owned(), execution_force_cancellation.clone()),
			execution_force_cancellation,
			initialize_environment: resources.initialize_environment_dependencies(),
			list_settings: resources.list_settings_dependencies(),
			get_setting: resources.get_setting_dependencies(),
			set_game_directory: resources.set_game_directory_dependencies(),
			install_archive,
			list_effective_conflicts: resources.list_effective_conflicts_dependencies(),
			inspect_mod_conflicts: resources.inspect_mod_conflicts_dependencies(),
			explain_path: resources.explain_path_dependencies(),
		})
	})
	.await;
	let outcome = match result {
		Ok(outcome) => outcome,
		Err(error) => {
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
