//! rmcp's bounded EOF drain does not own application supervision. The tracker
//! keeps admitted work alive through native Job and pipe drain, without extending
//! protocol response eligibility after the transport closes.
use infrastructure::ExecutionCapture;
use rmcp::model::RequestId;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::sync::OwnedSemaphorePermit;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

type RetainedCapture = (Arc<ExecutionCapture>, OwnedSemaphorePermit, CancellationToken);

#[derive(Default)]
pub(crate) struct Lifecycle {
	pub(crate) shutdown: CancellationToken,
	pub(crate) tasks: TaskTracker,
	captures: Mutex<HashMap<RequestId, RetainedCapture>>,
}
impl Lifecycle {
	pub(crate) async fn retain_capture(
		&self,
		id: RequestId,
		capture: Arc<ExecutionCapture>,
		permit: OwnedSemaphorePermit,
	) {
		self.captures
			.lock()
			.await
			.insert(id, (capture, permit, CancellationToken::new()));
	}
	pub(crate) async fn release_response(&self, id: &RequestId) {
		if let Some((_, _, released)) = self.captures.lock().await.remove(id) {
			released.cancel();
		}
	}
	/// Call only after execution, Job cleanup, and pipe drains have completed.
	/// rmcp can discard a cancelled response without calling transport.send.
	pub(crate) async fn execution_finished(self: &Arc<Self>, id: RequestId, cancellation: CancellationToken) {
		let released = self
			.captures
			.lock()
			.await
			.get(&id)
			.map(|(_, _, released)| released.clone());
		let Some(released) = released else {
			return;
		};

		let lifecycle = self.clone();

		self.tasks.spawn(async move {
			tokio::select! {
				() = released.cancelled() => return,
				() = cancellation.cancelled() => {},
				() = lifecycle.shutdown.cancelled() => {},
			}

			lifecycle.release_response(&id).await;
		});
	}

	pub(crate) async fn drain(&self) {
		self.shutdown.cancel();
		self.tasks.close();
		self.tasks.wait().await;

		self.captures.lock().await.clear();
	}
}

#[cfg(test)]
mod tests {
	use super::Lifecycle;
	use infrastructure::ExecutionCapture;
	use rmcp::model::RequestId;
	use rootcause::Result;
	use std::path::Path;
	use std::sync::Arc;
	use tokio::spawn;
	use tokio::sync::Semaphore;
	use tokio::sync::oneshot;
	use tokio::task::yield_now;
	use tokio_util::sync::CancellationToken;

	#[tokio::test]
	async fn response_commit_releases_retained_capture() -> Result<()> {
		let lifecycle = Lifecycle::default();
		let capture = Arc::new(ExecutionCapture::new(Path::new("unused")));
		let weak = Arc::downgrade(&capture);
		let id = RequestId::Number(2);
		let admission = Arc::new(Semaphore::new(1));
		let permit = admission.clone().try_acquire_owned()?;

		lifecycle.retain_capture(id.clone(), capture, permit).await;

		assert!(admission.clone().try_acquire_owned().is_err());
		assert!(weak.upgrade().is_some());

		lifecycle.release_response(&id).await;

		assert!(weak.upgrade().is_none());
		assert!(admission.try_acquire_owned().is_ok());

		Ok(())
	}

	#[tokio::test]
	async fn shutdown_waits_for_cleanup_before_releasing_captures() -> Result<()> {
		let lifecycle = Arc::new(Lifecycle::default());
		let capture = Arc::new(ExecutionCapture::new(Path::new("unused")));
		let weak = Arc::downgrade(&capture);

		lifecycle
			.retain_capture(
				RequestId::Number(2),
				capture,
				Arc::new(Semaphore::new(1)).try_acquire_owned()?,
			)
			.await;
		let (release, cleanup) = oneshot::channel();
		let (observed, observation) = oneshot::channel();
		let shutdown = lifecycle.shutdown.clone();
		lifecycle.tasks.spawn(async move {
			shutdown.cancelled().await;
			let _ = observed.send(());
			let _ = cleanup.await;
		});
		let draining = spawn(async move { lifecycle.drain().await });
		observation.await?;

		assert!(!draining.is_finished());
		assert!(weak.upgrade().is_some());

		release.send(())
			.map_err(|()| rootcause::report!("cleanup receiver dropped"))?;
		draining.await?;

		assert!(weak.upgrade().is_none());

		Ok(())
	}
	#[tokio::test]
	async fn late_cancellation_releases_capture_without_response_or_eof() -> Result<()> {
		let lifecycle = Arc::new(Lifecycle::default());
		let admission = Arc::new(Semaphore::new(1));
		let capture = Arc::new(ExecutionCapture::new(Path::new("unused")));
		let weak = Arc::downgrade(&capture);
		let id = RequestId::Number(3);
		let cancellation = CancellationToken::new();

		lifecycle
			.retain_capture(id.clone(), capture, admission.clone().try_acquire_owned()?)
			.await;

		lifecycle.execution_finished(id, cancellation.clone()).await;

		// The execution and drains have ended, but rmcp will discard this late response.
		cancellation.cancel();
		yield_now().await;

		assert!(admission.try_acquire_owned().is_ok());
		assert!(weak.upgrade().is_none());
		assert!(!lifecycle.shutdown.is_cancelled());

		Ok(())
	}
}
