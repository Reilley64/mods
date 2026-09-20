use crate::safe_fs::READ_CHUNK_BYTES;
use crate::safe_fs::SafeFile;
use application::ErrorMarker;
use application::conflicts::ConflictContentRead;
use cap_std::time::SystemTime;
use domain::Sha256Digest;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use rootcause::report;
use sha2::Digest;
use sha2::Sha256;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ContentFingerprint {
	length: u64,
	modified: Option<SystemTime>,
}

pub(crate) fn sha256(mut file: SafeFile, cancellation: &CancellationToken) -> Result<ConflictContentRead, ErrorMarker> {
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}
	let Ok(before) = fingerprint(&file) else {
		return Ok(ConflictContentRead::Unavailable);
	};

	let mut hasher = Sha256::new();
	let mut buffer = [0_u8; READ_CHUNK_BYTES];
	loop {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}
		let Ok(read) = file.read_chunk(&mut buffer) else {
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
	let Ok(after) = fingerprint(&file) else {
		return Ok(ConflictContentRead::Unavailable);
	};
	if before != after {
		return Ok(ConflictContentRead::Unstable);
	}

	let digest = hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect();
	let digest = Sha256Digest::new(digest).context(ErrorMarker::io_failure())?;
	Ok(ConflictContentRead::Sha256(digest))
}

fn fingerprint(file: &SafeFile) -> Result<ContentFingerprint, std::io::Error> {
	let metadata = file.metadata()?;
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
	use crate::safe_fs::SafeDir;
	use application::conflicts::ConflictContentRead;
	use std::env::current_dir;
	use std::error::Error;
	use tempfile::TempDir;
	use tokio_util::sync::CancellationToken;

	#[test]
	fn hashes_the_complete_file_with_sha256() -> Result<(), Box<dyn Error>> {
		let temp = TempDir::new_in(current_dir().expect("current directory")).expect("temporary directory");
		let root = SafeDir::open_absolute(temp.path()).expect("temporary directory must open");
		root.write_new("content", b"abc").expect("content must write");
		let file = root.open_regular("content").expect("content must open");

		let result = sha256(file, &CancellationToken::new()).expect("content must hash");

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
