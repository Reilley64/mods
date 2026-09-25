use crate::EffectiveResult;
use crate::ProviderReference;
use crate::Tombstone;
use crate::TombstoneScope;
use std::collections::HashMap;

/// Callers select participating providers and validate their namespaces before resolution.
/// Insertions are incremental so effectful callers retain their cancellation checkpoints.
#[derive(Default)]
pub struct TombstoneIndex {
	paths: HashMap<String, (usize, Tombstone)>,
	directories: HashMap<String, (usize, Tombstone)>,
	next_order: usize,
}

impl TombstoneIndex {
	pub fn insert(&mut self, tombstone: Tombstone) {
		let key = tombstone.path().comparison_key().to_owned();
		let order = self.next_order;
		self.next_order += 1;

		if tombstone.scope == TombstoneScope::DirectorySubtree
			&& self.directories
				.get(&key)
				.is_none_or(|(_, current)| current.owner.rank() <= tombstone.owner.rank())
		{
			self.directories.insert(key.clone(), (order, tombstone.clone()));
		}
		if self.paths
			.get(&key)
			.is_none_or(|(_, current)| current.owner.rank() <= tombstone.owner.rank())
		{
			self.paths.insert(key, (order, tombstone));
		}
	}

	/// `key` must be a Data-relative comparison key. Equal ranks retain the last insertion.
	/// Lookup visits path ancestors, not the complete provider metadata collection.
	pub fn controlling(&self, key: &str) -> Option<&Tombstone> {
		let mut controlling = self.paths.get(key);
		for (boundary, _) in key.match_indices('/') {
			let Some(ancestor) = self.directories.get(&key[..boundary]) else {
				continue;
			};
			if controlling.is_none_or(|current| {
				(current.1.owner.rank(), current.0) < (ancestor.1.owner.rank(), ancestor.0)
			}) {
				controlling = Some(ancestor);
			}
		}

		controlling.map(|(_, tombstone)| tombstone)
	}

	pub fn suppressing(&self, entry: &ProviderReference) -> Option<&Tombstone> {
		self.controlling(entry.original_path().comparison_key())
			.filter(|tombstone| tombstone.owner.rank() > entry.rank())
	}
}

impl Tombstone {
	/// `key` must be a Data-relative comparison key; subtree boundaries are inclusive.
	pub fn applies_to(&self, key: &str) -> bool {
		let boundary = self.path().comparison_key();
		key == boundary
			|| (self.scope == TombstoneScope::DirectorySubtree
				&& key.strip_prefix(boundary).is_some_and(|suffix| suffix.starts_with('/')))
	}
}

/// Both values must already refer to the queried path. A file at the same rank is not suppressed.
pub fn resolve_effective_file(file: Option<&ProviderReference>, tombstone: Option<&Tombstone>) -> EffectiveResult {
	if let Some(file) = file
		&& tombstone.is_none_or(|tombstone| file.rank() >= tombstone.owner.rank())
	{
		return EffectiveResult::File(file.clone());
	}

	EffectiveResult::Absent {
		controlling_tombstone: tombstone.cloned(),
	}
}

#[cfg(test)]
mod tests {
	use crate::DataRelativePath;
	use crate::EffectiveResult;
	use crate::ModName;
	use crate::ModPriority;
	use crate::ParticipationReason;
	use crate::ProviderReference;
	use crate::Tombstone;
	use crate::TombstoneIndex;
	use crate::TombstoneScope;
	use crate::resolve_effective_file;
	use std::error::Error;
	use std::result::Result as StdResult;

	fn data_mod(priority: u32, path: &str) -> StdResult<ProviderReference, Box<dyn Error>> {
		Ok(ProviderReference::DataMod {
			mod_name: ModName::new(format!("Mod {priority}")).map_err(|_| "invalid name")?,
			priority: ModPriority::new(priority),
			original_path: DataRelativePath::new(path.to_owned()).map_err(|_| "invalid path")?,
			participation_reason: ParticipationReason::EnabledMod,
		})
	}

	#[test]
	fn subtree_suppresses_base_and_lower_mod_but_higher_file_keeps_display_spelling()
	-> StdResult<(), Box<dyn Error>> {
		let lower = data_mod(2, "éς/Weapon.NIF")?;
		let base = ProviderReference::SteamData {
			original_path: lower.original_path().clone(),
		};
		let tombstone = Tombstone {
			owner: data_mod(7, "ÉΣ")?,
			scope: TombstoneScope::DirectorySubtree,
		};
		let higher = data_mod(9, "ÉΣ/WEAPON.nif")?;
		let mut index = TombstoneIndex::default();
		index.insert(tombstone.clone());

		for file in [&base, &lower] {
			assert_eq!(index.suppressing(file), Some(&tombstone));
			assert_eq!(
				resolve_effective_file(
					Some(file),
					index.controlling(file.original_path().comparison_key())
				),
				EffectiveResult::Absent {
					controlling_tombstone: Some(tombstone.clone())
				}
			);
		}
		assert_eq!(index.suppressing(&higher), None);
		assert_eq!(
			resolve_effective_file(
				Some(&higher),
				index.controlling(higher.original_path().comparison_key())
			),
			EffectiveResult::File(higher)
		);
		assert_eq!(index.controlling("éσ-other/weapon.nif"), None);
		Ok(())
	}

	#[test]
	fn exact_and_inclusive_subtree_choose_priority_not_specificity_or_insertion_order()
	-> StdResult<(), Box<dyn Error>> {
		let exact = Tombstone {
			owner: data_mod(4, "Meshes/Gun")?,
			scope: TombstoneScope::ExactFile,
		};
		let broad = Tombstone {
			owner: data_mod(8, "Meshes")?,
			scope: TombstoneScope::DirectorySubtree,
		};
		let mut index = TombstoneIndex::default();
		index.insert(broad.clone());
		index.insert(exact.clone());

		assert_eq!(index.controlling("meshes"), Some(&broad));
		assert_eq!(index.controlling("meshes/gun"), Some(&broad));
		assert_eq!(index.controlling("meshes/gun/part.nif"), Some(&broad));
		assert!(exact.applies_to("meshes/gun"));
		assert!(!exact.applies_to("meshes/gun/part.nif"));
		assert!(broad.applies_to("meshes"));
		assert!(!broad.applies_to("meshes-other/gun"));
		Ok(())
	}

	#[test]
	fn overwrite_controls_stored_mod_priority_without_changing_hypothetical_participation()
	-> StdResult<(), Box<dyn Error>> {
		let hypothetical = data_mod(u32::MAX, "Textures/Visible.dds")?
			.with_participation_reason(ParticipationReason::HypotheticalEnabledMod);
		let overwrite = ProviderReference::Overwrite {
			original_path: hypothetical.original_path().clone(),
		};
		let tombstone = Tombstone {
			owner: overwrite.clone(),
			scope: TombstoneScope::ExactFile,
		};
		let mut index = TombstoneIndex::default();
		index.insert(tombstone.clone());

		assert_eq!(index.suppressing(&hypothetical), Some(&tombstone));
		assert_eq!(
			resolve_effective_file(Some(&hypothetical), None),
			EffectiveResult::File(hypothetical)
		);
		assert_eq!(
			resolve_effective_file(Some(&overwrite), Some(&tombstone)),
			EffectiveResult::File(overwrite)
		);
		assert_eq!(
			resolve_effective_file(None, index.controlling("unknown")),
			EffectiveResult::Absent {
				controlling_tombstone: None
			}
		);
		Ok(())
	}
}
