/// Deliberately has no Debug or Display: credentials must not enter diagnostics.
#[derive(Clone)]
pub(crate) struct NexusApiKey(String);
impl NexusApiKey {
	pub(crate) fn new(value: String) -> Self {
		Self(value)
	}
	pub(crate) fn expose_secret(&self) -> &str {
		&self.0
	}
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NexusRequest {
	pub game_domain: String,
	pub mod_id: u64,
	pub file_id: Option<u64>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NexusFile {
	pub file_id: u64,
	pub name: String,
	pub version: String,
	pub category: String,
	pub available: bool,
	pub main: bool,
}
#[derive(Debug, Clone)]
pub(crate) struct NexusMod {
	pub name: String,
	pub version: String,
	pub files: Vec<NexusFile>,
}
