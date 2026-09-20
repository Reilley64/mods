#![forbid(unsafe_code)]

mod commands;
mod conflict_output;
mod diagnostics;
mod error;
mod install_warning;
mod operation;
mod output;
mod path_resolution;
mod publication;
mod runner;

use infrastructure::Resources;
use std::env::args_os;
use std::ffi::OsString;
use std::io::stderr;
use std::io::stdout;
use std::process::exit;

const BUILD_COMMIT: &str = env!("BUILD_COMMIT");

#[tokio::main]
async fn main() {
	let arguments: Vec<OsString> = args_os().collect();
	let result = runner::run_current_process(arguments, |root| {
		let resources = Resources::system(root.clone());
		let install_archive = resources.install_archive_dependencies();
		Ok(runner::Dependencies {
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
			let print_result = error.print();
			let stdout = stdout();
			let stderr = stderr();
			let flush_result = publication::flush(&mut stdout.lock(), &mut stderr.lock());
			let publication_result = print_result.and(flush_result);
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
