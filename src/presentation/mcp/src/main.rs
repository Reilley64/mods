#![forbid(unsafe_code)]

mod cancellation;
mod commands;
mod conflict_output;
mod contract;
mod diagnostics;
mod error_output;
mod execution_output;
mod inputs;
mod install_output;
mod lifecycle;
mod server;
mod settings_output;

mod path_resolution;

use cancellation::io_transport;
use clap::Parser;
use commands::Cli;
use diagnostics::DiagnosticSession;
use diagnostics::SINK_WARNING;
use diagnostics::SessionStart;
use domain::EnvironmentRoot;
use infrastructure_dependencies::Resources;
use path_resolution::resolve_path;
use rmcp::ServiceExt;
use rmcp::transport::stdio;
use rootcause::prelude::ResultExt;
use server::Server;
use std::env::current_dir;
use std::env::var_os;
use std::io;
use std::path::PathBuf;
use std::process::exit;

#[tokio::main]
async fn main() {
	let cli = Cli::parse();

	if run(cli).await.is_err() {
		eprintln!("error: MCP server failed");
		exit(1);
	}
}

async fn run(cli: Cli) -> rootcause::Result<()> {
	let startup = current_dir().into_report()?;

	let path = match cli.environment {
		Some(path) => resolve_path(&path, &startup),
		None => PathBuf::from(
			var_os("LOCALAPPDATA").ok_or_else(|| io::Error::other("LOCALAPPDATA is unavailable"))?,
		)
		.join("mods/environments/default"),
	};

	let root = EnvironmentRoot::new(path)?;

	let session = DiagnosticSession::start(root.as_path(), cli.log_level, "mcp.lifecycle");
	let server = Server::new(Resources::system(root.clone()), root, startup, cli.log_level)?;
	let lifecycle = server.lifecycle();
	let (reader, writer) = stdio();
	let transport = io_transport(reader, writer, lifecycle.clone());
	let operation = async move {
		let result = match server.serve(transport).await {
			Ok(server) => server
				.waiting()
				.await
				.into_report()
				.map_err(|report| report.into_dynamic())
				.map(|_| ()),
			Err(report) => Err(report).into_report().map_err(|report| report.into_dynamic()),
		};

		lifecycle.drain().await;

		result
	};

	match session {
		SessionStart::Disabled => operation.await,
		SessionStart::SetupFailed => {
			eprintln!("{SINK_WARNING}");
			operation.await
		}
		SessionStart::FileBacked(session) => {
			let result = session.capture(operation).await;

			session.finish(if result.is_ok() { "success" } else { "failure" });

			result
		}
	}
}
