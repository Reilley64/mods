use application::ErrorMarker;
use application::conflicts::ConflictContentRead;
use domain::Sha256Digest;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use rootcause::report;
use sha2::Digest;
use sha2::Sha256;
use std::io::Error;
use std::time::SystemTime;
use tokio::fs::File;
use tokio::io::AsyncReadExt;
use tokio_util::sync::CancellationToken;

const READ_CHUNK_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ContentFingerprint {
	length: u64,
	modified: Option<SystemTime>,
}

pub(crate) async fn sha256(
	mut file: File,
	cancellation: &CancellationToken,
) -> Result<ConflictContentRead, ErrorMarker> {
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}

	let Ok(before) = fingerprint(&file).await else {
		return Ok(ConflictContentRead::Unavailable);
	};

	let mut hasher = Sha256::new();
	let mut buffer = [0_u8; READ_CHUNK_BYTES];
	loop {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}

		let Ok(read) = file.read(&mut buffer).await else {
			return Ok(ConflictContentRead::Unavailable);
		};
		if read == 0 {
			break;
		}
		hasher.update(&buffer[..read]);
	}

	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}

	let Ok(after) = fingerprint(&file).await else {
		return Ok(ConflictContentRead::Unavailable);
	};
	if before != after {
		return Ok(ConflictContentRead::Unstable);
	}

	let digest = hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect();
	let digest = Sha256Digest::new(digest).context(ErrorMarker::io_failure())?;
	Ok(ConflictContentRead::Sha256(digest))
}

async fn fingerprint(file: &File) -> Result<ContentFingerprint, Error> {
	let metadata = file.metadata().await?;
	Ok(ContentFingerprint {
		length: metadata.len(),
		modified: metadata.modified().ok(),
	})
}

#[cfg(test)]
#[expect(
	clippy::expect_used,
	reason = "fixture construction and digest assertions require known-success values"
)]
mod tests {
	use super::sha256;
	use application::conflicts::ConflictContentRead;
	use std::env::current_dir;
	use std::error::Error;
	use tempfile::TempDir;
	use tokio_util::sync::CancellationToken;

	#[tokio::test]
	async fn hashes_the_complete_file_with_sha256() -> Result<(), Box<dyn Error>> {
		let temp = TempDir::new_in(current_dir().expect("current directory")).expect("temporary directory");
		std::fs::write(temp.path().join("content"), b"abc").expect("content must write");
		let file = tokio::fs::File::open(temp.path().join("content"))
			.await
			.expect("content must open");

		let result = sha256(file, &CancellationToken::new())
			.await
			.expect("content must hash");

		let ConflictContentRead::Sha256(digest) = result else {
			return Err("stable content must produce a digest".into());
		};
		assert_eq!(
			digest.as_str(),
			"ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
		);
		Ok(())
	}
}
