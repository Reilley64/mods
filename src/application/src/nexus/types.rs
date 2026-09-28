use domain::ArchivePath;

/// Deliberately has no Debug or Display: credentials must not enter diagnostics.
#[derive(Clone)]
pub struct NexusApiKey(String);
impl NexusApiKey {
	pub fn new(value: String) -> Self {
		Self(value)
	}
	pub fn expose_secret(&self) -> &str {
		&self.0
	}
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NexusRequest {
	pub game_domain: String,
	pub mod_id: u64,
	pub file_id: Option<u64>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NexusProvenance {
	pub game_domain: String,
	pub mod_id: u64,
	pub file_id: u64,
	pub file_version: String,
	pub mod_version: String,
	pub mod_name: String,
	pub file_name: String,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NexusFile {
	pub file_id: u64,
	pub name: String,
	pub version: String,
	pub category: String,
	pub available: bool,
	pub main: bool,
}
#[derive(Debug, Clone)]
pub struct NexusMod {
	pub name: String,
	pub version: String,
	pub files: Vec<NexusFile>,
}
#[derive(Debug, Clone)]
pub struct AcquiredNexusArchive {
	pub archive: ArchivePath,
	pub provenance: NexusProvenance,
}
