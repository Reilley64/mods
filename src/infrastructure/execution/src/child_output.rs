//! Private output spools outlive pipe drains and remain owned until response commitment.
//! Pipes and threads are supplied by os_pipe and std; this adapter owns only the
//! project rule that a failed spool cancels execution while both pipes still drain.
use application::ErrorMarker;
use os_pipe::PipeReader;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use rootcause::report;
use sha2::Digest;
use sha2::Sha256;
use std::fs::File;
use std::fs::create_dir_all;
use std::fs::read;
use std::io::Read;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::thread::JoinHandle;
use std::thread::spawn;
use tempfile::Builder;
use tempfile::TempDir;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::PrivateStreams;

pub struct ExecutionCapture {
	directory: OnceLock<TempDir>,
	temporary_root: PathBuf,
}
pub struct CapturedStream {
	pub byte_count: u64,
	pub sha256: String,
	pub text: Option<String>,
}
pub struct CapturedOutput {
	pub stdout: CapturedStream,
	pub stderr: CapturedStream,
}

impl ExecutionCapture {
	pub fn new(temporary_root: &Path) -> Self {
		Self {
			directory: OnceLock::new(),
			temporary_root: temporary_root.to_owned(),
		}
	}

	/// Keeps the spool owner borrowed so readiness checks cannot exempt an arbitrary path.
	pub fn directory(&self) -> Option<&TempDir> {
		self.directory.get()
	}

	pub fn read(&self) -> Result<CapturedOutput, ErrorMarker> {
		let directory = self
			.directory
			.get()
			.ok_or_else(|| report!(ErrorMarker::execution_supervision_failed().with_phase("cleanup")))?;

		let mut streams = Vec::new();
		for name in ["stdout", "stderr"] {
			let bytes = read(directory.path().join(name))
				.context(ErrorMarker::execution_supervision_failed().with_phase("cleanup"))?;
			streams.push(CapturedStream {
				byte_count: bytes.len() as u64,
				sha256: Sha256::digest(&bytes)
					.iter()
					.map(|byte| format!("{byte:02x}"))
					.collect(),
				text: String::from_utf8(bytes).ok(),
			});
		}

		let mut streams = streams.into_iter();
		let stdout = streams
			.next()
			.ok_or_else(|| report!(ErrorMarker::execution_supervision_failed().with_phase("cleanup")))?;
		let stderr = streams
			.next()
			.ok_or_else(|| report!(ErrorMarker::execution_supervision_failed().with_phase("cleanup")))?;

		Ok(CapturedOutput { stdout, stderr })
	}

	pub fn drain(
		&self,
		stdout: PipeReader,
		stderr: PipeReader,
		failure: CancellationToken,
	) -> Result<CaptureDrains, ErrorMarker> {
		create_dir_all(&self.temporary_root)
			.context(ErrorMarker::execution_supervision_failed().with_phase("launch"))?;
		let spool = Builder::new()
			.prefix(&Uuid::new_v4().to_string())
			.rand_bytes(0)
			.tempdir_in(&self.temporary_root)
			.context(ErrorMarker::execution_supervision_failed().with_phase("launch"))?;
		let spool_path = spool.path().to_owned();
		self.directory
			.set(spool)
			.map_err(|_| report!(ErrorMarker::execution_supervision_failed().with_phase("launch")))?;

		let stdout_file = File::create(spool_path.join("stdout"))
			.context(ErrorMarker::execution_supervision_failed().with_phase("launch"))?;
		let stderr_file = File::create(spool_path.join("stderr"))
			.context(ErrorMarker::execution_supervision_failed().with_phase("launch"))?;

		let mut threads = Vec::new();
		for (mut reader, mut file) in [(stdout, stdout_file), (stderr, stderr_file)] {
			let failure = failure.clone();
			threads.push(spawn(move || {
				let mut failed = None;
				let mut buffer = [0u8; 8192];
				loop {
					let count = match reader.read(&mut buffer) {
						Ok(0) => break,
						Ok(count) => count,
						Err(cause) => {
							failure.cancel();
							return Err(report!(cause).context(
								ErrorMarker::execution_supervision_failed()
									.with_phase("running"),
							));
						}
					};

					if failed.is_none()
						&& let Err(cause) = file.write_all(&buffer[..count])
					{
						failure.cancel();
						failed = Some(report!(cause).context(
							ErrorMarker::execution_supervision_failed()
								.with_phase("running"),
						));
					}
				}

				if let Some(failed) = failed {
					return Err(failed);
				}

				if let Err(cause) = file.flush() {
					failure.cancel();
					return Err(report!(cause).context(
						ErrorMarker::execution_supervision_failed().with_phase("running"),
					));
				}

				Ok(())
			}));
		}

		Ok(CaptureDrains { threads })
	}
}

pub struct CaptureDrains {
	threads: Vec<JoinHandle<Result<(), ErrorMarker>>>,
}
impl CaptureDrains {
	pub fn finish(mut self) -> Result<(), ErrorMarker> {
		let mut failure = None;
		for thread in self.threads.drain(..) {
			let result = thread
				.join()
				.map_err(|_| report!(ErrorMarker::execution_supervision_failed().with_phase("running")))
				.and_then(|result| result);
			if let Err(report) = result {
				failure = Some(report);
			}
		}

		if let Some(failure) = failure {
			return Err(failure);
		}

		Ok(())
	}
}
impl Drop for CaptureDrains {
	fn drop(&mut self) {
		for thread in self.threads.drain(..) {
			let _ = thread.join();
		}
	}
}

#[cfg(test)]
mod tests {
	use super::ExecutionCapture;
	use os_pipe::pipe;
	use rootcause::Result;
	use std::io::Error;
	use std::io::Write;
	use std::thread::spawn;
	use tempfile::TempDir;
	use tokio_util::sync::CancellationToken;

	#[test]
	fn capture_is_lazy_and_empty_streams_are_complete() -> Result<()> {
		let root = TempDir::new()?;
		let pending = root.path().join("temp");
		let capture = ExecutionCapture::new(&pending);
		assert!(!pending.exists());
		let (stdout, stdout_writer) = pipe()?;
		let (stderr, stderr_writer) = pipe()?;
		drop(stdout_writer);
		drop(stderr_writer);

		capture.drain(stdout, stderr, CancellationToken::new())?.finish()?;
		let output = capture.read()?;

		for stream in [output.stdout, output.stderr] {
			assert_eq!(stream.byte_count, 0);
			assert_eq!(stream.text.as_deref(), Some(""));
			assert_eq!(
				stream.sha256,
				"e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
			);
		}
		drop(capture);
		assert_eq!(pending.read_dir()?.count(), 0);
		Ok(())
	}

	#[test]
	fn captures_complete_utf8_and_binary_concurrently() -> Result<()> {
		let root = TempDir::new()?;
		let capture = ExecutionCapture::new(root.path());
		let (out_read, mut out_write) = pipe()?;
		let (err_read, mut err_write) = pipe()?;
		let drains = capture.drain(out_read, err_read, CancellationToken::new())?;
		let out = spawn(move || {
			for byte in "🦀".as_bytes() {
				out_write.write_all(&[*byte])?;
			}
			out_write.write_all(&vec![b'x'; 200_000])
		});
		let err = spawn(move || err_write.write_all(&vec![255; 200_000]));
		out.join().map_err(|_| Error::other("writer panicked"))??;
		err.join().map_err(|_| Error::other("writer panicked"))??;
		drains.finish()?;

		let output = capture.read()?;
		assert_eq!(output.stdout.byte_count, 200_004);
		assert!(output.stdout.text.as_deref().is_some_and(|text| text.starts_with("🦀")));
		assert_eq!(output.stderr.byte_count, 200_000);
		assert!(output.stderr.text.is_none());
		assert_eq!(output.stderr.sha256.len(), 64);
		Ok(())
	}
}
