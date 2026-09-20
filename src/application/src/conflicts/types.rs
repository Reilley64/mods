use domain::ConflictProblem;
use domain::DataRelativePath;
use domain::ProviderIdentity;
use domain::ProviderReference;
use domain::Sha256Digest;
use domain::Tombstone;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct IndexedConflictFileId {
	identity: ProviderIdentity,
	path: DataRelativePath,
}

impl IndexedConflictFileId {
	pub fn new(identity: ProviderIdentity, path: DataRelativePath) -> Self {
		Self { identity, path }
	}

	pub fn identity(&self) -> &ProviderIdentity {
		&self.identity
	}

	pub fn path(&self) -> &DataRelativePath {
		&self.path
	}
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedConflictFile {
	pub id: IndexedConflictFileId,
	pub provider: ProviderReference,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScannedConflictProvider {
	pub identity: ProviderIdentity,
	pub enabled: bool,
	pub files: Vec<IndexedConflictFile>,
	pub directories: Vec<ProviderReference>,
	pub tombstones: Vec<Tombstone>,
	pub problems: Vec<ConflictProblem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvironmentConflictScan {
	pub providers: Vec<ScannedConflictProvider>,
	pub problems: Vec<ConflictProblem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConflictContentRead {
	Sha256(Sha256Digest),
	Unavailable,
	Unstable,
}
