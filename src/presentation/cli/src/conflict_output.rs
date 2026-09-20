use crate::output::quote;
use application::conflicts::ExplainPathOutput;
use application::conflicts::InspectModConflictsOutput;
use application::conflicts::ListEffectiveConflictsOutput;
use domain::ConflictProblem;
use domain::ConflictProblemKind;
use domain::ConflictRow;
use domain::ContentComparison;
use domain::EffectiveResult;
use domain::Participation;
use domain::ParticipationReason;
use domain::ProblemScope;
use domain::ProviderIdentity;
use domain::ProviderRank;
use domain::ProviderReference;
use domain::ProviderState;
use domain::ProviderSummary;
use domain::ResolutionReason;
use domain::ResolutionStatus;
use domain::Tombstone;
use domain::TombstoneEffect;
use domain::TombstoneScope;
use std::fmt::Write;

pub(crate) fn list(output: &ListEffectiveConflictsOutput) -> String {
	let mut text = String::new();
	line_string(
		&mut text,
		"resolution_status",
		resolution_status(output.resolution_status),
	);
	rows(&mut text, "rows", &output.rows);
	problems(&mut text, "problems", &output.problems);
	text
}

pub(crate) fn inspection(output: &InspectModConflictsOutput) -> String {
	let mut text = String::new();
	line_string(&mut text, "mod_name", output.mod_name.as_str());
	line_string(&mut text, "participation", participation(output.participation));
	line_string(
		&mut text,
		"resolution_status",
		resolution_status(output.resolution_status),
	);
	provider_summary(&mut text, "provider_summary", &output.provider_summary);
	rows(&mut text, "rows", &output.rows);
	problems(&mut text, "problems", &output.problems);
	text
}

pub(crate) fn explanation(output: &ExplainPathOutput) -> String {
	let mut text = String::new();
	line_string(&mut text, "normalized_key", &output.normalized_key);
	line_path(&mut text, "display_path", output.display_path.as_str());
	line_string(
		&mut text,
		"resolution_status",
		resolution_status(output.resolution_status),
	);
	if let Some(result) = &output.effective_result {
		effective_result(&mut text, "effective_result", result);
	}
	providers(&mut text, "provider_stack", &output.provider_stack);
	tombstone_effects(&mut text, "tombstone_effects", &output.tombstone_effects);
	content_comparisons(&mut text, "content_comparisons", &output.content_comparisons);
	resolution_reasons(&mut text, "reasons", &output.reasons);
	problems(&mut text, "problems", &output.problems);
	text
}

fn rows(text: &mut String, prefix: &str, values: &[ConflictRow]) {
	line_count(text, prefix, values.len());
	for (index, row) in values.iter().enumerate() {
		let prefix = format!("{prefix}[{index}]");
		match row {
			ConflictRow::OrdinaryConflict {
				normalized_key,
				display_path,
				participation: row_participation,
				effective_file,
				losing_files,
				content_comparisons: comparisons,
			} => {
				line_string(text, &format!("{prefix}.kind"), "ordinary_conflict");
				line_string(text, &format!("{prefix}.normalized_key"), normalized_key);
				line_path(text, &format!("{prefix}.display_path"), display_path.as_str());
				line_string(
					text,
					&format!("{prefix}.participation"),
					participation(*row_participation),
				);
				provider(text, &format!("{prefix}.effective_file"), effective_file);
				providers(text, &format!("{prefix}.losing_files"), losing_files);
				content_comparisons(text, &format!("{prefix}.content_comparisons"), comparisons);
			}
			ConflictRow::Tombstone {
				normalized_key,
				display_path,
				participation: row_participation,
				controlling_tombstone,
				suppressed_entries,
			} => {
				line_string(text, &format!("{prefix}.kind"), "tombstone");
				line_string(text, &format!("{prefix}.normalized_key"), normalized_key);
				line_path(text, &format!("{prefix}.display_path"), display_path.as_str());
				line_string(
					text,
					&format!("{prefix}.participation"),
					participation(*row_participation),
				);
				tombstone(text, &format!("{prefix}.controlling_tombstone"), controlling_tombstone);
				providers(text, &format!("{prefix}.suppressed_entries"), suppressed_entries);
			}
		}
	}
}

fn provider_summary(text: &mut String, prefix: &str, summary: &ProviderSummary) {
	line_u64(
		text,
		&format!("{prefix}.ordinary_file_count"),
		summary.ordinary_file_count,
	);
	line_u64(
		text,
		&format!("{prefix}.effective_file_count"),
		summary.effective_file_count,
	);
	line_u64(
		text,
		&format!("{prefix}.file_conflict_win_count"),
		summary.file_conflict_win_count,
	);
	line_u64(
		text,
		&format!("{prefix}.file_conflict_loss_count"),
		summary.file_conflict_loss_count,
	);
	line_u64(
		text,
		&format!("{prefix}.owned_file_tombstone_count"),
		summary.owned_file_tombstone_count,
	);
	line_u64(
		text,
		&format!("{prefix}.owned_directory_tombstone_count"),
		summary.owned_directory_tombstone_count,
	);
	line_u64(
		text,
		&format!("{prefix}.lower_file_entries_suppressed_count"),
		summary.lower_file_entries_suppressed_count,
	);
	line_u64(
		text,
		&format!("{prefix}.own_file_entries_suppressed_count"),
		summary.own_file_entries_suppressed_count,
	);
	line_string(text, &format!("{prefix}.state"), provider_state(summary.state));
}

fn providers(text: &mut String, prefix: &str, values: &[ProviderReference]) {
	line_count(text, prefix, values.len());
	for (index, value) in values.iter().enumerate() {
		provider(text, &format!("{prefix}[{index}]"), value);
	}
}

fn provider(text: &mut String, prefix: &str, value: &ProviderReference) {
	match value {
		ProviderReference::SteamData { original_path } => {
			line_string(text, &format!("{prefix}.kind"), "steam_data");
			provider_priority(text, &format!("{prefix}.priority"), value.rank());
			line_path(text, &format!("{prefix}.original_path"), original_path.as_str());
			line_string(text, &format!("{prefix}.participation_reason"), "steam_base");
		}
		ProviderReference::DataMod {
			mod_name,
			original_path,
			participation_reason,
			..
		} => {
			line_string(text, &format!("{prefix}.kind"), "data_mod");
			line_string(text, &format!("{prefix}.mod_name"), mod_name.as_str());
			provider_priority(text, &format!("{prefix}.priority"), value.rank());
			line_path(text, &format!("{prefix}.original_path"), original_path.as_str());
			line_string(
				text,
				&format!("{prefix}.participation_reason"),
				match participation_reason {
					ParticipationReason::SteamBase => "steam_base",
					ParticipationReason::EnabledMod => "enabled_mod",
					ParticipationReason::DisabledMod => "disabled_mod",
					ParticipationReason::HypotheticalEnabledMod => "hypothetical_enabled_mod",
					ParticipationReason::ProjectedDisabledMod => "projected_disabled_mod",
					ParticipationReason::Overwrite => "overwrite",
				},
			);
		}
		ProviderReference::Overwrite { original_path } => {
			line_string(text, &format!("{prefix}.kind"), "overwrite");
			provider_priority(text, &format!("{prefix}.priority"), value.rank());
			line_path(text, &format!("{prefix}.original_path"), original_path.as_str());
			line_string(text, &format!("{prefix}.participation_reason"), "overwrite");
		}
	}
}

fn provider_identity(text: &mut String, prefix: &str, value: &ProviderIdentity) {
	match value {
		ProviderIdentity::SteamData => {
			line_string(text, &format!("{prefix}.kind"), "steam_data");
			provider_priority(text, &format!("{prefix}.priority"), value.rank());
		}
		ProviderIdentity::DataMod { mod_name, .. } => {
			line_string(text, &format!("{prefix}.kind"), "data_mod");
			line_string(text, &format!("{prefix}.mod_name"), mod_name.as_str());
			provider_priority(text, &format!("{prefix}.priority"), value.rank());
		}
		ProviderIdentity::Overwrite => {
			line_string(text, &format!("{prefix}.kind"), "overwrite");
			provider_priority(text, &format!("{prefix}.priority"), value.rank());
		}
	}
}

fn provider_priority(text: &mut String, prefix: &str, value: ProviderRank) {
	match value {
		ProviderRank::Base => line_string(text, &format!("{prefix}.kind"), "base"),
		ProviderRank::Regular(priority) => {
			line_string(text, &format!("{prefix}.kind"), "regular");
			line_u32(text, &format!("{prefix}.priority"), priority.get());
		}
		ProviderRank::Overwrite => line_string(text, &format!("{prefix}.kind"), "overwrite"),
	}
}

fn tombstone(text: &mut String, prefix: &str, value: &Tombstone) {
	line_string(
		text,
		&format!("{prefix}.scope"),
		match value.scope {
			TombstoneScope::ExactFile => "exact_file",
			TombstoneScope::DirectorySubtree => "directory_subtree",
		},
	);
	provider(text, &format!("{prefix}.owner"), &value.owner);
}

fn tombstone_effects(text: &mut String, prefix: &str, values: &[TombstoneEffect]) {
	line_count(text, prefix, values.len());
	for (index, effect) in values.iter().enumerate() {
		let prefix = format!("{prefix}[{index}]");
		match effect {
			TombstoneEffect::Controlling {
				tombstone: controlling_tombstone,
				suppressed_entries,
			} => {
				line_string(text, &format!("{prefix}.kind"), "controlling");
				tombstone(text, &format!("{prefix}.tombstone"), controlling_tombstone);
				providers(text, &format!("{prefix}.suppressed_entries"), suppressed_entries);
			}
			TombstoneEffect::ShadowedByTombstone {
				tombstone: shadowed,
				controlling_tombstone,
			} => {
				line_string(text, &format!("{prefix}.kind"), "shadowed_by_tombstone");
				tombstone(text, &format!("{prefix}.tombstone"), shadowed);
				tombstone(text, &format!("{prefix}.controlling_tombstone"), controlling_tombstone);
			}
			TombstoneEffect::OverriddenByFile {
				tombstone: overridden,
				overriding_file,
			} => {
				line_string(text, &format!("{prefix}.kind"), "overridden_by_file");
				tombstone(text, &format!("{prefix}.tombstone"), overridden);
				provider(text, &format!("{prefix}.overriding_file"), overriding_file);
			}
			TombstoneEffect::Orphan { tombstone: orphan } => {
				line_string(text, &format!("{prefix}.kind"), "orphan");
				tombstone(text, &format!("{prefix}.tombstone"), orphan);
			}
		}
	}
}

fn content_comparisons(text: &mut String, prefix: &str, values: &[ContentComparison]) {
	line_count(text, prefix, values.len());
	for (index, comparison) in values.iter().enumerate() {
		let prefix = format!("{prefix}[{index}]");
		line_string(
			text,
			&format!("{prefix}.state"),
			match comparison {
				ContentComparison::NotCompared { .. } => "not_compared",
				ContentComparison::SameSha256 { .. } => "same_sha256",
				ContentComparison::DifferentSha256 { .. } => "different_sha256",
				ContentComparison::Unavailable { .. } => "unavailable",
				ContentComparison::Unstable { .. } => "unstable",
			},
		);
		provider(text, &format!("{prefix}.winner"), comparison.winner());
		provider(text, &format!("{prefix}.loser"), comparison.loser());
		match comparison {
			ContentComparison::SameSha256 {
				winner_sha256,
				loser_sha256,
				..
			}
			| ContentComparison::DifferentSha256 {
				winner_sha256,
				loser_sha256,
				..
			} => {
				line_string(text, &format!("{prefix}.winner_sha256"), winner_sha256.as_str());
				line_string(text, &format!("{prefix}.loser_sha256"), loser_sha256.as_str());
			}
			ContentComparison::NotCompared { .. }
			| ContentComparison::Unavailable { .. }
			| ContentComparison::Unstable { .. } => {}
		}
	}
}

fn effective_result(text: &mut String, prefix: &str, value: &EffectiveResult) {
	match value {
		EffectiveResult::File(provider_value) => {
			line_string(text, &format!("{prefix}.kind"), "file");
			provider(text, &format!("{prefix}.provider"), provider_value);
		}
		EffectiveResult::Absent { controlling_tombstone } => {
			line_string(text, &format!("{prefix}.kind"), "absent");
			if let Some(controlling_tombstone) = controlling_tombstone {
				tombstone(text, &format!("{prefix}.controlling_tombstone"), controlling_tombstone);
			}
		}
	}
}

fn resolution_reasons(text: &mut String, prefix: &str, values: &[ResolutionReason]) {
	line_count(text, prefix, values.len());
	for (index, reason) in values.iter().enumerate() {
		let prefix = format!("{prefix}[{index}]");
		match reason {
			ResolutionReason::EffectiveFile { provider: winner } => {
				line_string(text, &format!("{prefix}.kind"), "effective_file");
				provider(text, &format!("{prefix}.provider"), winner);
			}
			ResolutionReason::LowerPriorityFile {
				provider: loser,
				winner,
			} => {
				line_string(text, &format!("{prefix}.kind"), "lower_priority_file");
				provider(text, &format!("{prefix}.provider"), loser);
				provider(text, &format!("{prefix}.winner"), winner);
			}
			ResolutionReason::SuppressedByTombstone {
				provider: suppressed,
				controlling_tombstone,
			} => {
				line_string(text, &format!("{prefix}.kind"), "suppressed_by_tombstone");
				provider(text, &format!("{prefix}.provider"), suppressed);
				tombstone(text, &format!("{prefix}.controlling_tombstone"), controlling_tombstone);
			}
			ResolutionReason::AbsentNoEntry => {
				line_string(text, &format!("{prefix}.kind"), "absent_no_entry");
			}
			ResolutionReason::AbsentByTombstone { controlling_tombstone } => {
				line_string(text, &format!("{prefix}.kind"), "absent_by_tombstone");
				tombstone(text, &format!("{prefix}.controlling_tombstone"), controlling_tombstone);
			}
			ResolutionReason::ShadowedTombstone {
				tombstone: shadowed,
				controlling_tombstone,
			} => {
				line_string(text, &format!("{prefix}.kind"), "shadowed_tombstone");
				tombstone(text, &format!("{prefix}.tombstone"), shadowed);
				tombstone(text, &format!("{prefix}.controlling_tombstone"), controlling_tombstone);
			}
			ResolutionReason::TombstoneOverriddenByFile {
				tombstone: overridden,
				overriding_file,
			} => {
				line_string(text, &format!("{prefix}.kind"), "tombstone_overridden_by_file");
				tombstone(text, &format!("{prefix}.tombstone"), overridden);
				provider(text, &format!("{prefix}.overriding_file"), overriding_file);
			}
			ResolutionReason::NamespaceInvalid => {
				line_string(text, &format!("{prefix}.kind"), "namespace_invalid");
			}
		}
	}
}

fn problems(text: &mut String, prefix: &str, values: &[ConflictProblem]) {
	line_count(text, prefix, values.len());
	for (index, problem) in values.iter().enumerate() {
		let prefix = format!("{prefix}[{index}]");
		line_string(
			text,
			&format!("{prefix}.kind"),
			match problem.kind {
				ConflictProblemKind::ModlistInvalid => "modlist_invalid",
				ConflictProblemKind::ProviderMissing => "provider_missing",
				ConflictProblemKind::InternalKeyCollision => "internal_key_collision",
				ConflictProblemKind::FileDirectoryCollision => "file_directory_collision",
				ConflictProblemKind::OrdinaryTombstoneCollision => "ordinary_tombstone_collision",
				ConflictProblemKind::DirectoryTombstoneDescendantCollision => {
					"directory_tombstone_descendant_collision"
				}
				ConflictProblemKind::InvalidTombstoneMetadata => "invalid_tombstone_metadata",
				ConflictProblemKind::InvalidTombstonePath => "invalid_tombstone_path",
				ConflictProblemKind::ReparsePoint => "reparse_point",
				ConflictProblemKind::HardLink => "hard_link",
				ConflictProblemKind::UnsupportedEntryType => "unsupported_entry_type",
				ConflictProblemKind::ContainmentEscape => "containment_escape",
				ConflictProblemKind::NonLosslessName => "non_lossless_name",
				ConflictProblemKind::ReservedPath => "reserved_path",
			},
		);
		problem_scope(text, &format!("{prefix}.scope"), &problem.scope);
	}
}

fn problem_scope(text: &mut String, prefix: &str, scope: &ProblemScope) {
	match scope {
		ProblemScope::Global => line_string(text, &format!("{prefix}.kind"), "global"),
		ProblemScope::Modlist => line_string(text, &format!("{prefix}.kind"), "modlist"),
		ProblemScope::Provider(identity) => {
			line_string(text, &format!("{prefix}.kind"), "provider");
			provider_identity(text, &format!("{prefix}.provider"), identity);
		}
		ProblemScope::Path {
			normalized_key,
			display_path,
		} => {
			line_string(text, &format!("{prefix}.kind"), "path");
			line_string(text, &format!("{prefix}.normalized_key"), normalized_key);
			line_path(text, &format!("{prefix}.display_path"), display_path.as_str());
		}
		ProblemScope::Subtree {
			normalized_key,
			display_path,
		} => {
			line_string(text, &format!("{prefix}.kind"), "subtree");
			line_string(text, &format!("{prefix}.normalized_key"), normalized_key);
			line_path(text, &format!("{prefix}.display_path"), display_path.as_str());
		}
	}
}

const fn resolution_status(value: ResolutionStatus) -> &'static str {
	match value {
		ResolutionStatus::Exact => "exact",
		ResolutionStatus::Invalid => "invalid",
	}
}

const fn participation(value: Participation) -> &'static str {
	match value {
		Participation::Active => "active",
		Participation::Inactive => "inactive",
		Participation::Hypothetical => "hypothetical",
	}
}

const fn provider_state(value: ProviderState) -> &'static str {
	match value {
		ProviderState::Invalid => "invalid",
		ProviderState::Inactive => "inactive",
		ProviderState::SuppressionOnly => "suppression_only",
		ProviderState::Empty => "empty",
		ProviderState::FullyOverridden => "fully_overridden",
		ProviderState::Mixed => "mixed",
		ProviderState::WinningConflicts => "winning_conflicts",
		ProviderState::LosingConflicts => "losing_conflicts",
		ProviderState::Uncontested => "uncontested",
	}
}

fn line_string(text: &mut String, name: &str, value: &str) {
	let _ = writeln!(text, "{name} = {}", quote(value));
}

fn line_path(text: &mut String, name: &str, value: &str) {
	line_string(text, name, &value.replace('/', "\\"));
}

fn line_u32(text: &mut String, name: &str, value: u32) {
	let _ = writeln!(text, "{name} = {value}");
}

fn line_u64(text: &mut String, name: &str, value: u64) {
	let _ = writeln!(text, "{name} = {value}");
}

fn line_count(text: &mut String, name: &str, value: usize) {
	let _ = writeln!(text, "{name}.count = {value}");
}

#[cfg(test)]
mod tests {
	use super::explanation;
	use super::inspection;
	use super::list;
	use super::participation;
	use super::provider_state;
	use super::resolution_status;
	use application::conflicts::ExplainPathOutput;
	use application::conflicts::InspectModConflictsOutput;
	use application::conflicts::ListEffectiveConflictsOutput;
	use domain::ConflictProblem;
	use domain::ConflictProblemKind;
	use domain::ConflictRow;
	use domain::ContentComparison;
	use domain::DataRelativePath;
	use domain::EffectiveResult;
	use domain::ModName;
	use domain::ModPriority;
	use domain::Participation;
	use domain::ParticipationReason;
	use domain::ProblemScope;
	use domain::ProviderIdentity;
	use domain::ProviderReference;
	use domain::ProviderState;
	use domain::ProviderSummary;
	use domain::ResolutionReason;
	use domain::ResolutionStatus;
	use domain::Sha256Digest;
	use domain::Tombstone;
	use domain::TombstoneEffect;
	use domain::TombstoneScope;
	use std::error::Error;

	fn path(value: &str) -> Result<DataRelativePath, Box<dyn Error>> {
		DataRelativePath::new(value.to_owned()).map_err(|_| "test path must be valid".into())
	}

	fn mod_name(value: &str) -> Result<ModName, Box<dyn Error>> {
		ModName::new(value.to_owned()).map_err(|_| "test mod name must be valid".into())
	}

	fn digest(value: char) -> Result<Sha256Digest, Box<dyn Error>> {
		Sha256Digest::new(value.to_string().repeat(64)).map_err(|_| "test digest must be valid".into())
	}

	fn steam(value: &str) -> Result<ProviderReference, Box<dyn Error>> {
		Ok(ProviderReference::SteamData {
			original_path: path(value)?,
		})
	}

	fn data_mod(
		name: &str,
		priority: u32,
		value: &str,
		reason: ParticipationReason,
	) -> Result<ProviderReference, Box<dyn Error>> {
		Ok(ProviderReference::DataMod {
			mod_name: mod_name(name)?,
			priority: ModPriority::new(priority),
			original_path: path(value)?,
			participation_reason: reason,
		})
	}

	fn overwrite(value: &str) -> Result<ProviderReference, Box<dyn Error>> {
		Ok(ProviderReference::Overwrite {
			original_path: path(value)?,
		})
	}

	fn file_tombstone(owner: ProviderReference) -> Tombstone {
		Tombstone {
			scope: TombstoneScope::ExactFile,
			owner,
		}
	}

	fn directory_tombstone(owner: ProviderReference) -> Tombstone {
		Tombstone {
			scope: TombstoneScope::DirectorySubtree,
			owner,
		}
	}

	#[test]
	fn scalar_conflict_values_have_fixed_names() {
		assert_eq!(resolution_status(ResolutionStatus::Exact), "exact");
		assert_eq!(resolution_status(ResolutionStatus::Invalid), "invalid");
		assert_eq!(participation(Participation::Active), "active");
		assert_eq!(participation(Participation::Inactive), "inactive");
		assert_eq!(participation(Participation::Hypothetical), "hypothetical");

		let states = [
			(ProviderState::Invalid, "invalid"),
			(ProviderState::Inactive, "inactive"),
			(ProviderState::SuppressionOnly, "suppression_only"),
			(ProviderState::Empty, "empty"),
			(ProviderState::FullyOverridden, "fully_overridden"),
			(ProviderState::Mixed, "mixed"),
			(ProviderState::WinningConflicts, "winning_conflicts"),
			(ProviderState::LosingConflicts, "losing_conflicts"),
			(ProviderState::Uncontested, "uncontested"),
		];
		for (state, expected) in states {
			assert_eq!(provider_state(state), expected);
		}
	}

	#[test]
	fn list_output_renders_all_row_content_provider_and_problem_variants() -> Result<(), Box<dyn Error>> {
		let winner = overwrite("Textures/Effects.dds")?;
		let loser = data_mod("Visuals", 5, "textures/effects.dds", ParticipationReason::EnabledMod)?;
		let base = steam("textures/effects.dds")?;
		let disabled = data_mod(
			"Disabled Visuals",
			2,
			"textures/effects.dds",
			ParticipationReason::DisabledMod,
		)?;
		let projected = data_mod(
			"Projected Visuals",
			6,
			"textures/effects.dds",
			ParticipationReason::ProjectedDisabledMod,
		)?;
		let comparisons = vec![
			ContentComparison::NotCompared {
				winner: winner.clone(),
				loser: loser.clone(),
			},
			ContentComparison::SameSha256 {
				winner: winner.clone(),
				loser: base.clone(),
				winner_sha256: digest('a')?,
				loser_sha256: digest('a')?,
			},
			ContentComparison::DifferentSha256 {
				winner: winner.clone(),
				loser: disabled.clone(),
				winner_sha256: digest('b')?,
				loser_sha256: digest('c')?,
			},
			ContentComparison::Unavailable {
				winner: winner.clone(),
				loser: projected.clone(),
			},
			ContentComparison::Unstable {
				winner: winner.clone(),
				loser: base.clone(),
			},
		];
		let controlling = directory_tombstone(data_mod(
			"Hypothetical Visuals",
			7,
			"textures",
			ParticipationReason::HypotheticalEnabledMod,
		)?);
		let rows = vec![
			ConflictRow::OrdinaryConflict {
				normalized_key: "textures/effects.dds".to_owned(),
				display_path: path("Textures/Effects.dds")?,
				participation: Participation::Active,
				effective_file: winner,
				losing_files: vec![loser, base.clone(), disabled, projected],
				content_comparisons: comparisons,
			},
			ConflictRow::Tombstone {
				normalized_key: "textures".to_owned(),
				display_path: path("Textures")?,
				participation: Participation::Hypothetical,
				controlling_tombstone: controlling,
				suppressed_entries: vec![base],
			},
		];
		let kinds = [
			ConflictProblemKind::ModlistInvalid,
			ConflictProblemKind::ProviderMissing,
			ConflictProblemKind::InternalKeyCollision,
			ConflictProblemKind::FileDirectoryCollision,
			ConflictProblemKind::OrdinaryTombstoneCollision,
			ConflictProblemKind::DirectoryTombstoneDescendantCollision,
			ConflictProblemKind::InvalidTombstoneMetadata,
			ConflictProblemKind::InvalidTombstonePath,
			ConflictProblemKind::ReparsePoint,
			ConflictProblemKind::HardLink,
			ConflictProblemKind::UnsupportedEntryType,
			ConflictProblemKind::ContainmentEscape,
			ConflictProblemKind::NonLosslessName,
			ConflictProblemKind::ReservedPath,
		];
		let mut problems = kinds
			.into_iter()
			.map(|kind| ConflictProblem {
				kind,
				scope: ProblemScope::Global,
			})
			.collect::<Vec<_>>();
		problems.extend([
			ConflictProblem {
				kind: ConflictProblemKind::ModlistInvalid,
				scope: ProblemScope::Modlist,
			},
			ConflictProblem {
				kind: ConflictProblemKind::ProviderMissing,
				scope: ProblemScope::Provider(ProviderIdentity::SteamData),
			},
			ConflictProblem {
				kind: ConflictProblemKind::ProviderMissing,
				scope: ProblemScope::Provider(ProviderIdentity::DataMod {
					mod_name: mod_name("Missing")?,
					priority: ModPriority::new(8),
				}),
			},
			ConflictProblem {
				kind: ConflictProblemKind::ProviderMissing,
				scope: ProblemScope::Provider(ProviderIdentity::Overwrite),
			},
			ConflictProblem {
				kind: ConflictProblemKind::ReservedPath,
				scope: ProblemScope::Path {
					normalized_key: "textures/file.dds".to_owned(),
					display_path: path("Textures/File.dds")?,
				},
			},
			ConflictProblem {
				kind: ConflictProblemKind::ReservedPath,
				scope: ProblemScope::Subtree {
					normalized_key: "textures".to_owned(),
					display_path: path("Textures")?,
				},
			},
		]);

		let text = list(&ListEffectiveConflictsOutput {
			resolution_status: ResolutionStatus::Invalid,
			rows,
			problems,
		});

		for expected in [
			"resolution_status = \"invalid\"",
			"rows.count = 2",
			"rows[0].kind = \"ordinary_conflict\"",
			"rows[0].display_path = \"Textures\\\\Effects.dds\"",
			"rows[0].effective_file.kind = \"overwrite\"",
			"rows[0].effective_file.priority.kind = \"overwrite\"",
			"rows[0].losing_files[0].kind = \"data_mod\"",
			"rows[0].losing_files[0].priority.kind = \"regular\"",
			"rows[0].losing_files[0].priority.priority = 5",
			"rows[0].losing_files[1].kind = \"steam_data\"",
			"rows[0].losing_files[1].priority.kind = \"base\"",
			"rows[0].content_comparisons[0].state = \"not_compared\"",
			"rows[0].content_comparisons[1].state = \"same_sha256\"",
			"rows[0].content_comparisons[2].state = \"different_sha256\"",
			"rows[0].content_comparisons[3].state = \"unavailable\"",
			"rows[0].content_comparisons[4].state = \"unstable\"",
			"rows[1].kind = \"tombstone\"",
			"rows[1].controlling_tombstone.scope = \"directory_subtree\"",
			"problems[14].scope.kind = \"modlist\"",
			"problems[15].scope.provider.kind = \"steam_data\"",
			"problems[15].scope.provider.priority.kind = \"base\"",
			"problems[16].scope.provider.kind = \"data_mod\"",
			"problems[16].scope.provider.priority.kind = \"regular\"",
			"problems[16].scope.provider.priority.priority = 8",
			"problems[17].scope.provider.kind = \"overwrite\"",
			"problems[17].scope.provider.priority.kind = \"overwrite\"",
			"problems[18].scope.kind = \"path\"",
			"problems[19].scope.kind = \"subtree\"",
		] {
			assert!(text.contains(expected), "missing {expected}");
		}
		for kind in [
			"modlist_invalid",
			"provider_missing",
			"internal_key_collision",
			"file_directory_collision",
			"ordinary_tombstone_collision",
			"directory_tombstone_descendant_collision",
			"invalid_tombstone_metadata",
			"invalid_tombstone_path",
			"reparse_point",
			"hard_link",
			"unsupported_entry_type",
			"containment_escape",
			"non_lossless_name",
			"reserved_path",
		] {
			assert!(text.contains(&format!("kind = \"{kind}\"")), "missing {kind}");
		}
		Ok(())
	}

	#[test]
	fn inspect_output_contains_the_complete_provider_summary() -> Result<(), Box<dyn Error>> {
		let text = inspection(&InspectModConflictsOutput {
			mod_name: mod_name("Visuals")?,
			participation: Participation::Inactive,
			resolution_status: ResolutionStatus::Exact,
			provider_summary: ProviderSummary {
				ordinary_file_count: 1,
				effective_file_count: 2,
				file_conflict_win_count: 3,
				file_conflict_loss_count: 4,
				owned_file_tombstone_count: 5,
				owned_directory_tombstone_count: 6,
				lower_file_entries_suppressed_count: 7,
				own_file_entries_suppressed_count: 8,
				state: ProviderState::Mixed,
			},
			rows: Vec::new(),
			problems: Vec::new(),
		});

		assert_eq!(
			text,
			concat!(
				"mod_name = \"Visuals\"\n",
				"participation = \"inactive\"\n",
				"resolution_status = \"exact\"\n",
				"provider_summary.ordinary_file_count = 1\n",
				"provider_summary.effective_file_count = 2\n",
				"provider_summary.file_conflict_win_count = 3\n",
				"provider_summary.file_conflict_loss_count = 4\n",
				"provider_summary.owned_file_tombstone_count = 5\n",
				"provider_summary.owned_directory_tombstone_count = 6\n",
				"provider_summary.lower_file_entries_suppressed_count = 7\n",
				"provider_summary.own_file_entries_suppressed_count = 8\n",
				"provider_summary.state = \"mixed\"\n",
				"rows.count = 0\n",
				"problems.count = 0\n",
			)
		);
		Ok(())
	}

	#[test]
	fn explain_output_renders_all_effect_and_reason_variants() -> Result<(), Box<dyn Error>> {
		let winner = overwrite("Meshes/Armor.nif")?;
		let loser = steam("meshes/armor.nif")?;
		let controlling =
			directory_tombstone(data_mod("Armor", 10, "meshes", ParticipationReason::EnabledMod)?);
		let shadowed = file_tombstone(data_mod(
			"Older Armor",
			4,
			"meshes/armor.nif",
			ParticipationReason::EnabledMod,
		)?);
		let orphan = file_tombstone(overwrite("meshes/missing.nif")?);
		let output = ExplainPathOutput {
			normalized_key: "meshes/armor.nif".to_owned(),
			display_path: path("Meshes/Armor.nif")?,
			resolution_status: ResolutionStatus::Exact,
			effective_result: Some(EffectiveResult::Absent {
				controlling_tombstone: Some(controlling.clone()),
			}),
			provider_stack: vec![winner.clone(), loser.clone()],
			tombstone_effects: vec![
				TombstoneEffect::Controlling {
					tombstone: controlling.clone(),
					suppressed_entries: vec![loser.clone()],
				},
				TombstoneEffect::ShadowedByTombstone {
					tombstone: shadowed.clone(),
					controlling_tombstone: controlling.clone(),
				},
				TombstoneEffect::OverriddenByFile {
					tombstone: shadowed.clone(),
					overriding_file: winner.clone(),
				},
				TombstoneEffect::Orphan { tombstone: orphan },
			],
			content_comparisons: vec![ContentComparison::Unavailable {
				winner: winner.clone(),
				loser: loser.clone(),
			}],
			reasons: vec![
				ResolutionReason::EffectiveFile {
					provider: winner.clone(),
				},
				ResolutionReason::LowerPriorityFile {
					provider: loser.clone(),
					winner: winner.clone(),
				},
				ResolutionReason::SuppressedByTombstone {
					provider: loser,
					controlling_tombstone: controlling.clone(),
				},
				ResolutionReason::AbsentNoEntry,
				ResolutionReason::AbsentByTombstone {
					controlling_tombstone: controlling.clone(),
				},
				ResolutionReason::ShadowedTombstone {
					tombstone: shadowed.clone(),
					controlling_tombstone: controlling,
				},
				ResolutionReason::TombstoneOverriddenByFile {
					tombstone: shadowed,
					overriding_file: winner,
				},
				ResolutionReason::NamespaceInvalid,
			],
			problems: Vec::new(),
		};

		let text = explanation(&output);
		for expected in [
			"display_path = \"Meshes\\\\Armor.nif\"",
			"effective_result.kind = \"absent\"",
			"effective_result.controlling_tombstone.scope = \"directory_subtree\"",
			"provider_stack.count = 2",
			"tombstone_effects[0].kind = \"controlling\"",
			"tombstone_effects[1].kind = \"shadowed_by_tombstone\"",
			"tombstone_effects[2].kind = \"overridden_by_file\"",
			"tombstone_effects[3].kind = \"orphan\"",
			"reasons[0].kind = \"effective_file\"",
			"reasons[1].kind = \"lower_priority_file\"",
			"reasons[2].kind = \"suppressed_by_tombstone\"",
			"reasons[3].kind = \"absent_no_entry\"",
			"reasons[4].kind = \"absent_by_tombstone\"",
			"reasons[5].kind = \"shadowed_tombstone\"",
			"reasons[6].kind = \"tombstone_overridden_by_file\"",
			"reasons[7].kind = \"namespace_invalid\"",
		] {
			assert!(text.contains(expected), "missing {expected}");
		}

		let invalid = explanation(&ExplainPathOutput {
			normalized_key: "meshes/armor.nif".to_owned(),
			display_path: path("Meshes/Armor.nif")?,
			resolution_status: ResolutionStatus::Invalid,
			effective_result: None,
			provider_stack: Vec::new(),
			tombstone_effects: Vec::new(),
			content_comparisons: Vec::new(),
			reasons: vec![ResolutionReason::NamespaceInvalid],
			problems: Vec::new(),
		});
		assert!(!invalid.contains("effective_result"));
		Ok(())
	}

	#[test]
	fn explain_output_renders_file_and_uncontrolled_absence_results() -> Result<(), Box<dyn Error>> {
		let file = explanation(&ExplainPathOutput {
			normalized_key: "menus/main.xml".to_owned(),
			display_path: path("Menus/Main.xml")?,
			resolution_status: ResolutionStatus::Exact,
			effective_result: Some(EffectiveResult::File(steam("Menus/Main.xml")?)),
			provider_stack: Vec::new(),
			tombstone_effects: Vec::new(),
			content_comparisons: Vec::new(),
			reasons: Vec::new(),
			problems: Vec::new(),
		});
		assert!(file.contains("effective_result.kind = \"file\""));
		assert!(file.contains("effective_result.provider.kind = \"steam_data\""));

		let absent = explanation(&ExplainPathOutput {
			normalized_key: "menus/missing.xml".to_owned(),
			display_path: path("Menus/Missing.xml")?,
			resolution_status: ResolutionStatus::Exact,
			effective_result: Some(EffectiveResult::Absent {
				controlling_tombstone: None,
			}),
			provider_stack: Vec::new(),
			tombstone_effects: Vec::new(),
			content_comparisons: Vec::new(),
			reasons: vec![ResolutionReason::AbsentNoEntry],
			problems: Vec::new(),
		});
		assert!(absent.contains("effective_result.kind = \"absent\""));
		assert!(!absent.contains("effective_result.controlling_tombstone"));
		Ok(())
	}
}
