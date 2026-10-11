use application::ErrorMarker;
use rootcause::Result;
use rootcause::report;
use std::io::ErrorKind;
use std::path::Path;
use tokio::fs::read_to_string;

pub(super) async fn read_optional_text(path: &Path) -> Result<Option<String>, ErrorMarker> {
	match read_to_string(path).await {
		Ok(text) => Ok(Some(text)),
		Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
		Err(error) => Err(report!(error).context(ErrorMarker::game_install_invalid())),
	}
}

pub(super) async fn read_required_text(path: &Path) -> Result<String, ErrorMarker> {
	read_optional_text(path)
		.await?
		.ok_or_else(|| report!(ErrorMarker::game_install_invalid()))
}
