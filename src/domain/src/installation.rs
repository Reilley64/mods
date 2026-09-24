use crate::DataRelativePath;
use crate::InvalidModName;
use crate::ModName;
use crate::environment::is_absolute_external;
use rootcause::Result;
use rootcause::report;
use std::error::Error;
use std::fmt;
use std::path::Path;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidArchivePath;
impl fmt::Display for InvalidArchivePath {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		formatter.write_str("archive path must be nonempty and absolute")
	}
}
impl Error for InvalidArchivePath {}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ArchivePath(PathBuf);
impl ArchivePath {
	pub fn new(path: PathBuf) -> Result<Self, InvalidArchivePath> {
		if !path.as_os_str().is_empty() && is_absolute_external(&path) {
			Ok(Self(path))
		} else {
			Err(report!(InvalidArchivePath))
		}
	}
	pub fn as_path(&self) -> &Path {
		&self.0
	}
	pub fn into_path_buf(self) -> PathBuf {
		self.0
	}
	pub fn derived_mod_name(&self) -> Result<ModName, InvalidModName> {
		let Some(text) = self.0.to_str() else {
			return Err(report!(InvalidModName));
		};

		let leaf = text.rsplit(['/', '\\']).next().unwrap_or_default();
		let stem = leaf.rsplit_once('.').map_or(leaf, |(stem, _)| stem);

		ModName::new(stem.to_owned())
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidSha256Digest;
impl fmt::Display for InvalidSha256Digest {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		formatter.write_str("SHA-256 digest must contain 64 lowercase hexadecimal characters")
	}
}
impl Error for InvalidSha256Digest {}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Sha256Digest(String);
impl Sha256Digest {
	pub fn new(value: String) -> Result<Self, InvalidSha256Digest> {
		if value.len() == 64
			&& value.bytes()
				.all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
		{
			Ok(Self(value))
		} else {
			Err(report!(InvalidSha256Digest))
		}
	}
	pub fn as_str(&self) -> &str {
		&self.0
	}
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArchiveIdentity {
	DataArchive {
		archive_sha256: Sha256Digest,
		package_root: String,
	},
	Fomod {
		archive_sha256: Sha256Digest,
		package_root: String,
		config_member: String,
		config_sha256: Sha256Digest,
	},
}
impl ArchiveIdentity {
	pub fn archive_sha256(&self) -> &Sha256Digest {
		match self {
			Self::DataArchive { archive_sha256, .. } | Self::Fomod { archive_sha256, .. } => archive_sha256,
		}
	}
	pub fn package_root(&self) -> &str {
		match self {
			Self::DataArchive { package_root, .. } | Self::Fomod { package_root, .. } => package_root,
		}
	}
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FomodChoice {
	pub group_id: String,
	pub option_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum InstallationPhase {
	Required,
	SelectedOrForced,
	Conditional,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum FileDependencyState {
	Missing,
	Inactive,
	Active,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FomodCondition {
	Constant(bool),
	All(Vec<Self>),
	Any(Vec<Self>),
	FileDependency { path: String, state: FileDependencyState },
	FlagDependency { name: String, value: String },
	GameDependency { minimum_version: String },
	NvseDependency { minimum_version: String },
	FommDependency { minimum_version: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptionFileTrigger {
	Selected,
	AlwaysInstall,
	InstallIfUsable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallCandidateOrigin {
	Required,
	Option {
		group_id: String,
		option_id: String,
		trigger: OptionFileTrigger,
	},
	Conditional {
		pattern_order: u64,
		condition: FomodCondition,
	},
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallCandidate {
	pub candidate_id: u64,
	pub origin: InstallCandidateOrigin,
	pub phase: InstallationPhase,
	pub declared_priority: i32,
	pub descriptor_order: u64,
	pub source_member: String,
	pub destination: DataRelativePath,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FomodCardinality {
	SelectExactlyOne,
	SelectAtMostOne,
	SelectAtLeastOne,
	SelectAny,
	SelectAll,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolvedOptionType {
	Required,
	NotUsable,
	Recommended,
	Optional,
	CouldBeUsable,
}

#[cfg(test)]
mod tests {
	use super::ArchivePath;
	use super::Sha256Digest;
	use std::error::Error;
	use std::path::PathBuf;
	use std::result::Result as StdResult;

	#[test]
	fn archive_path_derives_only_the_final_extension_after_validation() -> StdResult<(), Box<dyn Error>> {
		let archive = ArchivePath::new(PathBuf::from("C:\\Downloads\\Some.Mod.7z"))
			.map_err(|_| "invalid archive path")?;
		let name = archive.derived_mod_name().map_err(|_| "invalid derived name")?;
		assert_eq!(name.as_str(), "Some.Mod");
		Ok(())
	}

	#[test]
	fn sha256_requires_lowercase_hex() {
		assert!(Sha256Digest::new("a".repeat(64)).is_ok());
		assert!(Sha256Digest::new("A".repeat(64)).is_err());
		assert!(Sha256Digest::new("a".repeat(63)).is_err());
	}
}
