use crate::DataRelativePath;
use crate::ProviderReference;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TombstoneScope {
	ExactFile,
	DirectorySubtree,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Tombstone {
	pub scope: TombstoneScope,
	pub owner: ProviderReference,
}

impl Tombstone {
	pub fn path(&self) -> &DataRelativePath {
		self.owner.original_path()
	}
}

pub type TombstoneReference = Tombstone;

#[cfg(test)]
mod tests {
	use super::Tombstone;
	use super::TombstoneScope;
	use crate::DataRelativePath;
	use crate::ProviderReference;
	use std::error::Error;
	use std::result::Result as StdResult;

	#[test]
	fn tombstone_path_is_the_owner_reference_path() -> StdResult<(), Box<dyn Error>> {
		let tombstone = Tombstone {
			scope: TombstoneScope::DirectorySubtree,
			owner: ProviderReference::Overwrite {
				original_path: DataRelativePath::new("textures/old".to_owned())
					.map_err(|_| "invalid path")?,
			},
		};

		assert_eq!(tombstone.path().as_str(), "textures/old");
		Ok(())
	}
}
