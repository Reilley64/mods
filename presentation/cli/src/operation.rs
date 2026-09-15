use tokio::signal::ctrl_c;
use tokio::spawn;
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
