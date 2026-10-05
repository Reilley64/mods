use crate::cache;
use crate::cache::CompletedMetadata;
use crate::types::NexusApiKey;
use crate::types::NexusFile;
use crate::types::NexusMod;
use crate::types::NexusRequest;
use application::ErrorMarker;
use application::installation::DownloadedMod;
use application::installation::NexusProvenance;
use domain::ArchivePath;
use domain::EnvironmentRoot;
use reqwest::Client;
use reqwest::Response;
use reqwest::StatusCode;
use reqwest::Url;
use reqwest::header::HeaderValue;
use reqwest::redirect::Policy;
use reqwest::retry::never;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use rootcause::report;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use std::time::Duration;
use tempfile::Builder;
use tokio::fs::File;
use tokio::fs::rename;
use tokio::fs::write;
use tokio::io::AsyncWriteExt;
use tokio_util::sync::CancellationToken;

const API: &str = "https://api.nexusmods.com/v1";

struct NexusHttp {
	client: Client,
	api: String,
}
impl NexusHttp {
	fn system() -> Result<Self, ErrorMarker> {
		let client = Client::builder()
			.https_only(true)
			.retry(never())
			.redirect(Policy::none())
			.connect_timeout(Duration::from_secs(30))
			.timeout(Duration::from_secs(3600))
			.user_agent(concat!("mods/", env!("CARGO_PKG_VERSION")))
			.build()
			.context(ErrorMarker::nexus_network_failure())?;
		Ok(Self {
			client,
			api: API.into(),
		})
	}
}

fn status(response: Response) -> Result<Response, ErrorMarker> {
	let code = response.status();
	if code.is_success() {
		return Ok(response);
	}
	let marker = match code {
		StatusCode::UNAUTHORIZED => ErrorMarker::nexus_credentials_invalid(),
		StatusCode::FORBIDDEN => ErrorMarker::nexus_access_denied(),
		StatusCode::TOO_MANY_REQUESTS => ErrorMarker::nexus_rate_limited(),
		StatusCode::NOT_FOUND | StatusCode::GONE => ErrorMarker::nexus_unavailable(),
		_ => ErrorMarker::nexus_network_failure(),
	};
	Err(report!(marker))
}

async fn get<T: DeserializeOwned>(
	http: &NexusHttp,
	path: &str,
	key: &NexusApiKey,
	cancellation: &CancellationToken,
) -> Result<T, ErrorMarker> {
	let mut header = HeaderValue::from_str(key.expose_secret()).context(ErrorMarker::nexus_credentials_invalid())?;
	header.set_sensitive(true);
	let request = http
		.client
		.get(format!("{}/{path}", http.api))
		.header("apikey", header)
		.header("Application-Name", "mods")
		.header("Application-Version", env!("CARGO_PKG_VERSION"));
	let response = tokio::select! {
	 biased;
	 _ = cancellation.cancelled() => return Err(report!(ErrorMarker::operation_cancelled())),
	 response = request.send() => response.map_err(|error| report!(error.without_url()).context(ErrorMarker::nexus_network_failure()))?,
	};
	let response = status(response)?;
	tokio::select! {
	 biased;
	 _ = cancellation.cancelled() => Err(report!(ErrorMarker::operation_cancelled())),
	 result = response.json::<T>() => result.map_err(|error| report!(error.without_url()).context(ErrorMarker::nexus_response_invalid())),
	}
}

#[derive(Deserialize)]
struct ModResponse {
	name: Option<String>,
	version: Option<String>,
	available: bool,
}
#[derive(Deserialize)]
struct FilesResponse {
	files: Vec<FileResponse>,
}
#[derive(Deserialize)]
struct FileResponse {
	file_id: u64,
	name: Option<String>,
	version: Option<String>,
	category_id: Option<u64>,
	category_name: Option<String>,
}
#[derive(Deserialize)]
struct UserResponse {
	is_premium: bool,
}
#[derive(Deserialize)]
struct DownloadLink {
	#[serde(rename = "URI")]
	uri: String,
}

async fn resolve_with(
	http: &NexusHttp,
	request: NexusRequest,
	key: NexusApiKey,
	cancellation: CancellationToken,
) -> Result<NexusMod, ErrorMarker> {
	let path = format!("games/{}/mods/{}", request.game_domain, request.mod_id);
	let metadata: ModResponse = get(http, &format!("{path}.json"), &key, &cancellation).await?;
	if !metadata.available {
		return Err(report!(ErrorMarker::nexus_unavailable()));
	}

	let files: FilesResponse = get(http, &format!("{path}/files.json"), &key, &cancellation).await?;

	Ok(NexusMod {
		name: metadata.name.unwrap_or_default(),
		version: metadata.version.unwrap_or_default(),
		files: files
			.files
			.into_iter()
			.map(|file| NexusFile {
				file_id: file.file_id,
				name: file.name.unwrap_or_default(),
				version: file.version.unwrap_or_default(),
				available: matches!(file.category_id, Some(1..=5)),
				main: file.category_id == Some(1),
				category: file.category_name.unwrap_or_else(|| "Unavailable".into()),
			})
			.collect(),
	})
}

async fn download_with(
	http: &NexusHttp,
	root: EnvironmentRoot,
	provenance: NexusProvenance,
	key: NexusApiKey,
	cancellation: CancellationToken,
) -> Result<DownloadedMod, ErrorMarker> {
	let user: UserResponse = get(http, "users/validate.json", &key, &cancellation).await?;
	if !user.is_premium {
		return Err(report!(ErrorMarker::nexus_premium_required()));
	}

	let links: Vec<DownloadLink> = get(
		http,
		&format!(
			"games/{}/mods/{}/files/{}/download_link.json",
			provenance.game_domain, provenance.mod_id, provenance.file_id
		),
		&key,
		&cancellation,
	)
	.await?;
	let link = links
		.into_iter()
		.next()
		.ok_or_else(|| report!(ErrorMarker::nexus_unavailable()))?;
	// Download URLs carry account-bound tokens. Never put them in errors or provenance.
	let url = Url::parse(&link.uri).context(ErrorMarker::nexus_response_invalid())?;
	if (url.scheme() != "https" && !http.api.starts_with("http://127.0.0.1:"))
		|| !url.username().is_empty()
		|| url.password().is_some()
	{
		return Err(report!(ErrorMarker::nexus_response_invalid()));
	}

	let directory = cache::directory(&root, true)
		.await?
		.ok_or_else(|| report!(ErrorMarker::io_failure()))?;
	let request = NexusRequest {
		game_domain: provenance.game_domain.clone(),
		mod_id: provenance.mod_id,
		file_id: Some(provenance.file_id),
	};
	let destination = directory.join(cache::identity_name(&request)?);
	// TempDir removes incomplete transfers on errors, cancellation, or a dropped future.
	// The issue's acquisition cleanup contract differs from installation transaction recovery.
	let temporary = Builder::new()
		.prefix(".partial-")
		.tempdir_in(&directory)
		.context(ErrorMarker::io_failure())?;
	let archive_path = temporary.path().join("archive");
	let mut file = File::create(&archive_path).await.context(ErrorMarker::io_failure())?;

	let transfer =
		async {
			// This request has no API key header; credentials never follow a CDN link.
			let response = http.client.get(url).send().await.map_err(|error| {
				report!(error.without_url()).context(ErrorMarker::nexus_network_failure())
			})?;
			let mut response = status(response)?;
			let expected_size = response.content_length();
			let mut size = 0_u64;
			while let Some(chunk) = response.chunk().await.map_err(|error| {
				report!(error.without_url()).context(ErrorMarker::nexus_network_failure())
			})? {
				file.write_all(&chunk).await.context(ErrorMarker::io_failure())?;
				size += chunk.len() as u64;
			}
			if size == 0 || expected_size.is_some_and(|expected| expected != size) {
				return Err(report!(ErrorMarker::nexus_network_failure()));
			}
			file.sync_all().await.context(ErrorMarker::io_failure())?;
			Ok(size)
		};

	let transferred = tokio::select! {
	 biased;
	 _ = cancellation.cancelled() => Err(report!(ErrorMarker::operation_cancelled())),
	 result = transfer => result,
	};
	drop(file.into_std().await);
	if let Err(error) = transferred {
		temporary.close().context(ErrorMarker::io_failure())?;
		return Err(error);
	}
	let size = transferred?;

	let metadata = toml::to_string(&CompletedMetadata::from_provenance(&provenance, size))
		.context(ErrorMarker::io_failure())?;
	write(temporary.path().join("provenance.toml"), metadata)
		.await
		.context(ErrorMarker::io_failure())?;
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}
	rename(temporary.path(), &destination)
		.await
		.context(ErrorMarker::io_failure())?;

	let suggested_name = if provenance.file_name.is_empty() {
		provenance.mod_name.clone()
	} else {
		provenance.file_name.clone()
	};
	Ok(DownloadedMod {
		suggested_name,
		archive: ArchivePath::new(destination.join("archive")).context(ErrorMarker::unsafe_archive())?,
		provenance: Some(provenance),
	})
}

pub(crate) async fn resolve(
	request: NexusRequest,
	key: NexusApiKey,
	cancellation: CancellationToken,
) -> Result<NexusMod, ErrorMarker> {
	resolve_with(&NexusHttp::system()?, request, key, cancellation).await
}
pub(crate) async fn download(
	root: EnvironmentRoot,
	provenance: NexusProvenance,
	key: NexusApiKey,
	cancellation: CancellationToken,
) -> Result<DownloadedMod, ErrorMarker> {
	download_with(&NexusHttp::system()?, root, provenance, key, cancellation).await
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "synthetic HTTP fixture assertions")]
mod tests {
	use super::*;
	use application::ErrorCode;
	use std::fs;
	use std::sync::Arc;
	use std::sync::Mutex;
	use tempfile::TempDir;
	use tokio::io::AsyncReadExt;
	use tokio::net::TcpListener;
	use tokio::task::JoinHandle;

	struct Reply {
		code: u16,
		body: String,
		length: Option<usize>,
		cancel: bool,
	}
	impl Reply {
		fn json(body: &str) -> Self {
			Self {
				code: 200,
				body: body.into(),
				length: None,
				cancel: false,
			}
		}
	}
	async fn server(
		replies: Vec<Reply>,
		cancellation: CancellationToken,
	) -> (NexusHttp, Arc<Mutex<Vec<String>>>, JoinHandle<()>) {
		let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
		let address = listener.local_addr().expect("address");
		let api = format!("http://{address}");
		let base = api.clone();
		let requests = Arc::new(Mutex::new(Vec::new()));
		let recorded = requests.clone();
		let handle = tokio::spawn(async move {
			for reply in replies {
				let (mut socket, _) = listener.accept().await.expect("accept");
				let mut request = Vec::new();
				while !request.ends_with(b"\r\n\r\n") {
					let mut byte = [0];
					if socket.read(&mut byte).await.expect("request") == 0 {
						break;
					}
					request.push(byte[0]);
				}
				recorded.lock()
					.expect("requests")
					.push(String::from_utf8(request).expect("utf8"));
				let body = reply.body.replace("BASE", &base);
				let response = format!(
					"HTTP/1.1 {} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
					reply.code,
					reply.length.unwrap_or(body.len()),
					body
				);
				socket.write_all(response.as_bytes()).await.expect("response");
				if reply.cancel {
					cancellation.cancel();
				}
			}
		});
		let http = NexusHttp {
			client: Client::builder()
				.retry(never())
				.redirect(Policy::none())
				.build()
				.expect("client"),
			api,
		};
		(http, requests, handle)
	}
	fn root() -> (TempDir, EnvironmentRoot) {
		let directory = TempDir::new().expect("temporary root");
		let root =
			EnvironmentRoot::new(directory.path().canonicalize().expect("canonical root")).expect("root");
		(directory, root)
	}
	fn provenance() -> NexusProvenance {
		NexusProvenance {
			game_domain: "newvegas".into(),
			mod_id: 42,
			file_id: 7,
			file_version: "01-beta".into(),
			mod_version: "2".into(),
			mod_name: "Page".into(),
			file_name: "File".into(),
		}
	}
	fn key() -> NexusApiKey {
		NexusApiKey::new("synthetic-api-key".into())
	}
	fn download_replies(last: Reply) -> Vec<Reply> {
		vec![
			Reply::json(r#"{"is_premium":true}"#),
			Reply::json(r#"[{"URI":"BASE/archive?token=synthetic-temporary-token"}]"#),
			last,
		]
	}
	#[tokio::test]
	async fn premium_transfer_publishes_exact_cache_and_never_sends_key_to_archive_host() {
		let (directory, root) = root();
		let (http, requests, server) =
			server(download_replies(Reply::json("archive bytes")), CancellationToken::new()).await;
		let result = download_with(&http, root.clone(), provenance(), key(), CancellationToken::new())
			.await
			.expect("download");
		server.await.expect("server");
		assert_eq!(fs::read(result.archive.as_path()).expect("bytes"), b"archive bytes");
		let request = NexusRequest {
			game_domain: "newvegas".into(),
			mod_id: 42,
			file_id: Some(7),
		};
		let cached = cache::read(&root, &request, &CancellationToken::new())
			.await
			.expect("read")
			.expect("cache");
		assert_eq!(cached.provenance, Some(provenance()));
		let requests = requests.lock().expect("requests");
		assert!(requests[0].contains("apikey: synthetic-api-key"));
		assert!(!requests[2].contains("apikey"));
		assert_eq!(
			fs::read_dir(directory.path().join("cache/downloads"))
				.expect("entries")
				.count(),
			1
		);
		let metadata =
			fs::read_to_string(directory.path().join("cache/downloads/newvegas-42-7/provenance.toml"))
				.expect("metadata");
		assert!(!metadata.contains("synthetic"));
	}
	#[tokio::test]
	async fn invalid_rate_limited_and_non_premium_accounts_have_distinct_errors_without_transfer() {
		for (code, body, expected) in [
			(401, "secret response", ErrorCode::NexusCredentialsInvalid),
			(429, "limited", ErrorCode::NexusRateLimited),
			(200, r#"{"is_premium":false}"#, ErrorCode::NexusPremiumRequired),
			(500, "down", ErrorCode::NexusNetworkFailure),
		] {
			let (_directory, root) = root();
			let (http, requests, server) = server(
				vec![Reply {
					code,
					body: body.into(),
					length: None,
					cancel: false,
				}],
				CancellationToken::new(),
			)
			.await;
			let error = download_with(&http, root.clone(), provenance(), key(), CancellationToken::new())
				.await
				.expect_err("denied");
			server.await.expect("server");
			assert_eq!(error.current_context().code(), expected);
			assert_eq!(requests.lock().expect("requests").len(), 1);
			assert!(!root.as_path().join("cache/downloads").exists());
			assert!(!format!("{error:?}").contains("synthetic-api-key"));
		}
	}
	#[tokio::test]
	async fn truncated_and_cancelled_transfers_remove_all_partial_bytes() {
		for cancel in [false, true] {
			let (_directory, root) = root();
			let token = CancellationToken::new();
			let last = Reply {
				code: 200,
				body: "incomplete".into(),
				length: Some(1000),
				cancel,
			};
			let (http, requests, server) = server(download_replies(last), token.clone()).await;
			let error = download_with(&http, root.clone(), provenance(), key(), token)
				.await
				.expect_err("incomplete");
			server.await.expect("server");
			let expected = if cancel {
				ErrorCode::OperationCancelled
			} else {
				ErrorCode::NexusNetworkFailure
			};
			assert_eq!(error.current_context().code(), expected);
			assert_eq!(requests.lock().expect("requests").len(), 3);
			assert_eq!(
				fs::read_dir(root.as_path().join("cache/downloads"))
					.expect("entries")
					.count(),
				0
			);
			assert!(!format!("{error:?}").contains("synthetic-temporary-token"));
		}
	}
	#[tokio::test]
	async fn metadata_handles_hidden_mods_and_optional_file_fields() {
		let request = NexusRequest {
			game_domain: "newvegas".into(),
			mod_id: 42,
			file_id: None,
		};
		let (http, _, task) =
			server(vec![Reply::json(r#"{"available":false}"#)], CancellationToken::new()).await;
		let error = resolve_with(&http, request.clone(), key(), CancellationToken::new())
			.await
			.expect_err("hidden");
		task.await.expect("server");
		assert_eq!(error.current_context().code(), ErrorCode::NexusUnavailable);
		let (http, _, task) = server(vec![Reply::json(r#"{"available":true,"name":"Page","version":"2.0"}"#), Reply::json(r#"{"files":[{"file_id":7,"name":null,"version":"01-beta","category_id":1,"category_name":"MAIN"},{"file_id":8,"category_id":null,"category_name":null}]}"#)], CancellationToken::new()).await;
		let result = resolve_with(&http, request, key(), CancellationToken::new())
			.await
			.expect("metadata");
		task.await.expect("server");
		assert!(result.files[0].main && result.files[0].available);
		assert_eq!(result.files[0].version, "01-beta");
		assert!(result.files[0].name.is_empty());
		assert!(!result.files[1].available);
	}
}
