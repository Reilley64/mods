use super::CaptureDrains;
use super::ExecutionCapture;
use crate::InheritedStreams;
use application::ErrorMarker;
use os_pipe::pipe;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use std::os::windows::io::AsHandle;
use std::os::windows::io::BorrowedHandle;
use tokio_util::sync::CancellationToken;

pub struct PrivateStreams {
	streams: Option<InheritedStreams>,
	drains: Option<CaptureDrains>,
}
impl ExecutionCapture {
	pub fn prepare(&self, failure: CancellationToken) -> Result<PrivateStreams, ErrorMarker> {
		let (stdin, stdin_writer) =
			pipe().context(ErrorMarker::execution_supervision_failed().with_phase("launch"))?;
		drop(stdin_writer);
		let (stdout, stdout_writer) =
			pipe().context(ErrorMarker::execution_supervision_failed().with_phase("launch"))?;
		let (stderr, stderr_writer) =
			pipe().context(ErrorMarker::execution_supervision_failed().with_phase("launch"))?;
		let streams = InheritedStreams::duplicate([
			stdin.as_handle(),
			stdout_writer.as_handle(),
			stderr_writer.as_handle(),
		])
		.context(ErrorMarker::execution_supervision_failed().with_phase("launch"))?;

		let drains = self.drain(stdout, stderr, failure)?;

		Ok(PrivateStreams {
			streams: Some(streams),
			drains: Some(drains),
		})
	}
}
impl PrivateStreams {
	pub fn borrowed(&self) -> Option<[BorrowedHandle<'_>; 3]> {
		self.streams.as_ref().map(InheritedStreams::borrowed)
	}
	pub fn close_child_ends(&mut self) {
		self.streams.take();
	}
	pub fn finish(mut self) -> Result<(), ErrorMarker> {
		self.streams.take();

		if let Some(drains) = self.drains.take() {
			drains.finish()?;
		}

		Ok(())
	}
}
impl Drop for PrivateStreams {
	fn drop(&mut self) {
		self.streams.take();
		self.drains.take();
	}
}
