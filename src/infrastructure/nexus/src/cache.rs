use crate::types::NexusRequest;
use application::ErrorMarker;
use application::installation::DownloadedMod;
use application::installation::NexusProvenance;
use domain::ArchivePath;
use domain::EnvironmentRoot;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use rootcause::report;
use serde::Deserialize;
use serde::Serialize;
use std::io::ErrorKind;
#[cfg(windows)]
use std::os::windows::fs::MetadataExt;
use std::path::Component;
use std::path::Path;
use std::path::PathBuf;
use tokio::fs::create_dir;
use tokio::fs::metadata;
use tokio::fs::read_to_string;
use tokio::fs::symlink_metadata;
use tokio_util::sync::CancellationToken;

#[derive(Serialize, Deserialize)]
pub(crate) struct CompletedMetadata {
	pub game_domain: String,
	pub mod_id: u64,
	pub file_id: u64,
	pub file_version: String,
	pub mod_version: String,
	pub mod_name: String,
	pub file_name: String,
	pub archive_size: u64,
}
impl CompletedMetadata {
	pub(crate) fn from_provenance(value: &NexusProvenance, archive_size: u64) -> Self {
		Self {
			game_domain: value.game_domain.clone(),
			mod_id: value.mod_id,
			file_id: value.file_id,
			file_version: value.file_version.clone(),
			mod_version: value.mod_version.clone(),
			mod_name: value.mod_name.clone(),
			file_name: value.file_name.clone(),
			archive_size,
		}
	}
	fn provenance(self) -> NexusProvenance {
		NexusProvenance {
			game_domain: self.game_domain,
			mod_id: self.mod_id,
			file_id: self.file_id,
			file_version: self.file_version,
			mod_version: self.mod_version,
			mod_name: self.mod_name,
			file_name: self.file_name,
		}
	}
}

pub(crate) async fn directory(root: &EnvironmentRoot, create: bool) -> Result<Option<PathBuf>, ErrorMarker> {
	let mut path = PathBuf::new();
	for component in root.as_path().components() {
		path.push(component);
		// A Windows drive prefix is not an absolute root until RootDir is appended.
		if matches!(component, Component::Prefix(_)) {
			continue;
		}
		require_kind(&path, true).await?;
	}
	for name in ["cache", "downloads"] {
		path.push(name);
		if create
			&& let Err(error) = create_dir(&path).await
			&& error.kind() != ErrorKind::AlreadyExists
		{
			return Err(report!(error).context(ErrorMarker::io_failure()));
		}
		if !require_kind(&path, true).await? {
			return Ok(None);
		}
	}
	Ok(Some(path))
}

pub(crate) async fn require_kind(path: &Path, directory: bool) -> Result<bool, ErrorMarker> {
	let metadata = match symlink_metadata(path).await {
		Ok(metadata) => metadata,
		Err(error) if error.kind() == ErrorKind::NotFound => return Ok(false),
		Err(error) => return Err(report!(error).context(ErrorMarker::io_failure())),
	};
	#[cfg(windows)]
	let reparse = metadata.file_attributes() & 0x400 != 0;
	#[cfg(not(windows))]
	let reparse = metadata.file_type().is_symlink();
	if reparse || (directory && !metadata.is_dir()) || (!directory && !metadata.is_file()) {
		return Err(report!(ErrorMarker::environment_root_unsafe()));
	}
	Ok(true)
}

pub(crate) fn identity_name(request: &NexusRequest) -> Result<String, ErrorMarker> {
	let file_id = request
		.file_id
		.filter(|value| *value > 0)
		.ok_or_else(|| report!(ErrorMarker::nexus_source_invalid()))?;
	if request.game_domain != "newvegas" || request.mod_id == 0 {
		return Err(report!(ErrorMarker::nexus_source_invalid()));
	}
	Ok(format!("newvegas-{}-{file_id}", request.mod_id))
}

pub(crate) async fn read(
	root: &EnvironmentRoot,
	request: &NexusRequest,
	cancellation: &CancellationToken,
) -> Result<Option<DownloadedMod>, ErrorMarker> {
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}
	let name = identity_name(request)?;
	let Some(directory) = directory(root, false).await? else {
		return Ok(None);
	};
	let entry = directory.join(name);
	if !require_kind(&entry, true).await? {
		return Ok(None);
	}

	let metadata_path = entry.join("provenance.toml");
	let archive_path = entry.join("archive");
	if !require_kind(&metadata_path, false).await? || !require_kind(&archive_path, false).await? {
		return Ok(None);
	}

	let text = read_to_string(metadata_path).await.context(ErrorMarker::io_failure())?;
	let completed: CompletedMetadata =
		toml::from_str(&text).context(ErrorMarker::environment_invalid(Some("download_cache")))?;
	let archive_size = metadata(&archive_path).await.context(ErrorMarker::io_failure())?.len();
	if completed.game_domain != request.game_domain
		|| completed.mod_id != request.mod_id
		|| Some(completed.file_id) != request.file_id
		|| archive_size != completed.archive_size
	{
		return Err(report!(ErrorMarker::environment_invalid(Some("download_cache"))));
	}
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}

	let provenance = completed.provenance();
	let suggested_name = if provenance.file_name.is_empty() {
		provenance.mod_name.clone()
	} else {
		provenance.file_name.clone()
	};
	Ok(Some(DownloadedMod {
		suggested_name,
		archive: ArchivePath::new(archive_path).context(ErrorMarker::unsafe_archive())?,
		provenance: Some(provenance),
	}))
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "synthetic cache fixture setup")]
mod tests {
	use super::*;
	use application::ErrorCode;
	use std::fs;
	#[cfg(unix)]
	use std::os::unix::fs::symlink;
	use tempfile::TempDir;

	fn request(mod_id: u64, file_id: u64) -> NexusRequest {
		NexusRequest {
			game_domain: "newvegas".into(),
			mod_id,
			file_id: Some(file_id),
		}
	}
	#[cfg(windows)]
	#[tokio::test]
	async fn canonical_windows_root_keeps_verbatim_prefix_until_root_is_complete() {
		let temp = TempDir::new().expect("temporary environment");
		let canonical = temp.path().canonicalize().expect("canonical Windows path");
		assert!(matches!(canonical.components().next(), Some(Component::Prefix(_))));
		let root = EnvironmentRoot::new(canonical).expect("environment root");

		let downloads = directory(&root, true)
			.await
			.expect("canonical cache directory")
			.expect("downloads");

		assert_eq!(downloads, root.as_path().join("cache/downloads"));
		assert!(downloads.is_dir());
		assert!(read(&root, &request(42, 7), &CancellationToken::new())
			.await
			.expect("empty cache")
			.is_none());
	}

	#[tokio::test]
	async fn cache_requires_exact_identity_and_complete_bytes_and_ignores_partial_directories() {
		let temp = TempDir::new().expect("root");
		let root = EnvironmentRoot::new(temp.path().canonicalize().expect("canonical")).expect("root");
		let downloads = directory(&root, true).await.expect("downloads").expect("path");
		fs::create_dir(downloads.join(".partial-unfinished")).expect("partial");
		assert!(read(&root, &request(42, 7), &CancellationToken::new())
			.await
			.expect("read")
			.is_none());
		let entry = downloads.join("newvegas-42-7");
		fs::create_dir(&entry).expect("entry");
		fs::write(entry.join("archive"), b"bytes").expect("archive");
		assert!(read(&root, &request(42, 7), &CancellationToken::new())
			.await
			.expect("incomplete")
			.is_none());
		let provenance = NexusProvenance {
			game_domain: "newvegas".into(),
			mod_id: 42,
			file_id: 7,
			file_version: "1".into(),
			mod_version: "2".into(),
			mod_name: "Page".into(),
			file_name: "File".into(),
		};
		fs::write(
			entry.join("provenance.toml"),
			toml::to_string(&CompletedMetadata::from_provenance(&provenance, 5)).expect("metadata"),
		)
		.expect("write");
		assert!(read(&root, &request(42, 7), &CancellationToken::new())
			.await
			.expect("complete")
			.is_some());
		assert!(read(&root, &request(43, 7), &CancellationToken::new())
			.await
			.expect("other mod")
			.is_none());
		assert!(read(&root, &request(42, 8), &CancellationToken::new())
			.await
			.expect("other file")
			.is_none());
		let assert_corrupt_entry = async || {
			let error = read(&root, &request(42, 7), &CancellationToken::new())
				.await
				.expect_err("corrupt entry");
			assert_eq!(error.current_context().code(), ErrorCode::EnvironmentInvalid);
			assert_eq!(error.current_context().phase(), Some("download_cache"));
		};
		fs::write(entry.join("archive"), b"shortened").expect("truncate");
		assert_corrupt_entry().await;
		fs::write(entry.join("archive"), b"bytes").expect("restore archive");
		fs::write(entry.join("provenance.toml"), "game_domain = '").expect("malformed metadata");
		assert_corrupt_entry().await;
		let other_identity = NexusProvenance {
			mod_id: 43,
			..provenance
		};
		fs::write(
			entry.join("provenance.toml"),
			toml::to_string(&CompletedMetadata::from_provenance(&other_identity, 5)).expect("metadata"),
		)
		.expect("mismatched identity");
		assert_corrupt_entry().await;
		let token = CancellationToken::new();
		token.cancel();
		assert_eq!(
			read(&root, &request(42, 7), &token)
				.await
				.expect_err("cancelled")
				.current_context()
				.code(),
			ErrorCode::OperationCancelled
		);
	}
	#[cfg(unix)]
	#[tokio::test]
	async fn cache_rejects_redirected_download_directory() {
		let temp = TempDir::new().expect("root");
		let outside = TempDir::new().expect("outside");
		let root = EnvironmentRoot::new(temp.path().canonicalize().expect("canonical")).expect("root");
		fs::create_dir(root.as_path().join("cache")).expect("cache");
		symlink(outside.path(), root.as_path().join("cache/downloads")).expect("link");
		assert!(directory(&root, true).await.is_err());
		assert!(read(&root, &request(42, 7), &CancellationToken::new()).await.is_err());
		assert_eq!(fs::read_dir(outside.path()).expect("outside entries").count(), 0);
	}
}
