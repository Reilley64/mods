use domain::EffectiveResult;
use domain::ParticipationReason;
use domain::ProviderRank;
use domain::ProviderReference;
use domain::Tombstone;
use domain::TombstoneScope;
use serde_json::Value;
use serde_json::json;

pub(crate) fn path(value: &str) -> String {
	value.replace('/', "\\")
}

pub(crate) fn rank(value: ProviderRank) -> Value {
	match value {
		ProviderRank::Base => json!({"kind": "base"}),
		ProviderRank::Regular(value) => json!({"kind": "regular", "priority": value.get()}),
		ProviderRank::Overwrite => json!({"kind": "overwrite"}),
	}
}

pub(crate) fn provider(value: &ProviderReference) -> Value {
	let rank = rank(value.rank());
	match value {
		ProviderReference::SteamData { original_path } => json!({
		    "kind": "steam_data", "priority": rank, "original_path": path(original_path.as_str()), "participation_reason": "steam_base",
		}),
		ProviderReference::DataMod {
			mod_name,
			original_path,
			participation_reason,
			..
		} => json!({
		    "kind": "data_mod", "mod_name": mod_name.as_str(), "priority": rank,
		    "original_path": path(original_path.as_str()), "participation_reason": match participation_reason {
			ParticipationReason::SteamBase => "steam_base",
			ParticipationReason::EnabledMod => "enabled_mod",
			ParticipationReason::DisabledMod => "disabled_mod",
			ParticipationReason::HypotheticalEnabledMod => "hypothetical_enabled_mod",
			ParticipationReason::ProjectedDisabledMod => "projected_disabled_mod",
			ParticipationReason::Overwrite => "overwrite",
		    },
		}),
		ProviderReference::Overwrite { original_path } => json!({
		    "kind": "overwrite", "priority": rank, "original_path": path(original_path.as_str()), "participation_reason": "overwrite",
		}),
	}
}

pub(crate) fn tombstone(value: &Tombstone) -> Value {
	json!({"scope": match value.scope { TombstoneScope::ExactFile => "exact_file", TombstoneScope::DirectorySubtree => "directory_subtree" }, "owner": provider(&value.owner)})
}

pub(crate) fn effective_result(value: &EffectiveResult) -> Value {
	match value {
		EffectiveResult::File(file) => json!({"kind": "file", "provider": provider(file)}),
		EffectiveResult::Absent { controlling_tombstone } => {
			json!({"kind": "absent", "controlling_tombstone": controlling_tombstone.as_ref().map(tombstone)})
		}
	}
}
