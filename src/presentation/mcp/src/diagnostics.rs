use crate::commands::LogLevel;
use std::future::Future;
use std::path::Path;
use tracing::Dispatch;
use tracing::dispatcher::with_default;
use tracing::instrument::WithSubscriber;
use tracing_appender::rolling::RollingFileAppender;
use tracing_appender::rolling::Rotation;
use tracing_subscriber::Layer;
use tracing_subscriber::Registry;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::filter::Targets;
use tracing_subscriber::fmt;
use tracing_subscriber::layer::SubscriberExt;
use uuid::Uuid;

pub(crate) const SINK_WARNING: &str = "warning: diagnostic session logging is unavailable";

pub(crate) enum SessionStart {
	FileBacked(DiagnosticSession),
	Disabled,
	SetupFailed,
}

pub(crate) struct DiagnosticSession {
	id: Uuid,
	operation: &'static str,
	subscriber: Dispatch,
}

impl DiagnosticSession {
	pub(crate) fn start(root: &Path, level: LogLevel, operation: &'static str) -> SessionStart {
		if level == LogLevel::Off {
			return SessionStart::Disabled;
		}

		let id = Uuid::new_v4();
		let Ok(appender) = RollingFileAppender::builder()
			.rotation(Rotation::NEVER)
			.filename_prefix(format!("{id}.jsonl"))
			.build(root.join("logs"))
		else {
			return SessionStart::SetupFailed;
		};

		let layer = fmt::layer().json().with_writer(appender).with_filter(filter(level));
		let subscriber = Dispatch::new(Registry::default().with(layer));

		with_default(&subscriber, || {
			tracing::info!(
				target: "mods::diagnostics",
				event = "session.started",
				diagnostic_session = %id,
				operation,
				"diagnostic session started"
			);
		});

		SessionStart::FileBacked(Self {
			id,
			operation,
			subscriber,
		})
	}

	pub(crate) fn id(&self) -> Uuid {
		self.id
	}

	pub(crate) async fn capture<F>(&self, future: F) -> F::Output
	where
		F: Future,
	{
		future.with_subscriber(self.subscriber.clone()).await
	}

	pub(crate) fn finish(self, outcome: &'static str) {
		with_default(&self.subscriber, || match outcome {
			"success" => tracing::info!(
				target: "mods::diagnostics",
				event = "session.completed",
				diagnostic_session = %self.id,
				operation = self.operation,
				outcome,
				"diagnostic session completed"
			),
			"cancelled" => tracing::info!(
				target: "mods::diagnostics",
				event = "session.cancelled",
				diagnostic_session = %self.id,
				operation = self.operation,
				outcome,
				"diagnostic session cancelled"
			),
			_ => tracing::error!(
				target: "mods::diagnostics",
				event = "session.failed",
				diagnostic_session = %self.id,
				operation = self.operation,
				outcome,
				"diagnostic session failed"
			),
		});
	}
}

fn filter(level: LogLevel) -> Targets {
	let configured = match level {
		LogLevel::Trace => LevelFilter::TRACE,
		LogLevel::Debug => LevelFilter::DEBUG,
		LogLevel::Info => LevelFilter::INFO,
		LogLevel::Warn => LevelFilter::WARN,
		LogLevel::Error => LevelFilter::ERROR,
		LogLevel::Off => LevelFilter::OFF,
	};

	[
		"mods",
		"application",
		"domain",
		"infrastructure_environment",
		"infrastructure_settings",
		"infrastructure_game_platform",
		"infrastructure_archive",
		"infrastructure_execution",
		"infrastructure_dependencies",
	]
	.into_iter()
	.fold(
		Targets::new()
			.with_default(LevelFilter::WARN)
			.with_target("mods::diagnostics", LevelFilter::TRACE),
		|targets, target| targets.with_target(target, configured),
	)
}

#[cfg(test)]
mod tests {
	use super::DiagnosticSession;
	use super::SessionStart;
	use crate::commands::LogLevel;
	use serde_json::Value;
	use serde_json::from_str;
	use std::error::Error;
	use std::fs::read_dir;
	use std::fs::read_to_string;
	use std::path::Path;
	use tempfile::TempDir;
	use uuid::Uuid;

	fn records(root: &Path) -> Result<(Uuid, Vec<Value>), Box<dyn Error>> {
		let files = read_dir(root.join("logs"))?.collect::<Result<Vec<_>, _>>()?;
		assert_eq!(files.len(), 1);
		let id = Uuid::parse_str(
			files[0].path()
				.file_stem()
				.and_then(|stem| stem.to_str())
				.ok_or("diagnostic file stem")?,
		)?;
		let records = read_to_string(files[0].path())?
			.lines()
			.map(from_str::<Value>)
			.collect::<Result<Vec<_>, _>>()?;

		Ok((id, records))
	}

	#[test]
	fn off_creates_no_log_file_or_root() -> Result<(), Box<dyn Error>> {
		let temp = TempDir::new()?;
		let root = temp.path().join("environment");
		assert!(matches!(
			DiagnosticSession::start(&root, LogLevel::Off, "config.list"),
			SessionStart::Disabled
		));
		assert!(!root.exists());

		Ok(())
	}

	#[test]
	fn normal_session_creates_one_uuid_file_with_boundary_events() -> Result<(), Box<dyn Error>> {
		let temp = TempDir::new()?;
		let SessionStart::FileBacked(session) =
			DiagnosticSession::start(temp.path(), LogLevel::Error, "config.list")
		else {
			return Err("expected file-backed session".into());
		};
		let expected_id = session.id();
		session.finish("success");

		let (id, records) = records(temp.path())?;
		assert_eq!(id, expected_id);
		let events = records
			.iter()
			.filter_map(|record| record.pointer("/fields/event").and_then(Value::as_str))
			.collect::<Vec<_>>();
		assert_eq!(events, ["session.started", "session.completed"]);
		Ok(())
	}

	#[tokio::test]
	async fn configured_log_level_filters_captured_project_events() -> Result<(), Box<dyn Error>> {
		let temp = TempDir::new()?;
		let SessionStart::FileBacked(session) =
			DiagnosticSession::start(temp.path(), LogLevel::Info, "config.list")
		else {
			return Err("expected file-backed session".into());
		};
		session.capture(async {
			tracing::debug!(target: "application::diagnostic_test", event = "project.debug", "debug event");
			tracing::info!(target: "application::diagnostic_test", event = "project.info", "info event");
		})
		.await;
		session.finish("success");

		let (_, records) = records(temp.path())?;
		let events = records
			.iter()
			.filter_map(|record| record.pointer("/fields/event").and_then(Value::as_str))
			.collect::<Vec<_>>();
		assert_eq!(events, ["session.started", "project.info", "session.completed"]);

		Ok(())
	}
}
