use tokio::signal::ctrl_c;
use tokio::spawn;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

pub(crate) fn ctrl_c_token() -> CancellationToken {
	let cancellation = CancellationToken::new();
	let signal = cancellation.clone();
	spawn(async move {
		if ctrl_c().await.is_ok() {
			signal.cancel();
		}
	});
	cancellation
}

/// Owns the two-stage console subscription only while an execution is running.
pub(crate) struct ExecutionSignals {
	pub(crate) cancellation: CancellationToken,
	listener: JoinHandle<()>,
}
impl ExecutionSignals {
	pub(crate) fn new(force: CancellationToken) -> Self {
		let cancellation = CancellationToken::new();
		let signal = cancellation.clone();
		let listener = spawn(async move {
			if ctrl_c().await.is_err() {
				return;
			}
			signal.cancel();
			if ctrl_c().await.is_ok() {
				force.cancel();
			}
		});
		Self { cancellation, listener }
	}
}
impl Drop for ExecutionSignals {
	fn drop(&mut self) {
		self.listener.abort();
	}
}
