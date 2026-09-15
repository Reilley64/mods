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
		let appender = match RollingFileAppender::builder()
			.rotation(Rotation::NEVER)
			.filename_prefix(format!("{id}.jsonl"))
			.build(root.join("logs"))
		{
			Ok(appender) => appender,
			Err(_) => return SessionStart::SetupFailed,
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
		"environment",
		"settings",
		"game_platform",
		"archive",
		"execution",
		"infrastructure",
	]
	.into_iter()
	.fold(
		Targets::new()
			.with_default(LevelFilter::WARN)
			.with_target("mods::diagnostics", LevelFilter::TRACE),
		|targets, target| targets.with_target(target, configured),
	)
}
