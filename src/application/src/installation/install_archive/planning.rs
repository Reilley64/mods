use crate::errors::ErrorMarker;
use crate::installation::CandidateDecision;
use crate::installation::EffectiveResult;
use crate::installation::InstallWarning;
use crate::installation::LoserReason;
use crate::installation::PlanCandidateReference;
use crate::installation::PlannedCandidate;
use crate::installation::TombstoneScope;
use crate::installation::WinnerReason;
use domain::DataRelativePath;
use domain::InstallCandidate;
use domain::InstallationPhase;
use rootcause::Result;
use rootcause::report;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::HashMap;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct CandidatePrecedence {
	phase: InstallationPhase,
	declared_priority: i32,
	descriptor_order: u64,
}

pub(super) fn plan_candidates(
	candidates: Vec<InstallCandidate>,
	current_winners: &HashMap<DataRelativePath, EffectiveResult>,
	cancellation: &CancellationToken,
) -> Result<(Vec<PlannedCandidate>, Vec<InstallWarning>), ErrorMarker> {
	let mut ids = BTreeSet::new();
	for candidate in &candidates {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}
		if !ids.insert(candidate.candidate_id) {
			return Err(report!(ErrorMarker::ambiguous_install_plan()));
		}
	}

	let precedence = |candidate: &InstallCandidate| CandidatePrecedence {
		phase: candidate.phase,
		declared_priority: candidate.declared_priority,
		descriptor_order: candidate.descriptor_order,
	};

	let mut by_destination = BTreeMap::<String, Vec<usize>>::new();
	for (index, candidate) in candidates.iter().enumerate() {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}
		by_destination
			.entry(candidate.destination.comparison_key().to_owned())
			.or_default()
			.push(index);
	}

	let mut winners = BTreeMap::new();
	let mut warnings = Vec::new();
	for (key, indexes) in &by_destination {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}
		let mut winner_index = None;
		for index in indexes {
			if cancellation.is_cancelled() {
				return Err(report!(ErrorMarker::operation_cancelled()));
			}
			if winner_index
				.is_none_or(|winner| precedence(&candidates[*index]) > precedence(&candidates[winner]))
			{
				winner_index = Some(*index);
			}
		}
		let Some(winner_index) = winner_index else {
			continue;
		};
		let winner = &candidates[winner_index];
		let winner_precedence = precedence(winner);
		let mut loser_candidate_ids = Vec::new();
		for index in indexes {
			if cancellation.is_cancelled() {
				return Err(report!(ErrorMarker::operation_cancelled()));
			}
			if *index == winner_index {
				continue;
			}
			let candidate_precedence = precedence(&candidates[*index]);
			if candidate_precedence == winner_precedence {
				return Err(report!(ErrorMarker::ambiguous_install_plan()));
			}
			if candidate_precedence.phase == winner_precedence.phase
				&& candidate_precedence.declared_priority == winner_precedence.declared_priority
			{
				loser_candidate_ids.push(candidates[*index].candidate_id);
			}
		}
		winners.insert(key.clone(), winner_index);
		if !loser_candidate_ids.is_empty() {
			warnings.push(InstallWarning::FomodEqualPriorityTieResolved {
				destination: winner.destination.clone(),
				phase: winner.phase,
				declared_priority: winner.declared_priority,
				winner_candidate_id: winner.candidate_id,
				loser_candidate_ids,
			});
		}
	}

	for destination in winners.keys() {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}
		for (component_boundary, _) in destination.match_indices('/') {
			if cancellation.is_cancelled() {
				return Err(report!(ErrorMarker::operation_cancelled()));
			}
			if winners.contains_key(&destination[..component_boundary]) {
				return Err(report!(ErrorMarker::unsafe_archive()));
			}
		}
	}

	let mut winner_values = BTreeMap::new();
	let mut winner_reasons = BTreeMap::new();
	for (key, winner_index) in &winners {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}
		let winner = candidates[*winner_index].clone();
		let winner_precedence = precedence(&winner);
		let indexes = &by_destination[key];
		let mut lower_phase = false;
		let mut lower_priority = false;
		for index in indexes {
			if cancellation.is_cancelled() {
				return Err(report!(ErrorMarker::operation_cancelled()));
			}
			let candidate_precedence = precedence(&candidates[*index]);
			lower_phase |= candidate_precedence.phase < winner_precedence.phase;
			lower_priority |= candidate_precedence.declared_priority < winner_precedence.declared_priority;
		}
		let reason = if indexes.len() == 1 {
			WinnerReason::OnlyCandidate
		} else if lower_phase {
			WinnerReason::HigherPhase
		} else if lower_priority {
			WinnerReason::HigherDeclaredPriority
		} else {
			WinnerReason::LaterDescriptorOrder
		};
		winner_values.insert(key.clone(), winner);
		winner_reasons.insert(key.clone(), reason);
	}

	let mut current_winners_by_key = HashMap::with_capacity(current_winners.len());
	for (destination, result) in current_winners {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}
		current_winners_by_key.insert(destination.comparison_key(), result);
	}
	let mut planned = Vec::with_capacity(candidates.len());
	for (index, candidate) in candidates.into_iter().enumerate() {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}
		let key = candidate.destination.comparison_key();
		let winner_index = winners[key];
		let winner = &winner_values[key];
		let candidate_precedence = precedence(&candidate);
		let winner_precedence = precedence(winner);
		let decision = if index == winner_index {
			CandidateDecision::Winner {
				reason: winner_reasons[key],
			}
		} else {
			let reason = if candidate_precedence.phase < winner_precedence.phase {
				LoserReason::LowerPhase
			} else if candidate_precedence.declared_priority < winner_precedence.declared_priority {
				LoserReason::LowerDeclaredPriority
			} else {
				LoserReason::EarlierDescriptorOrder
			};
			CandidateDecision::Loser {
				reason,
				winner_candidate_id: winner.candidate_id,
			}
		};
		let proposed_winner = PlanCandidateReference {
			candidate_id: winner.candidate_id,
			source_member: winner.source_member.clone(),
		};
		let current_winner = if let Some(current_winner) = current_winners.get(&candidate.destination) {
			current_winner.clone()
		} else {
			let mut current_winner = EffectiveResult::Absent {
				controlling_tombstone: None,
			};
			for (component_boundary, _) in key.rmatch_indices('/') {
				if cancellation.is_cancelled() {
					return Err(report!(ErrorMarker::operation_cancelled()));
				}
				let Some(ancestor_result) = current_winners_by_key.get(&key[..component_boundary])
				else {
					continue;
				};
				if matches!(
					ancestor_result,
					EffectiveResult::Absent {
						controlling_tombstone: Some(tombstone)
					} if tombstone.scope == TombstoneScope::DirectorySubtree
				) {
					current_winner = (*ancestor_result).clone();
					break;
				}
			}
			current_winner
		};
		planned.push(PlannedCandidate {
			candidate,
			current_winner,
			proposed_winner,
			decision,
		});
	}

	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}
	Ok((planned, warnings))
}

#[cfg(test)]
mod tests {
	use super::plan_candidates;
	use crate::ErrorCode;
	use crate::installation::CandidateDecision;
	use crate::installation::EffectiveResult;
	use crate::installation::InstallWarning;
	use crate::installation::LoserReason;
	use crate::installation::TombstoneReference;
	use crate::installation::TombstoneScope;
	use crate::installation::WinnerReason;
	use domain::DataRelativePath;
	use domain::InstallCandidate;
	use domain::InstallCandidateOrigin;
	use domain::InstallationPhase;
	use domain::ProviderReference;
	use std::collections::HashMap;
	use std::error::Error;
	use std::result::Result as StdResult;
	use tokio_util::sync::CancellationToken;

	fn path(value: &str) -> StdResult<DataRelativePath, Box<dyn Error>> {
		DataRelativePath::new(value.to_owned()).map_err(|_| "invalid fixture path".into())
	}

	fn candidates(destinations: &[&str]) -> StdResult<Vec<InstallCandidate>, Box<dyn Error>> {
		destinations
			.iter()
			.enumerate()
			.map(|(index, destination)| {
				let sequence = u64::try_from(index + 1)?;
				Ok(InstallCandidate {
					candidate_id: sequence,
					origin: InstallCandidateOrigin::Required,
					phase: InstallationPhase::Required,
					declared_priority: 0,
					descriptor_order: sequence,
					source_member: format!("source-{sequence}"),
					destination: path(destination)?,
				})
			})
			.collect()
	}

	fn tombstone(scope: TombstoneScope, owner_path: &str) -> StdResult<EffectiveResult, Box<dyn Error>> {
		Ok(EffectiveResult::Absent {
			controlling_tombstone: Some(TombstoneReference {
				scope,
				owner: ProviderReference::Overwrite {
					original_path: path(owner_path)?,
				},
			}),
		})
	}

	#[test]
	fn directory_tombstone_controls_descendant_candidate() -> StdResult<(), Box<dyn Error>> {
		let controlling_tombstone = tombstone(TombstoneScope::DirectorySubtree, "owners/directory")?;
		let current_winners = HashMap::from([(path("meshes/weapons")?, controlling_tombstone.clone())]);

		let (planned, _) = plan_candidates(
			candidates(&["meshes/weapons/rifle.nif"])?,
			&current_winners,
			&CancellationToken::new(),
		)
		.map_err(|_| "planning failed")?;

		assert_eq!(planned[0].current_winner, controlling_tombstone);
		Ok(())
	}

	#[test]
	fn exact_file_tombstone_does_not_control_descendant_candidate() -> StdResult<(), Box<dyn Error>> {
		let current_winners = HashMap::from([(
			path("meshes/weapons")?,
			tombstone(TombstoneScope::ExactFile, "owners/exact")?,
		)]);

		let (planned, _) = plan_candidates(
			candidates(&["meshes/weapons/rifle.nif"])?,
			&current_winners,
			&CancellationToken::new(),
		)
		.map_err(|_| "planning failed")?;

		assert_eq!(
			planned[0].current_winner,
			EffectiveResult::Absent {
				controlling_tombstone: None
			}
		);
		Ok(())
	}

	#[test]
	fn most_specific_ancestor_preserves_later_updated_tombstone() -> StdResult<(), Box<dyn Error>> {
		let earlier_tombstone = tombstone(TombstoneScope::DirectorySubtree, "owners/earlier")?;
		let later_tombstone = tombstone(TombstoneScope::DirectorySubtree, "owners/later-broad")?;
		let current_winners = HashMap::from([
			(path("meshes")?, earlier_tombstone),
			(path("meshes/weapons")?, later_tombstone.clone()),
		]);

		let (planned, _) = plan_candidates(
			candidates(&["meshes/weapons/rifle.nif"])?,
			&current_winners,
			&CancellationToken::new(),
		)
		.map_err(|_| "planning failed")?;

		assert_eq!(planned[0].current_winner, later_tombstone);
		Ok(())
	}

	#[test]
	fn directory_tombstone_ancestor_uses_unicode_case_folding() -> StdResult<(), Box<dyn Error>> {
		let controlling_tombstone = tombstone(TombstoneScope::DirectorySubtree, "owners/unicode")?;
		let current_winners = HashMap::from([(path("ÉΣ")?, controlling_tombstone.clone())]);

		let (planned, _) = plan_candidates(
			candidates(&["éς/rifle.nif"])?,
			&current_winners,
			&CancellationToken::new(),
		)
		.map_err(|_| "planning failed")?;

		assert_eq!(planned[0].current_winner, controlling_tombstone);
		Ok(())
	}

	#[test]
	fn unrelated_directory_tombstone_does_not_control_candidate() -> StdResult<(), Box<dyn Error>> {
		let current_winners = HashMap::from([(
			path("meshes")?,
			tombstone(TombstoneScope::DirectorySubtree, "owners/unrelated")?,
		)]);

		let (planned, _) = plan_candidates(
			candidates(&["textures/rifle.dds"])?,
			&current_winners,
			&CancellationToken::new(),
		)
		.map_err(|_| "planning failed")?;

		assert_eq!(
			planned[0].current_winner,
			EffectiveResult::Absent {
				controlling_tombstone: None
			}
		);
		Ok(())
	}

	#[test]
	fn exact_destination_result_wins_over_directory_ancestor() -> StdResult<(), Box<dyn Error>> {
		let exact_result = EffectiveResult::File(ProviderReference::Overwrite {
			original_path: path("owners/exact-file")?,
		});
		let current_winners = HashMap::from([
			(
				path("meshes")?,
				tombstone(TombstoneScope::DirectorySubtree, "owners/ancestor")?,
			),
			(path("meshes/weapons/rifle.nif")?, exact_result.clone()),
		]);

		let (planned, _) = plan_candidates(
			candidates(&["meshes/weapons/rifle.nif"])?,
			&current_winners,
			&CancellationToken::new(),
		)
		.map_err(|_| "planning failed")?;

		assert_eq!(planned[0].current_winner, exact_result);
		Ok(())
	}

	#[test]
	fn winning_file_directory_collisions_fail_in_both_input_orders() -> StdResult<(), Box<dyn Error>> {
		for destinations in [["foo", "foo/bar.txt"], ["foo/bar.txt", "foo"]] {
			let result =
				plan_candidates(candidates(&destinations)?, &HashMap::new(), &CancellationToken::new());
			let Err(report) = result else {
				return Err(format!("collision was accepted for {destinations:?}").into());
			};
			assert!(report.iter_reports().any(|report| report
				.downcast_current_context::<crate::ErrorMarker>()
				.is_some_and(|marker| marker.code() == ErrorCode::UnsafeArchive)));
		}
		Ok(())
	}

	#[test]
	fn winning_file_directory_collision_uses_unicode_case_folding() -> StdResult<(), Box<dyn Error>> {
		let result = plan_candidates(
			candidates(&["ÉΣ", "éς/bar.txt"])?,
			&HashMap::new(),
			&CancellationToken::new(),
		);
		let Err(report) = result else {
			return Err("case-folded collision was accepted".into());
		};
		assert!(report.iter_reports().any(|report| report
			.downcast_current_context::<crate::ErrorMarker>()
			.is_some_and(|marker| marker.code() == ErrorCode::UnsafeArchive)));
		Ok(())
	}

	#[test]
	fn destination_prefix_without_component_boundary_is_valid() -> StdResult<(), Box<dyn Error>> {
		let (planned, warnings) = plan_candidates(
			candidates(&["foo", "foobar"])?,
			&HashMap::new(),
			&CancellationToken::new(),
		)
		.map_err(|_| "planning failed")?;

		assert!(warnings.is_empty());
		assert!(planned.iter().all(|candidate| matches!(
			candidate.decision,
			CandidateDecision::Winner {
				reason: WinnerReason::OnlyCandidate
			}
		)));
		Ok(())
	}

	#[test]
	fn same_destination_still_resolves_by_precedence_and_warns() -> StdResult<(), Box<dyn Error>> {
		let (planned, warnings) =
			plan_candidates(candidates(&["foo", "FOO"])?, &HashMap::new(), &CancellationToken::new())
				.map_err(|_| "planning failed")?;

		assert!(matches!(
			planned[0].decision,
			CandidateDecision::Loser {
				reason: LoserReason::EarlierDescriptorOrder,
				winner_candidate_id: 2
			}
		));
		assert!(matches!(
			planned[1].decision,
			CandidateDecision::Winner {
				reason: WinnerReason::LaterDescriptorOrder
			}
		));
		assert!(matches!(
			warnings.as_slice(),
			[InstallWarning::FomodEqualPriorityTieResolved {
				phase: InstallationPhase::Required,
				declared_priority: 0,
				winner_candidate_id: 2,
				loser_candidate_ids,
				..
			}] if loser_candidate_ids == &[1]
		));
		Ok(())
	}
}
