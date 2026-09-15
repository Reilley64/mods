#![forbid(unsafe_code)]

mod commands;
mod diagnostics;
#[cfg(test)]
mod diagnostics_tests;
mod error;
mod operation;
mod output;
mod runner;

use std::env::args_os;
use std::ffi::OsString;
use std::io::Write;
use std::io::stderr;
use std::io::stdout;
use std::process::exit;

const BUILD_COMMIT: &str = env!("BUILD_COMMIT");

#[tokio::main]
async fn main() {
	let arguments: Vec<OsString> = args_os().collect();
	let result = runner::run_current_process(arguments, |root| {
		let resources = infrastructure::Resources::system(root.clone());
		Ok(runner::Dependencies {
			initialize_environment: resources.initialize_environment_dependencies(),
			list_settings: resources.list_settings_dependencies(),
			get_setting: resources.get_setting_dependencies(),
			set_game_directory: resources.set_game_directory_dependencies(),
		})
	})
	.await;
	let outcome = match result {
		Ok(outcome) => outcome,
		Err(error) => {
			let status = error.exit_code();
			let _ = error.print();
			exit(status);
		}
	};
	let _ = stdout().write_all(outcome.stdout.as_bytes());
	let _ = stderr().write_all(outcome.stderr.as_bytes());
	let _ = stdout().flush();
	let _ = stderr().flush();
	if outcome.status != 0 {
		exit(outcome.status as i32);
	}
	let _ = BUILD_COMMIT;
}
