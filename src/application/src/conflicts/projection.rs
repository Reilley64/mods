use crate::conflicts::ConflictContentRead;
use crate::conflicts::EnvironmentConflictScan;
use crate::conflicts::ExplainPathOutput;
use crate::conflicts::IndexedConflictFile;
use crate::conflicts::IndexedConflictFileId;
use crate::conflicts::InspectModConflictsOutput;
use crate::conflicts::ListEffectiveConflictsOutput;
use crate::conflicts::ScannedConflictProvider;
use crate::errors::ErrorMarker;
use crate::installation::CandidateDecision;
use crate::installation::InstallPlan;
use crate::ports::ReadConflictContent;
use domain::ConflictProblem;
use domain::ConflictProblemKind;
use domain::ConflictRow;
use domain::ContentComparison;
use domain::DataRelativePath;
use domain::EffectiveResult;
use domain::ModName;
use domain::Participation;
use domain::ParticipationReason;
use domain::ProblemScope;
use domain::ProviderClass;
use domain::ProviderIdentity;
use domain::ProviderReference;
use domain::ProviderState;
use domain::ProviderSummary;
use domain::ResolutionReason;
use domain::ResolutionStatus;
use domain::Tombstone;
use domain::TombstoneEffect;
use domain::TombstoneScope;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use rootcause::report;
use std::cmp::Ordering;
use std::cmp::Reverse;
use std::collections::HashMap;
use tokio_util::sync::CancellationToken;

struct Projection {
	files: Vec<IndexedConflictFile>,
	directories: Vec<ProviderReference>,
	tombstones: Vec<Tombstone>,
	problems: Vec<ConflictProblem>,
	participation: Participation,
}

struct ResolvedPath {
	unsuppressed: Vec<IndexedConflictFile>,
	suppressed: Vec<(IndexedConflictFile, Tombstone)>,
}

#[derive(Clone, Copy)]
pub(crate) enum InstallationParticipation {
	Actual,
	HypotheticalEnabled,
}

pub(crate) async fn project_installation(
	mut scan: EnvironmentConflictScan,
	plan: &InstallPlan,
	participation: InstallationParticipation,
	read_content: ReadConflictContent,
	cancellation: CancellationToken,
) -> Result<ListEffectiveConflictsOutput, ErrorMarker> {
	let identity = ProviderIdentity::DataMod {
		mod_name: plan.mod_name.clone(),
		priority: plan.projected_state.priority,
	};
	let reason = match participation {
		InstallationParticipation::HypotheticalEnabled => ParticipationReason::HypotheticalEnabledMod,
		InstallationParticipation::Actual => ParticipationReason::EnabledMod,
	};
	let mut files = Vec::new();
	let mut directories = HashMap::new();
	for candidate in &plan.candidates {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled().with_phase("conflict_scan")));
		}
		if !matches!(candidate.decision, CandidateDecision::Winner { .. }) {
			continue;
		}

		let path = &candidate.candidate.destination;
		let provider = ProviderReference::DataMod {
			mod_name: plan.mod_name.clone(),
			priority: plan.projected_state.priority,
			original_path: path.clone(),
			participation_reason: reason,
		};
		files.push(IndexedConflictFile {
			id: IndexedConflictFileId::new(identity.clone(), path.clone()),
			provider,
		});
		for (boundary, _) in path.as_str().match_indices('/') {
			let directory = DataRelativePath::new(path.as_str()[..boundary].to_owned())
				.context(ErrorMarker::ambiguous_install_plan())?;
			directories.entry(directory.comparison_key().to_owned()).or_insert(
				ProviderReference::DataMod {
					mod_name: plan.mod_name.clone(),
					priority: plan.projected_state.priority,
					original_path: directory,
					participation_reason: reason,
				},
			);
		}
	}

	scan.providers.retain(|provider| {
		!matches!(&provider.identity,
		ProviderIdentity::DataMod { mod_name, .. } if mod_name == &plan.mod_name)
	});
	scan.providers.push(ScannedConflictProvider {
		identity: identity.clone(),
		enabled: plan.projected_state.enabled,
		files,
		directories: directories.into_values().collect(),
		tombstones: Vec::new(),
		problems: Vec::new(),
	});

	let projection = match participation {
		InstallationParticipation::Actual => Projection::actual(scan),
		InstallationParticipation::HypotheticalEnabled => Projection::from_scan(scan, Some(&identity)),
	};
	let rows = projection.rows(false, &read_content, &cancellation, None).await?;

	Ok(ListEffectiveConflictsOutput {
		resolution_status: projection.resolution_status(),
		rows,
		problems: projection.problems,
	})
}

pub(super) async fn project_list(
	scan: EnvironmentConflictScan,
	compare_content: bool,
	read_content: ReadConflictContent,
	cancellation: CancellationToken,
) -> Result<ListEffectiveConflictsOutput, ErrorMarker> {
	let projection = Projection::actual(scan);
	let rows = projection
		.rows(compare_content, &read_content, &cancellation, None)
		.await?;
	Ok(ListEffectiveConflictsOutput {
		resolution_status: projection.resolution_status(),
		rows,
		problems: projection.problems,
	})
}

pub(super) async fn project_inspection(
	scan: EnvironmentConflictScan,
	mod_name: ModName,
	compare_content: bool,
	read_content: ReadConflictContent,
	cancellation: CancellationToken,
) -> Result<InspectModConflictsOutput, ErrorMarker> {
	let Some(provider) = scan.providers.iter().find(|provider| {
		matches!(&provider.identity, ProviderIdentity::DataMod { mod_name: candidate, .. } if candidate == &mod_name)
	}) else {
		return Err(report!(ErrorMarker::mod_not_found().with_mod_name(mod_name.clone())));
	};

	let enabled = provider.enabled;
	let identity = provider.identity.clone();

	let ProviderIdentity::DataMod {
		mod_name: canonical_mod_name,
		..
	} = &identity
	else {
		return Err(report!(ErrorMarker::mod_not_found().with_mod_name(mod_name.clone())));
	};
	let canonical_mod_name = canonical_mod_name.clone();

	let projection = if enabled {
		Projection::actual(scan)
	} else {
		Projection::from_scan(scan, Some(&identity))
	};
	let participation = if enabled {
		Participation::Active
	} else {
		Participation::Hypothetical
	};

	let rows = projection
		.rows(compare_content, &read_content, &cancellation, Some(&identity))
		.await?;
	let provider_summary = projection.summary(&identity, enabled);

	Ok(InspectModConflictsOutput {
		mod_name: canonical_mod_name,
		participation,
		resolution_status: projection.resolution_status(),
		provider_summary,
		rows,
		problems: projection.problems,
	})
}

pub(super) async fn project_path(
	scan: EnvironmentConflictScan,
	path: DataRelativePath,
	compare_content: bool,
	read_content: ReadConflictContent,
	cancellation: CancellationToken,
) -> Result<ExplainPathOutput, ErrorMarker> {
	let projection = Projection::actual(scan);
	let resolution_status = projection.resolution_status();
	let resolved = projection.resolve(path.comparison_key());

	let provider_stack = {
		let mut providers = resolved
			.unsuppressed
			.iter()
			.chain(resolved.suppressed.iter().map(|(file, _)| file))
			.map(|file| file.provider.clone())
			.collect::<Vec<_>>();
		providers.extend(projection
			.directories
			.iter()
			.filter(|directory| directory.original_path().comparison_key() == path.comparison_key())
			.cloned());
		sort_provider_references(&mut providers);
		providers
	};

	let controlling_tombstone = projection
		.tombstones
		.iter()
		.filter(|tombstone| tombstone_applies(tombstone, path.comparison_key()))
		.max_by_key(|tombstone| tombstone.owner.rank())
		.cloned();

	let effective_result = if resolution_status == ResolutionStatus::Exact {
		Some(if let Some(winner) = resolved.unsuppressed.first() {
			EffectiveResult::File(winner.provider.clone())
		} else {
			EffectiveResult::Absent {
				controlling_tombstone: controlling_tombstone.clone(),
			}
		})
	} else {
		None
	};

	let display_path = match &effective_result {
		Some(EffectiveResult::File(provider)) => provider.original_path().clone(),
		Some(EffectiveResult::Absent {
			controlling_tombstone: Some(tombstone),
		}) => tombstone.path().clone(),
		_ => provider_stack
			.first()
			.map_or_else(|| path.clone(), |provider| provider.original_path().clone()),
	};

	let losing = resolved.unsuppressed.iter().skip(1).cloned().collect::<Vec<_>>();
	let content_comparisons = if let Some(winner) = resolved.unsuppressed.first() {
		compare_files(winner, &losing, compare_content, &read_content, &cancellation).await?
	} else {
		Vec::new()
	};

	let mut reasons = Vec::new();
	if resolution_status == ResolutionStatus::Invalid {
		reasons.push(ResolutionReason::NamespaceInvalid);
	} else if let Some(winner) = resolved.unsuppressed.first() {
		reasons.push(ResolutionReason::EffectiveFile {
			provider: winner.provider.clone(),
		});
		for loser in &losing {
			reasons.push(ResolutionReason::LowerPriorityFile {
				provider: loser.provider.clone(),
				winner: winner.provider.clone(),
			});
		}
	} else if let Some(tombstone) = &controlling_tombstone {
		reasons.push(ResolutionReason::AbsentByTombstone {
			controlling_tombstone: tombstone.clone(),
		});
	} else {
		reasons.push(ResolutionReason::AbsentNoEntry);
	}

	let tombstone_effects = projection.tombstone_effects(path.comparison_key(), &resolved);
	for effect in &tombstone_effects {
		match effect {
			TombstoneEffect::Controlling {
				tombstone,
				suppressed_entries,
			} => {
				for provider in suppressed_entries {
					reasons.push(ResolutionReason::SuppressedByTombstone {
						provider: provider.clone(),
						controlling_tombstone: tombstone.clone(),
					});
				}
			}
			TombstoneEffect::ShadowedByTombstone {
				tombstone,
				controlling_tombstone,
			} => reasons.push(ResolutionReason::ShadowedTombstone {
				tombstone: tombstone.clone(),
				controlling_tombstone: controlling_tombstone.clone(),
			}),
			TombstoneEffect::OverriddenByFile {
				tombstone,
				overriding_file,
			} => reasons.push(ResolutionReason::TombstoneOverriddenByFile {
				tombstone: tombstone.clone(),
				overriding_file: overriding_file.clone(),
			}),
			TombstoneEffect::Orphan { .. } => {}
		}
	}

	Ok(ExplainPathOutput {
		normalized_key: path.comparison_key().to_owned(),
		display_path,
		resolution_status,
		effective_result,
		provider_stack,
		tombstone_effects,
		content_comparisons,
		reasons,
		problems: projection.problems,
	})
}

impl Projection {
	fn actual(scan: EnvironmentConflictScan) -> Self {
		Self::from_scan(scan, None)
	}

	fn from_scan(scan: EnvironmentConflictScan, hypothetical: Option<&ProviderIdentity>) -> Self {
		let participation = if hypothetical.is_some() {
			Participation::Hypothetical
		} else {
			Participation::Active
		};

		let mut providers = Vec::new();
		for mut provider in scan.providers {
			let selected = hypothetical.is_some_and(|identity| identity == &provider.identity);
			if !provider.enabled && !selected {
				continue;
			}
			if selected {
				for file in &mut provider.files {
					file.provider = file
						.provider
						.with_participation_reason(ParticipationReason::HypotheticalEnabledMod);
				}
				for directory in &mut provider.directories {
					*directory = directory
						.with_participation_reason(ParticipationReason::HypotheticalEnabledMod);
				}
				for tombstone in &mut provider.tombstones {
					tombstone.owner = tombstone
						.owner
						.with_participation_reason(ParticipationReason::HypotheticalEnabledMod);
				}
			}
			providers.push(provider);
		}

		providers.sort_by_key(|provider| provider.identity.rank());

		let mut problems = scan.problems;
		for provider in &providers {
			problems.extend(provider.problems.iter().cloned());
		}

		let files = providers
			.iter()
			.flat_map(|provider| provider.files.iter().cloned())
			.collect::<Vec<_>>();
		let directories = providers
			.iter()
			.flat_map(|provider| provider.directories.iter().cloned())
			.collect::<Vec<_>>();
		let mut tombstones = providers
			.iter()
			.flat_map(|provider| provider.tombstones.iter().cloned())
			.collect::<Vec<_>>();
		tombstones.sort_by(|left, right| {
			right.owner
				.rank()
				.cmp(&left.owner.rank())
				.then_with(|| {
					compare_utf16(left.path().comparison_key(), right.path().comparison_key())
				})
				.then_with(|| compare_utf16(left.path().as_str(), right.path().as_str()))
		});

		append_structural_problems(&files, &directories, &mut problems);
		problems.sort_by(compare_problems);
		problems.dedup();

		Self {
			files,
			directories,
			tombstones,
			problems,
			participation,
		}
	}

	fn resolution_status(&self) -> ResolutionStatus {
		if self.problems.is_empty() {
			ResolutionStatus::Exact
		} else {
			ResolutionStatus::Invalid
		}
	}

	fn resolve(&self, key: &str) -> ResolvedPath {
		let mut unsuppressed = Vec::new();
		let mut suppressed = Vec::new();
		for file in self
			.files
			.iter()
			.filter(|file| file.provider.original_path().comparison_key() == key)
		{
			if let Some(tombstone) = controlling_tombstone(&self.tombstones, &file.provider) {
				suppressed.push((file.clone(), tombstone.clone()));
			} else {
				unsuppressed.push(file.clone());
			}
		}
		unsuppressed.sort_by_key(|file| Reverse(file.provider.rank()));
		suppressed.sort_by_key(|(file, _)| Reverse(file.provider.rank()));
		ResolvedPath {
			unsuppressed,
			suppressed,
		}
	}

	async fn rows(
		&self,
		compare_content: bool,
		read_content: &ReadConflictContent,
		cancellation: &CancellationToken,
		selected: Option<&ProviderIdentity>,
	) -> Result<Vec<ConflictRow>, ErrorMarker> {
		if self.resolution_status() == ResolutionStatus::Invalid {
			return Ok(Vec::new());
		}

		let mut keys = self
			.files
			.iter()
			.map(|file| file.provider.original_path().comparison_key().to_owned())
			.collect::<Vec<_>>();
		keys.sort_by(|left, right| compare_utf16(left, right));
		keys.dedup();

		let mut rows = Vec::new();
		for key in keys {
			let resolved = self.resolve(&key);
			let non_base = resolved
				.unsuppressed
				.iter()
				.filter(|file| file.provider.class() != ProviderClass::SteamData)
				.cloned()
				.collect::<Vec<_>>();
			if non_base.len() < 2 {
				continue;
			}
			let winner = &non_base[0];
			let losers = non_base.iter().skip(1).cloned().collect::<Vec<_>>();
			let selected_participates = selected.is_none_or(|identity| {
				&winner.provider.identity() == identity
					|| losers.iter().any(|loser| &loser.provider.identity() == identity)
			});
			if !selected_participates {
				continue;
			}
			let content_comparisons =
				compare_files(winner, &losers, compare_content, read_content, cancellation).await?;
			rows.push(ConflictRow::OrdinaryConflict {
				normalized_key: key,
				display_path: winner.provider.original_path().clone(),
				participation: self.participation,
				effective_file: winner.provider.clone(),
				losing_files: losers.into_iter().map(|file| file.provider).collect(),
				content_comparisons,
			});
		}

		for tombstone in &self.tombstones {
			let mut suppressed_entries = self
				.files
				.iter()
				.map(|file| &file.provider)
				.chain(self.directories.iter())
				.filter(|entry| {
					controlling_tombstone(&self.tombstones, entry)
						.is_some_and(|controlling| controlling == tombstone)
				})
				.cloned()
				.collect::<Vec<_>>();
			if suppressed_entries.is_empty() {
				continue;
			}
			sort_provider_references(&mut suppressed_entries);
			let selected_participates = selected.is_none_or(|identity| {
				&tombstone.owner.identity() == identity
					|| suppressed_entries.iter().any(|entry| &entry.identity() == identity)
			});
			if !selected_participates {
				continue;
			}
			rows.push(ConflictRow::Tombstone {
				normalized_key: tombstone.path().comparison_key().to_owned(),
				display_path: tombstone.path().clone(),
				participation: self.participation,
				controlling_tombstone: tombstone.clone(),
				suppressed_entries,
			});
		}

		rows.sort_by(compare_rows);

		Ok(rows)
	}

	fn summary(&self, identity: &ProviderIdentity, was_enabled: bool) -> ProviderSummary {
		let files = self
			.files
			.iter()
			.filter(|file| &file.provider.identity() == identity)
			.collect::<Vec<_>>();
		let tombstones = self
			.tombstones
			.iter()
			.filter(|tombstone| &tombstone.owner.identity() == identity)
			.collect::<Vec<_>>();
		let mut effective_file_count = 0;
		let mut file_conflict_win_count = 0;
		let mut file_conflict_loss_count = 0;
		let mut own_file_entries_suppressed_count = 0;
		for file in &files {
			let resolved = self.resolve(file.provider.original_path().comparison_key());
			if resolved.suppressed.iter().any(|(candidate, _)| candidate.id == file.id) {
				own_file_entries_suppressed_count += 1;
				continue;
			}
			let non_base = resolved
				.unsuppressed
				.iter()
				.filter(|candidate| candidate.provider.class() != ProviderClass::SteamData)
				.collect::<Vec<_>>();
			if resolved.unsuppressed.first().is_some_and(|winner| winner.id == file.id) {
				effective_file_count += 1;
				if non_base.len() >= 2 {
					file_conflict_win_count += 1;
				}
			} else if non_base.len() >= 2 {
				file_conflict_loss_count += 1;
			}
		}
		let lower_file_entries_suppressed_count = self
			.files
			.iter()
			.filter(|file| {
				controlling_tombstone(&self.tombstones, &file.provider)
					.is_some_and(|tombstone| &tombstone.owner.identity() == identity)
			})
			.count() as u64;
		let ordinary_file_count = files.len() as u64;
		let owned_file_tombstone_count = tombstones
			.iter()
			.filter(|tombstone| tombstone.scope == TombstoneScope::ExactFile)
			.count() as u64;
		let owned_directory_tombstone_count = tombstones
			.iter()
			.filter(|tombstone| tombstone.scope == TombstoneScope::DirectorySubtree)
			.count() as u64;
		let state = if self.resolution_status() == ResolutionStatus::Invalid {
			ProviderState::Invalid
		} else if !was_enabled {
			ProviderState::Inactive
		} else if ordinary_file_count == 0 && !tombstones.is_empty() {
			ProviderState::SuppressionOnly
		} else if ordinary_file_count == 0 {
			ProviderState::Empty
		} else if effective_file_count == 0 {
			ProviderState::FullyOverridden
		} else if file_conflict_win_count != 0 && file_conflict_loss_count != 0 {
			ProviderState::Mixed
		} else if file_conflict_win_count != 0 {
			ProviderState::WinningConflicts
		} else if file_conflict_loss_count != 0 {
			ProviderState::LosingConflicts
		} else {
			ProviderState::Uncontested
		};
		ProviderSummary {
			ordinary_file_count,
			effective_file_count,
			file_conflict_win_count,
			file_conflict_loss_count,
			owned_file_tombstone_count,
			owned_directory_tombstone_count,
			lower_file_entries_suppressed_count,
			own_file_entries_suppressed_count,
			state,
		}
	}

	fn tombstone_effects(&self, key: &str, resolved: &ResolvedPath) -> Vec<TombstoneEffect> {
		let applicable = self
			.tombstones
			.iter()
			.filter(|tombstone| tombstone_applies(tombstone, key))
			.collect::<Vec<_>>();
		let mut effects = Vec::new();
		for tombstone in applicable {
			let mut suppressed_entries = resolved
				.suppressed
				.iter()
				.filter(|(_, controlling)| controlling == tombstone)
				.map(|(file, _)| file.provider.clone())
				.collect::<Vec<_>>();
			suppressed_entries.extend(self
				.directories
				.iter()
				.filter(|directory| directory.original_path().comparison_key() == key)
				.filter(|directory| {
					controlling_tombstone(&self.tombstones, directory)
						.is_some_and(|controlling| controlling == tombstone)
				})
				.cloned());
			sort_provider_references(&mut suppressed_entries);
			if !suppressed_entries.is_empty() {
				effects.push(TombstoneEffect::Controlling {
					tombstone: tombstone.clone(),
					suppressed_entries,
				});
				continue;
			}
			if let Some(controlling) = self
				.tombstones
				.iter()
				.filter(|candidate| {
					candidate.owner.rank() > tombstone.owner.rank()
						&& tombstone_applies(candidate, key)
				})
				.max_by_key(|candidate| candidate.owner.rank())
			{
				effects.push(TombstoneEffect::ShadowedByTombstone {
					tombstone: tombstone.clone(),
					controlling_tombstone: controlling.clone(),
				});
			} else if let Some(overriding) = resolved
				.unsuppressed
				.iter()
				.find(|file| file.provider.rank() > tombstone.owner.rank())
			{
				effects.push(TombstoneEffect::OverriddenByFile {
					tombstone: tombstone.clone(),
					overriding_file: overriding.provider.clone(),
				});
			} else {
				effects.push(TombstoneEffect::Orphan {
					tombstone: tombstone.clone(),
				});
			}
		}
		effects
	}
}

fn append_structural_problems(
	files: &[IndexedConflictFile],
	directories: &[ProviderReference],
	problems: &mut Vec<ConflictProblem>,
) {
	let mut reported = Vec::new();
	for file in files {
		let file_path = file.provider.original_path();
		let file_key = file_path.comparison_key();
		let collides = directories
			.iter()
			.any(|directory| key_is_at_or_below(directory.original_path().comparison_key(), file_key))
			|| files.iter().any(|candidate| {
				candidate.id != file.id
					&& key_is_below(candidate.provider.original_path().comparison_key(), file_key)
			});
		if collides && !reported.iter().any(|key| key == file_key) {
			reported.push(file_key.to_owned());
			problems.push(ConflictProblem {
				kind: ConflictProblemKind::FileDirectoryCollision,
				scope: ProblemScope::Path {
					normalized_key: file_key.to_owned(),
					display_path: file_path.clone(),
				},
			});
		}
	}
}

fn controlling_tombstone<'a>(tombstones: &'a [Tombstone], entry: &ProviderReference) -> Option<&'a Tombstone> {
	tombstones
		.iter()
		.filter(|tombstone| {
			tombstone.owner.rank() > entry.rank()
				&& tombstone_applies(tombstone, entry.original_path().comparison_key())
		})
		.max_by_key(|tombstone| tombstone.owner.rank())
}

fn tombstone_applies(tombstone: &Tombstone, key: &str) -> bool {
	let tombstone_key = tombstone.path().comparison_key();
	match tombstone.scope {
		TombstoneScope::ExactFile => key == tombstone_key,
		TombstoneScope::DirectorySubtree => key_is_at_or_below(key, tombstone_key),
	}
}

fn key_is_at_or_below(key: &str, boundary: &str) -> bool {
	key == boundary || key_is_below(key, boundary)
}

fn key_is_below(key: &str, boundary: &str) -> bool {
	key.strip_prefix(boundary).is_some_and(|suffix| suffix.starts_with('/'))
}

async fn compare_files(
	winner: &IndexedConflictFile,
	losers: &[IndexedConflictFile],
	compare_content: bool,
	read_content: &ReadConflictContent,
	cancellation: &CancellationToken,
) -> Result<Vec<ContentComparison>, ErrorMarker> {
	if !compare_content {
		return Ok(losers
			.iter()
			.map(|loser| ContentComparison::NotCompared {
				winner: winner.provider.clone(),
				loser: loser.provider.clone(),
			})
			.collect());
	}

	let winner_content = read_content.call((winner.id.clone(), cancellation.clone())).await?;
	let mut comparisons = Vec::new();
	for loser in losers {
		let loser_content = read_content.call((loser.id.clone(), cancellation.clone())).await?;
		let comparison = match (&winner_content, &loser_content) {
			(ConflictContentRead::Unstable, _) | (_, ConflictContentRead::Unstable) => {
				ContentComparison::Unstable {
					winner: winner.provider.clone(),
					loser: loser.provider.clone(),
				}
			}
			(ConflictContentRead::Unavailable, _) | (_, ConflictContentRead::Unavailable) => {
				ContentComparison::Unavailable {
					winner: winner.provider.clone(),
					loser: loser.provider.clone(),
				}
			}
			(ConflictContentRead::Sha256(winner_sha256), ConflictContentRead::Sha256(loser_sha256))
				if winner_sha256 == loser_sha256 =>
			{
				ContentComparison::SameSha256 {
					winner: winner.provider.clone(),
					loser: loser.provider.clone(),
					winner_sha256: winner_sha256.clone(),
					loser_sha256: loser_sha256.clone(),
				}
			}
			(ConflictContentRead::Sha256(winner_sha256), ConflictContentRead::Sha256(loser_sha256)) => {
				ContentComparison::DifferentSha256 {
					winner: winner.provider.clone(),
					loser: loser.provider.clone(),
					winner_sha256: winner_sha256.clone(),
					loser_sha256: loser_sha256.clone(),
				}
			}
		};
		comparisons.push(comparison);
	}
	Ok(comparisons)
}

fn sort_provider_references(values: &mut [ProviderReference]) {
	values.sort_by(|left, right| {
		right.rank()
			.cmp(&left.rank())
			.then_with(|| {
				compare_utf16(
					left.original_path().comparison_key(),
					right.original_path().comparison_key(),
				)
			})
			.then_with(|| compare_utf16(left.original_path().as_str(), right.original_path().as_str()))
	});
}

fn compare_problems(left: &ConflictProblem, right: &ConflictProblem) -> Ordering {
	(left.kind as u8)
		.cmp(&(right.kind as u8))
		.then_with(|| compare_problem_scopes(&left.scope, &right.scope))
}

fn compare_problem_scopes(left: &ProblemScope, right: &ProblemScope) -> Ordering {
	let left_kind = problem_scope_order(left);
	let right_kind = problem_scope_order(right);
	left_kind.cmp(&right_kind).then_with(|| match (left, right) {
		(ProblemScope::Provider(left), ProblemScope::Provider(right)) => left
			.rank()
			.cmp(&right.rank())
			.then_with(|| compare_provider_identity_names(left, right)),
		(
			ProblemScope::Path {
				normalized_key: left_key,
				display_path: left_path,
			},
			ProblemScope::Path {
				normalized_key: right_key,
				display_path: right_path,
			},
		)
		| (
			ProblemScope::Subtree {
				normalized_key: left_key,
				display_path: left_path,
			},
			ProblemScope::Subtree {
				normalized_key: right_key,
				display_path: right_path,
			},
		) => compare_utf16(left_key, right_key)
			.then_with(|| compare_utf16(left_path.as_str(), right_path.as_str())),
		_ => Ordering::Equal,
	})
}

fn problem_scope_order(scope: &ProblemScope) -> u8 {
	match scope {
		ProblemScope::Global => 0,
		ProblemScope::Modlist => 1,
		ProblemScope::Provider(_) => 2,
		ProblemScope::Path { .. } => 3,
		ProblemScope::Subtree { .. } => 4,
	}
}

fn compare_provider_identity_names(left: &ProviderIdentity, right: &ProviderIdentity) -> Ordering {
	let (
		ProviderIdentity::DataMod {
			mod_name: left_name, ..
		},
		ProviderIdentity::DataMod {
			mod_name: right_name, ..
		},
	) = (left, right)
	else {
		return Ordering::Equal;
	};

	compare_utf16(left_name.as_str(), right_name.as_str())
}

fn compare_rows(left: &ConflictRow, right: &ConflictRow) -> Ordering {
	let (left_key, left_path, left_kind) = row_order(left);
	let (right_key, right_path, right_kind) = row_order(right);
	compare_utf16(left_key, right_key)
		.then_with(|| compare_utf16(left_path.as_str(), right_path.as_str()))
		.then_with(|| left_kind.cmp(&right_kind))
}

fn row_order(row: &ConflictRow) -> (&str, &DataRelativePath, u8) {
	match row {
		ConflictRow::OrdinaryConflict {
			normalized_key,
			display_path,
			..
		} => (normalized_key, display_path, 0),
		ConflictRow::Tombstone {
			normalized_key,
			display_path,
			..
		} => (normalized_key, display_path, 1),
	}
}

fn compare_utf16(left: &str, right: &str) -> Ordering {
	left.encode_utf16().cmp(right.encode_utf16())
}

#[cfg(test)]
#[expect(
	clippy::expect_used,
	reason = "fixture construction and output assertions require known-success values"
)]
mod tests {
	use super::project_inspection;
	use super::project_list;
	use super::project_path;
	use crate::ErrorMarker;
	use crate::conflicts::ConflictContentRead;
	use crate::conflicts::EnvironmentConflictScan;
	use crate::conflicts::IndexedConflictFile;
	use crate::conflicts::IndexedConflictFileId;
	use crate::conflicts::InspectModConflictsOutput;
	use crate::conflicts::ScannedConflictProvider;
	use crate::ports::PortFuture;
	use crate::ports::ReadConflictContent;
	use domain::ConflictProblem;
	use domain::ConflictProblemKind;
	use domain::ConflictRow;
	use domain::ContentState;
	use domain::DataRelativePath;
	use domain::EffectiveResult;
	use domain::ModName;
	use domain::ModPriority;
	use domain::Participation;
	use domain::ParticipationReason;
	use domain::ProblemScope;
	use domain::ProviderClass;
	use domain::ProviderIdentity;
	use domain::ProviderRank;
	use domain::ProviderReference;
	use domain::ProviderState;
	use domain::ResolutionStatus;
	use domain::Sha256Digest;
	use domain::Tombstone;
	use domain::TombstoneScope;
	use rootcause::Result;
	use std::sync::Arc;
	use std::sync::Mutex;
	use std::sync::atomic::AtomicUsize;
	use std::sync::atomic::Ordering;
	use tokio_util::sync::CancellationToken;

	fn path(value: &str) -> DataRelativePath {
		DataRelativePath::new(value.to_owned()).expect("path fixture")
	}

	fn mod_name(value: &str) -> ModName {
		ModName::new(value.to_owned()).expect("mod fixture")
	}

	fn data_mod_file(
		name: &str,
		priority: u32,
		path_value: &str,
		reason: ParticipationReason,
	) -> IndexedConflictFile {
		let mod_name = mod_name(name);
		let path = path(path_value);
		let identity = ProviderIdentity::DataMod {
			mod_name: mod_name.clone(),
			priority: ModPriority::new(priority),
		};
		IndexedConflictFile {
			id: IndexedConflictFileId::new(identity, path.clone()),
			provider: ProviderReference::DataMod {
				mod_name,
				priority: ModPriority::new(priority),
				original_path: path,
				participation_reason: reason,
			},
		}
	}

	fn steam_file(path_value: &str) -> IndexedConflictFile {
		let path = path(path_value);
		IndexedConflictFile {
			id: IndexedConflictFileId::new(ProviderIdentity::SteamData, path.clone()),
			provider: ProviderReference::SteamData { original_path: path },
		}
	}

	fn overwrite_file(path_value: &str) -> IndexedConflictFile {
		let path = path(path_value);
		IndexedConflictFile {
			id: IndexedConflictFileId::new(ProviderIdentity::Overwrite, path.clone()),
			provider: ProviderReference::Overwrite { original_path: path },
		}
	}

	fn fixed_provider(
		identity: ProviderIdentity,
		enabled: bool,
		files: Vec<IndexedConflictFile>,
	) -> ScannedConflictProvider {
		ScannedConflictProvider {
			identity,
			enabled,
			files,
			directories: Vec::new(),
			tombstones: Vec::new(),
			problems: Vec::new(),
		}
	}

	fn provider(
		name: &str,
		priority: u32,
		enabled: bool,
		files: Vec<IndexedConflictFile>,
		tombstones: Vec<Tombstone>,
	) -> ScannedConflictProvider {
		ScannedConflictProvider {
			identity: ProviderIdentity::DataMod {
				mod_name: mod_name(name),
				priority: ModPriority::new(priority),
			},
			enabled,
			files,
			directories: Vec::new(),
			tombstones,
			problems: Vec::new(),
		}
	}

	fn content_port(calls: Arc<AtomicUsize>) -> ReadConflictContent {
		Arc::new(move |_, _| {
			calls.fetch_add(1, Ordering::SeqCst);
			Box::pin(async {
				Ok(ConflictContentRead::Sha256(
					Sha256Digest::new("a".repeat(64)).expect("digest fixture"),
				))
			}) as PortFuture<_, ErrorMarker>
		})
	}

	fn owned_tombstone(name: &str, priority: u32, path_value: &str, scope: TombstoneScope) -> Tombstone {
		Tombstone {
			scope,
			owner: ProviderReference::DataMod {
				mod_name: mod_name(name),
				priority: ModPriority::new(priority),
				original_path: path(path_value),
				participation_reason: ParticipationReason::EnabledMod,
			},
		}
	}

	async fn inspect(
		scan: EnvironmentConflictScan,
		name: &str,
		compare_content: bool,
		calls: Arc<AtomicUsize>,
	) -> Result<InspectModConflictsOutput, ErrorMarker> {
		project_inspection(
			scan,
			mod_name(name),
			compare_content,
			content_port(calls),
			CancellationToken::new(),
		)
		.await
	}

	#[tokio::test]
	async fn list_orders_case_insensitive_contenders_and_does_not_hash_by_default() -> Result<(), ErrorMarker> {
		let calls = Arc::new(AtomicUsize::new(0));
		let scan = EnvironmentConflictScan {
			providers: vec![
				provider(
					"Low",
					0,
					true,
					vec![data_mod_file(
						"Low",
						0,
						"Textures/A.dds",
						ParticipationReason::EnabledMod,
					)],
					Vec::new(),
				),
				provider(
					"High",
					1,
					true,
					vec![data_mod_file(
						"High",
						1,
						"textures/a.DDS",
						ParticipationReason::EnabledMod,
					)],
					Vec::new(),
				),
			],
			problems: Vec::new(),
		};

		let output = project_list(scan, false, content_port(calls.clone()), CancellationToken::new()).await?;

		assert!(matches!(output.rows[0], ConflictRow::OrdinaryConflict { .. }));
		let ConflictRow::OrdinaryConflict {
			effective_file,
			losing_files,
			content_comparisons,
			..
		} = &output.rows[0]
		else {
			return Ok(());
		};
		assert_eq!(
			effective_file.identity(),
			ProviderIdentity::DataMod {
				mod_name: mod_name("High"),
				priority: ModPriority::new(1),
			}
		);
		assert_eq!(losing_files.len(), 1);
		assert_eq!(content_comparisons[0].state(), ContentState::NotCompared);
		assert_eq!(calls.load(Ordering::SeqCst), 0);
		Ok(())
	}

	#[tokio::test]
	async fn multiple_losers_read_each_contender_once() -> Result<(), ErrorMarker> {
		let files = vec![
			data_mod_file("Low", 0, "same.txt", ParticipationReason::EnabledMod),
			data_mod_file("Middle", 1, "same.txt", ParticipationReason::EnabledMod),
			data_mod_file("High", 2, "same.txt", ParticipationReason::EnabledMod),
		];
		let scan = EnvironmentConflictScan {
			providers: files
				.iter()
				.map(|file| fixed_provider(file.provider.identity(), true, vec![file.clone()]))
				.collect(),
			problems: Vec::new(),
		};
		let calls = Arc::new(Mutex::new(Vec::new()));
		let recorded_calls = calls.clone();
		let read_content: ReadConflictContent = Arc::new(move |id, _| {
			recorded_calls.lock().expect("record reads").push(id);
			Box::pin(async {
				Ok(ConflictContentRead::Sha256(
					Sha256Digest::new("a".repeat(64)).expect("digest fixture"),
				))
			}) as PortFuture<_, ErrorMarker>
		});

		let output = project_list(scan, true, read_content, CancellationToken::new()).await?;

		assert_eq!(output.rows.len(), 1);
		let ConflictRow::OrdinaryConflict {
			effective_file,
			losing_files,
			content_comparisons,
			..
		} = &output.rows[0]
		else {
			panic!("expected ordinary conflict");
		};
		assert_eq!(effective_file, &files[2].provider);
		assert_eq!(
			losing_files,
			&vec![files[1].provider.clone(), files[0].provider.clone()]
		);
		assert_eq!(
			content_comparisons
				.iter()
				.map(|comparison| comparison.state())
				.collect::<Vec<_>>(),
			vec![ContentState::SameSha256, ContentState::SameSha256]
		);
		assert_eq!(
			*calls.lock().expect("recorded reads"),
			vec![files[2].id.clone(), files[1].id.clone(), files[0].id.clone()]
		);
		Ok(())
	}

	#[tokio::test]
	async fn complete_hashes_are_reused_for_each_winner_loser_pair() -> Result<(), ErrorMarker> {
		let calls = Arc::new(AtomicUsize::new(0));
		let scan = EnvironmentConflictScan {
			providers: vec![
				provider(
					"Low",
					0,
					true,
					vec![data_mod_file("Low", 0, "same.txt", ParticipationReason::EnabledMod)],
					Vec::new(),
				),
				provider(
					"High",
					1,
					true,
					vec![data_mod_file("High", 1, "same.txt", ParticipationReason::EnabledMod)],
					Vec::new(),
				),
			],
			problems: Vec::new(),
		};

		let output = project_list(scan, true, content_port(calls.clone()), CancellationToken::new()).await?;
		assert!(matches!(output.rows[0], ConflictRow::OrdinaryConflict { .. }));
		let ConflictRow::OrdinaryConflict {
			content_comparisons, ..
		} = &output.rows[0]
		else {
			return Ok(());
		};
		assert_eq!(content_comparisons[0].state(), ContentState::SameSha256);
		assert_eq!(calls.load(Ordering::SeqCst), 2);
		Ok(())
	}

	#[tokio::test]
	async fn disabled_mod_is_inserted_hypothetically_at_its_stored_priority() -> Result<(), ErrorMarker> {
		let selected = mod_name("Disabled");
		let scan = EnvironmentConflictScan {
			providers: vec![
				provider(
					"Enabled",
					0,
					true,
					vec![data_mod_file(
						"Enabled",
						0,
						"shared.txt",
						ParticipationReason::EnabledMod,
					)],
					Vec::new(),
				),
				provider(
					"Disabled",
					1,
					false,
					vec![data_mod_file(
						"Disabled",
						1,
						"shared.txt",
						ParticipationReason::DisabledMod,
					)],
					Vec::new(),
				),
			],
			problems: Vec::new(),
		};

		let output = project_inspection(
			scan,
			selected,
			false,
			content_port(Arc::new(AtomicUsize::new(0))),
			CancellationToken::new(),
		)
		.await?;

		assert_eq!(output.participation, Participation::Hypothetical);
		assert_eq!(output.provider_summary.state, ProviderState::Inactive);
		assert_eq!(output.rows.len(), 1);
		Ok(())
	}

	#[tokio::test]
	async fn directory_tombstone_controls_descendant_file() -> Result<(), ErrorMarker> {
		let tombstone = Tombstone {
			scope: TombstoneScope::DirectorySubtree,
			owner: ProviderReference::DataMod {
				mod_name: mod_name("High"),
				priority: ModPriority::new(1),
				original_path: path("textures/old"),
				participation_reason: ParticipationReason::EnabledMod,
			},
		};
		let scan = EnvironmentConflictScan {
			providers: vec![
				provider(
					"Low",
					0,
					true,
					vec![data_mod_file(
						"Low",
						0,
						"Textures/Old/a.dds",
						ParticipationReason::EnabledMod,
					)],
					Vec::new(),
				),
				provider("High", 1, true, Vec::new(), vec![tombstone]),
			],
			problems: Vec::new(),
		};

		let output = project_list(
			scan,
			false,
			content_port(Arc::new(AtomicUsize::new(0))),
			CancellationToken::new(),
		)
		.await?;

		assert!(matches!(output.rows[0], ConflictRow::Tombstone { .. }));
		Ok(())
	}

	#[tokio::test]
	async fn ordinary_and_exact_tombstone_rows_share_a_key_in_stable_kind_order() -> Result<(), ErrorMarker> {
		let tombstone = Tombstone {
			scope: TombstoneScope::ExactFile,
			owner: ProviderReference::DataMod {
				mod_name: mod_name("Suppressor"),
				priority: ModPriority::new(1),
				original_path: path("shared.txt"),
				participation_reason: ParticipationReason::EnabledMod,
			},
		};
		let scan = EnvironmentConflictScan {
			providers: vec![
				provider(
					"Suppressed",
					0,
					true,
					vec![data_mod_file(
						"Suppressed",
						0,
						"shared.txt",
						ParticipationReason::EnabledMod,
					)],
					Vec::new(),
				),
				provider("Suppressor", 1, true, Vec::new(), vec![tombstone]),
				provider(
					"Higher",
					2,
					true,
					vec![data_mod_file(
						"Higher",
						2,
						"shared.txt",
						ParticipationReason::EnabledMod,
					)],
					Vec::new(),
				),
				provider(
					"Winner",
					3,
					true,
					vec![data_mod_file(
						"Winner",
						3,
						"shared.txt",
						ParticipationReason::EnabledMod,
					)],
					Vec::new(),
				),
			],
			problems: Vec::new(),
		};

		let output = project_list(
			scan,
			false,
			content_port(Arc::new(AtomicUsize::new(0))),
			CancellationToken::new(),
		)
		.await?;

		assert_eq!(output.rows.len(), 2);
		assert!(matches!(output.rows[0], ConflictRow::OrdinaryConflict { .. }));
		assert!(matches!(output.rows[1], ConflictRow::Tombstone { .. }));
		Ok(())
	}

	#[tokio::test]
	async fn overwrite_wins_after_enabled_mods_and_disabled_priorities_remain_gaps() -> Result<(), ErrorMarker> {
		let scan = EnvironmentConflictScan {
			providers: vec![
				fixed_provider(ProviderIdentity::SteamData, true, vec![steam_file("Shared/File.txt")]),
				provider(
					"Low",
					0,
					true,
					vec![data_mod_file(
						"Low",
						0,
						"shared/file.TXT",
						ParticipationReason::EnabledMod,
					)],
					Vec::new(),
				),
				provider(
					"Disabled",
					1,
					false,
					vec![data_mod_file(
						"Disabled",
						1,
						"shared/file.txt",
						ParticipationReason::DisabledMod,
					)],
					Vec::new(),
				),
				provider(
					"High",
					2,
					true,
					vec![data_mod_file(
						"High",
						2,
						"shared/file.txt",
						ParticipationReason::EnabledMod,
					)],
					Vec::new(),
				),
				fixed_provider(
					ProviderIdentity::Overwrite,
					true,
					vec![overwrite_file("SHARED/file.txt")],
				),
			],
			problems: Vec::new(),
		};

		let output = project_list(
			scan,
			false,
			content_port(Arc::new(AtomicUsize::new(0))),
			CancellationToken::new(),
		)
		.await?;
		let ConflictRow::OrdinaryConflict {
			effective_file,
			losing_files,
			..
		} = &output.rows[0]
		else {
			return Ok(());
		};

		assert!(matches!(effective_file, ProviderReference::Overwrite { .. }));
		assert_eq!(losing_files.len(), 2);
		assert!(matches!(
			losing_files[0].identity(),
			ProviderIdentity::DataMod { priority, .. } if priority == ModPriority::new(2)
		));
		assert!(losing_files
			.iter()
			.all(|provider| provider.class() != ProviderClass::SteamData));
		Ok(())
	}

	#[tokio::test]
	async fn content_read_failures_are_unavailable_and_different_complete_hashes_remain_conflicts()
	-> Result<(), ErrorMarker> {
		let scan = EnvironmentConflictScan {
			providers: vec![
				provider(
					"Low",
					0,
					true,
					vec![data_mod_file("Low", 0, "same.txt", ParticipationReason::EnabledMod)],
					Vec::new(),
				),
				provider(
					"High",
					1,
					true,
					vec![data_mod_file("High", 1, "same.txt", ParticipationReason::EnabledMod)],
					Vec::new(),
				),
			],
			problems: Vec::new(),
		};
		let unavailable: ReadConflictContent = Arc::new(|id, _| {
			Box::pin(async move {
				if id.identity().rank() == ProviderRank::Regular(ModPriority::new(0)) {
					Ok(ConflictContentRead::Unavailable)
				} else {
					Ok(ConflictContentRead::Sha256(
						Sha256Digest::new("a".repeat(64)).expect("digest fixture"),
					))
				}
			}) as PortFuture<_, ErrorMarker>
		});
		let unavailable_output =
			project_list(scan.clone(), true, unavailable, CancellationToken::new()).await?;
		let different: ReadConflictContent = Arc::new(|id, _| {
			Box::pin(async move {
				let value = if id.identity().rank() == ProviderRank::Regular(ModPriority::new(0)) {
					"a"
				} else {
					"b"
				};
				Ok(ConflictContentRead::Sha256(
					Sha256Digest::new(value.repeat(64)).expect("digest fixture"),
				))
			}) as PortFuture<_, ErrorMarker>
		});
		let unstable: ReadConflictContent = Arc::new(|_, _| {
			Box::pin(async { Ok(ConflictContentRead::Unstable) }) as PortFuture<_, ErrorMarker>
		});
		let unstable_output = project_list(scan.clone(), true, unstable, CancellationToken::new()).await?;
		let different_output = project_list(scan, true, different, CancellationToken::new()).await?;

		assert!(matches!(
			&unstable_output.rows[0],
			ConflictRow::OrdinaryConflict { content_comparisons, .. }
				if content_comparisons[0].state() == ContentState::Unstable
		));

		let ConflictRow::OrdinaryConflict {
			content_comparisons: unavailable_comparisons,
			..
		} = &unavailable_output.rows[0]
		else {
			return Ok(());
		};
		let ConflictRow::OrdinaryConflict {
			content_comparisons: different_comparisons,
			..
		} = &different_output.rows[0]
		else {
			return Ok(());
		};
		assert_eq!(unavailable_comparisons[0].state(), ContentState::Unavailable);
		assert_eq!(different_comparisons[0].state(), ContentState::DifferentSha256);
		Ok(())
	}

	#[tokio::test]
	async fn disabled_hypothetical_detects_structural_collisions_without_invalidating_actual_view()
	-> Result<(), ErrorMarker> {
		let scan = EnvironmentConflictScan {
			providers: vec![
				provider(
					"Enabled",
					0,
					true,
					vec![data_mod_file("Enabled", 0, "folder", ParticipationReason::EnabledMod)],
					Vec::new(),
				),
				provider(
					"Disabled",
					1,
					false,
					vec![data_mod_file(
						"Disabled",
						1,
						"folder/file.txt",
						ParticipationReason::DisabledMod,
					)],
					Vec::new(),
				),
			],
			problems: Vec::new(),
		};
		let calls = Arc::new(AtomicUsize::new(0));
		let actual = project_list(
			scan.clone(),
			false,
			content_port(calls.clone()),
			CancellationToken::new(),
		)
		.await?;
		let inspected = project_inspection(
			scan,
			mod_name("disabled"),
			false,
			content_port(calls),
			CancellationToken::new(),
		)
		.await?;

		assert_eq!(actual.resolution_status, ResolutionStatus::Exact);
		assert_eq!(inspected.resolution_status, ResolutionStatus::Invalid);
		assert_eq!(inspected.mod_name.as_str(), "Disabled");
		Ok(())
	}

	#[tokio::test]
	async fn explanation_includes_complete_directory_provenance_and_canonical_display_spelling()
	-> Result<(), ErrorMarker> {
		let mut base = fixed_provider(ProviderIdentity::SteamData, true, Vec::new());
		base.directories.push(ProviderReference::SteamData {
			original_path: path("TEXTURES"),
		});
		let mut lower = provider("Lower", 0, true, Vec::new(), Vec::new());
		lower.directories.push(ProviderReference::DataMod {
			mod_name: mod_name("Lower"),
			priority: ModPriority::new(0),
			original_path: path("textures"),
			participation_reason: ParticipationReason::EnabledMod,
		});
		let mut higher = provider("Higher", 1, true, Vec::new(), Vec::new());
		higher.directories.push(ProviderReference::DataMod {
			mod_name: mod_name("Higher"),
			priority: ModPriority::new(1),
			original_path: path("Textures"),
			participation_reason: ParticipationReason::EnabledMod,
		});
		let scan = EnvironmentConflictScan {
			providers: vec![base, lower, higher],
			problems: Vec::new(),
		};

		let output = project_path(
			scan,
			path("TeXtUrEs"),
			false,
			content_port(Arc::new(AtomicUsize::new(0))),
			CancellationToken::new(),
		)
		.await?;

		assert_eq!(output.display_path.as_str(), "Textures");
		assert_eq!(output.provider_stack.len(), 3);
		assert_eq!(output.provider_stack[0].original_path().as_str(), "Textures");
		assert_eq!(output.provider_stack[1].original_path().as_str(), "textures");
		assert_eq!(output.provider_stack[2].original_path().as_str(), "TEXTURES");
		Ok(())
	}

	#[tokio::test]
	async fn orphan_tombstone_controls_an_exact_absent_explanation() -> Result<(), ErrorMarker> {
		let tombstone = Tombstone {
			scope: TombstoneScope::ExactFile,
			owner: ProviderReference::DataMod {
				mod_name: mod_name("Owner"),
				priority: ModPriority::new(0),
				original_path: path("gone.txt"),
				participation_reason: ParticipationReason::EnabledMod,
			},
		};
		let scan = EnvironmentConflictScan {
			providers: vec![provider("Owner", 0, true, Vec::new(), vec![tombstone.clone()])],
			problems: Vec::new(),
		};

		let output = project_path(
			scan,
			path("GONE.txt"),
			false,
			content_port(Arc::new(AtomicUsize::new(0))),
			CancellationToken::new(),
		)
		.await?;

		assert_eq!(output.display_path.as_str(), "gone.txt");
		assert!(matches!(
			output.effective_result,
			Some(EffectiveResult::Absent {
				controlling_tombstone: Some(ref controlling)
			}) if controlling == &tombstone
		));
		Ok(())
	}

	#[tokio::test]
	async fn disabled_provider_problems_affect_only_hypothetical_inspection() -> Result<(), ErrorMarker> {
		let mut disabled = provider("Disabled", 1, false, Vec::new(), Vec::new());
		disabled.problems.push(ConflictProblem {
			kind: ConflictProblemKind::InvalidTombstoneMetadata,
			scope: ProblemScope::Provider(disabled.identity.clone()),
		});
		let scan = EnvironmentConflictScan {
			providers: vec![disabled],
			problems: Vec::new(),
		};

		let actual = project_list(
			scan.clone(),
			false,
			content_port(Arc::new(AtomicUsize::new(0))),
			CancellationToken::new(),
		)
		.await?;
		let hypothetical = inspect(scan, "Disabled", false, Arc::new(AtomicUsize::new(0))).await?;

		assert_eq!(actual.resolution_status, ResolutionStatus::Exact);
		assert!(actual.problems.is_empty());
		assert_eq!(hypothetical.resolution_status, ResolutionStatus::Invalid);
		assert_eq!(hypothetical.provider_summary.state, ProviderState::Invalid);
		Ok(())
	}

	#[tokio::test]
	async fn inspection_hashes_only_conflicts_that_involve_the_selected_mod() -> Result<(), ErrorMarker> {
		let scan = EnvironmentConflictScan {
			providers: vec![
				provider(
					"Lower",
					0,
					true,
					vec![data_mod_file(
						"Lower",
						0,
						"selected.txt",
						ParticipationReason::EnabledMod,
					)],
					Vec::new(),
				),
				provider(
					"Selected",
					1,
					true,
					vec![data_mod_file(
						"Selected",
						1,
						"selected.txt",
						ParticipationReason::EnabledMod,
					)],
					Vec::new(),
				),
				provider(
					"Unrelated Lower",
					2,
					true,
					vec![data_mod_file(
						"Unrelated Lower",
						2,
						"unrelated.txt",
						ParticipationReason::EnabledMod,
					)],
					Vec::new(),
				),
				provider(
					"Unrelated Higher",
					3,
					true,
					vec![data_mod_file(
						"Unrelated Higher",
						3,
						"unrelated.txt",
						ParticipationReason::EnabledMod,
					)],
					Vec::new(),
				),
			],
			problems: Vec::new(),
		};
		let calls = Arc::new(AtomicUsize::new(0));

		let output = inspect(scan, "Selected", true, calls.clone()).await?;

		assert_eq!(output.rows.len(), 1);
		assert_eq!(calls.load(Ordering::SeqCst), 2);
		Ok(())
	}

	#[tokio::test]
	async fn summary_states_and_counters_follow_unit_defined_scenarios() -> Result<(), ErrorMarker> {
		let no_calls = || Arc::new(AtomicUsize::new(0));

		let suppression_only = inspect(
			EnvironmentConflictScan {
				providers: vec![
					provider(
						"Lower",
						0,
						true,
						vec![
							data_mod_file(
								"Lower",
								0,
								"old.txt",
								ParticipationReason::EnabledMod,
							),
							data_mod_file(
								"Lower",
								0,
								"folder/file.txt",
								ParticipationReason::EnabledMod,
							),
						],
						Vec::new(),
					),
					provider(
						"Suppression",
						1,
						true,
						Vec::new(),
						vec![
							owned_tombstone(
								"Suppression",
								1,
								"old.txt",
								TombstoneScope::ExactFile,
							),
							owned_tombstone(
								"Suppression",
								1,
								"folder",
								TombstoneScope::DirectorySubtree,
							),
						],
					),
				],
				problems: Vec::new(),
			},
			"Suppression",
			false,
			no_calls(),
		)
		.await?
		.provider_summary;
		assert_eq!(suppression_only.state, ProviderState::SuppressionOnly);
		assert_eq!(
			(
				suppression_only.ordinary_file_count,
				suppression_only.owned_file_tombstone_count,
				suppression_only.owned_directory_tombstone_count,
				suppression_only.lower_file_entries_suppressed_count
			),
			(0, 1, 1, 2)
		);

		let empty = inspect(
			EnvironmentConflictScan {
				providers: vec![provider("Empty", 0, true, Vec::new(), Vec::new())],
				problems: Vec::new(),
			},
			"Empty",
			false,
			no_calls(),
		)
		.await?
		.provider_summary;
		assert_eq!(empty.state, ProviderState::Empty);

		let fully_overridden = inspect(
			EnvironmentConflictScan {
				providers: vec![
					provider(
						"Selected",
						0,
						true,
						vec![data_mod_file(
							"Selected",
							0,
							"same.txt",
							ParticipationReason::EnabledMod,
						)],
						Vec::new(),
					),
					provider(
						"Higher",
						1,
						true,
						vec![data_mod_file(
							"Higher",
							1,
							"same.txt",
							ParticipationReason::EnabledMod,
						)],
						Vec::new(),
					),
				],
				problems: Vec::new(),
			},
			"Selected",
			false,
			no_calls(),
		)
		.await?
		.provider_summary;
		assert_eq!(fully_overridden.state, ProviderState::FullyOverridden);
		assert_eq!(
			(
				fully_overridden.ordinary_file_count,
				fully_overridden.effective_file_count,
				fully_overridden.file_conflict_loss_count
			),
			(1, 0, 1)
		);

		let mixed = inspect(
			EnvironmentConflictScan {
				providers: vec![
					provider(
						"Lower",
						0,
						true,
						vec![data_mod_file(
							"Lower",
							0,
							"won.txt",
							ParticipationReason::EnabledMod,
						)],
						Vec::new(),
					),
					provider(
						"Selected",
						1,
						true,
						vec![
							data_mod_file(
								"Selected",
								1,
								"won.txt",
								ParticipationReason::EnabledMod,
							),
							data_mod_file(
								"Selected",
								1,
								"lost.txt",
								ParticipationReason::EnabledMod,
							),
						],
						Vec::new(),
					),
					provider(
						"Higher",
						2,
						true,
						vec![data_mod_file(
							"Higher",
							2,
							"lost.txt",
							ParticipationReason::EnabledMod,
						)],
						Vec::new(),
					),
				],
				problems: Vec::new(),
			},
			"Selected",
			false,
			no_calls(),
		)
		.await?
		.provider_summary;
		assert_eq!(mixed.state, ProviderState::Mixed);
		assert_eq!(
			(
				mixed.ordinary_file_count,
				mixed.effective_file_count,
				mixed.file_conflict_win_count,
				mixed.file_conflict_loss_count
			),
			(2, 1, 1, 1)
		);

		let uncontested_with_suppression = inspect(
			EnvironmentConflictScan {
				providers: vec![
					provider(
						"Lower",
						0,
						true,
						vec![data_mod_file(
							"Lower",
							0,
							"old.txt",
							ParticipationReason::EnabledMod,
						)],
						Vec::new(),
					),
					provider(
						"Selected",
						1,
						true,
						vec![
							data_mod_file(
								"Selected",
								1,
								"own.txt",
								ParticipationReason::EnabledMod,
							),
							data_mod_file(
								"Selected",
								1,
								"removed.txt",
								ParticipationReason::EnabledMod,
							),
						],
						vec![owned_tombstone(
							"Selected",
							1,
							"old.txt",
							TombstoneScope::ExactFile,
						)],
					),
					provider(
						"Higher",
						2,
						true,
						Vec::new(),
						vec![owned_tombstone(
							"Higher",
							2,
							"removed.txt",
							TombstoneScope::ExactFile,
						)],
					),
				],
				problems: Vec::new(),
			},
			"Selected",
			false,
			no_calls(),
		)
		.await?
		.provider_summary;
		assert_eq!(uncontested_with_suppression.state, ProviderState::Uncontested);
		assert_eq!(
			(
				uncontested_with_suppression.ordinary_file_count,
				uncontested_with_suppression.effective_file_count,
				uncontested_with_suppression.lower_file_entries_suppressed_count,
				uncontested_with_suppression.own_file_entries_suppressed_count
			),
			(2, 1, 1, 1)
		);

		let winning = inspect(
			EnvironmentConflictScan {
				providers: vec![
					provider(
						"Lower",
						0,
						true,
						vec![data_mod_file(
							"Lower",
							0,
							"shared.txt",
							ParticipationReason::EnabledMod,
						)],
						Vec::new(),
					),
					provider(
						"Selected",
						1,
						true,
						vec![data_mod_file(
							"Selected",
							1,
							"shared.txt",
							ParticipationReason::EnabledMod,
						)],
						Vec::new(),
					),
				],
				problems: Vec::new(),
			},
			"Selected",
			false,
			no_calls(),
		)
		.await?
		.provider_summary;
		assert_eq!(winning.state, ProviderState::WinningConflicts);
		assert_eq!((winning.effective_file_count, winning.file_conflict_win_count), (1, 1));

		let losing = inspect(
			EnvironmentConflictScan {
				providers: vec![
					provider(
						"Selected",
						0,
						true,
						vec![
							data_mod_file(
								"Selected",
								0,
								"shared.txt",
								ParticipationReason::EnabledMod,
							),
							data_mod_file(
								"Selected",
								0,
								"unique.txt",
								ParticipationReason::EnabledMod,
							),
						],
						Vec::new(),
					),
					provider(
						"Higher",
						1,
						true,
						vec![data_mod_file(
							"Higher",
							1,
							"shared.txt",
							ParticipationReason::EnabledMod,
						)],
						Vec::new(),
					),
				],
				problems: Vec::new(),
			},
			"Selected",
			false,
			no_calls(),
		)
		.await?
		.provider_summary;
		assert_eq!(losing.state, ProviderState::LosingConflicts);
		assert_eq!(
			(
				losing.ordinary_file_count,
				losing.effective_file_count,
				losing.file_conflict_loss_count
			),
			(2, 1, 1)
		);

		let mut invalid_provider = provider("Invalid", 0, true, Vec::new(), Vec::new());
		invalid_provider.problems.push(ConflictProblem {
			kind: ConflictProblemKind::ProviderMissing,
			scope: ProblemScope::Provider(invalid_provider.identity.clone()),
		});
		let invalid = inspect(
			EnvironmentConflictScan {
				providers: vec![invalid_provider],
				problems: Vec::new(),
			},
			"Invalid",
			false,
			no_calls(),
		)
		.await?
		.provider_summary;
		assert_eq!(invalid.state, ProviderState::Invalid);
		Ok(())
	}

	#[tokio::test]
	async fn row_and_suppressed_entry_order_uses_windows_utf16_code_units() -> Result<(), ErrorMarker> {
		let supplementary = "folder/𐀀.txt";
		let private_use = "folder/\u{e000}.txt";
		let ordinary_scan = EnvironmentConflictScan {
			providers: vec![
				provider(
					"Lower",
					0,
					true,
					vec![
						data_mod_file("Lower", 0, private_use, ParticipationReason::EnabledMod),
						data_mod_file(
							"Lower",
							0,
							supplementary,
							ParticipationReason::EnabledMod,
						),
					],
					Vec::new(),
				),
				provider(
					"Higher",
					1,
					true,
					vec![
						data_mod_file(
							"Higher",
							1,
							private_use,
							ParticipationReason::EnabledMod,
						),
						data_mod_file(
							"Higher",
							1,
							supplementary,
							ParticipationReason::EnabledMod,
						),
					],
					Vec::new(),
				),
			],
			problems: Vec::new(),
		};
		let ordinary = project_list(
			ordinary_scan,
			false,
			content_port(Arc::new(AtomicUsize::new(0))),
			CancellationToken::new(),
		)
		.await?;
		let ConflictRow::OrdinaryConflict {
			display_path: first, ..
		} = &ordinary.rows[0]
		else {
			return Err(rootcause::report!(ErrorMarker::io_failure()));
		};
		let ConflictRow::OrdinaryConflict {
			display_path: second, ..
		} = &ordinary.rows[1]
		else {
			return Err(rootcause::report!(ErrorMarker::io_failure()));
		};
		assert_eq!(first.as_str(), supplementary);
		assert_eq!(second.as_str(), private_use);

		let tombstone = owned_tombstone("Higher", 1, "folder", TombstoneScope::DirectorySubtree);
		let scan = EnvironmentConflictScan {
			providers: vec![
				provider(
					"Lower",
					0,
					true,
					vec![
						data_mod_file("Lower", 0, private_use, ParticipationReason::EnabledMod),
						data_mod_file(
							"Lower",
							0,
							supplementary,
							ParticipationReason::EnabledMod,
						),
					],
					Vec::new(),
				),
				provider("Higher", 1, true, Vec::new(), vec![tombstone]),
			],
			problems: Vec::new(),
		};

		let output = project_list(
			scan,
			false,
			content_port(Arc::new(AtomicUsize::new(0))),
			CancellationToken::new(),
		)
		.await?;

		let ConflictRow::Tombstone { suppressed_entries, .. } = &output.rows[0] else {
			return Err(rootcause::report!(ErrorMarker::io_failure()));
		};
		assert_eq!(suppressed_entries[0].original_path().as_str(), supplementary);
		assert_eq!(suppressed_entries[1].original_path().as_str(), private_use);
		Ok(())
	}
}
