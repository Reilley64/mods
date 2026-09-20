use crate::error::ArchiveError;
use crate::extract::extract_selected;
use crate::extract::read_member_bounded;
use crate::fomod::FomodDocument;
use crate::fomod::NormalizedElement;
use crate::fomod::parse;
use crate::index::ArchiveIndexCore;
use crate::index::ArchiveMember;
use crate::index::MemberKind;
use crate::index::index_archive;
use crate::limits::COPY_BUFFER_BYTES;
use crate::limits::MAX_ARCHIVE_WORK;
use crate::limits::MAX_FOMOD_DERIVED_CANDIDATES;
use crate::limits::MAX_FOMOD_DERIVED_WORK;
use crate::limits::MAX_SOURCE_DESTINATION_FAN_OUT;
use crate::limits::MAX_STAGED_OUTPUT_BYTES;
use crate::limits::MAX_XML_BYTES;
use crate::path::SafeArchivePath;
use crate::path::normalization_alias_key;
use application::ErrorMarker;
use application::installation::ArchiveIndex;
use application::installation::ConditionOperator;
use application::installation::ConditionScope;
use application::installation::ConditionalCandidates;
use application::installation::FomodFlagWrite;
use application::installation::FomodGroup;
use application::installation::FomodInstaller;
use application::installation::FomodOption;
use application::installation::FomodOptionTypePattern;
use application::installation::IndexedInstaller;
use application::installation::InstallWarning;
use application::installation::MalformedGroupRepair;
use application::ports::BeginInstallationFile;
use application::ports::ExtractApprovedFiles;
use application::ports::IndexArchive;
use application::ports::InstallationFile;
use application::ports::PortFuture;
use domain::ArchiveIdentity;
use domain::ArchivePath;
use domain::DataRelativePath;
use domain::FileDependencyState;
use domain::FomodCardinality;
use domain::FomodCondition;
use domain::InstallCandidate;
use domain::InstallCandidateOrigin;
use domain::InstallationPhase;
use domain::OptionFileTrigger;
use domain::ResolvedOptionType;
use domain::Sha256Digest;
use domain::case_fold_key;
use rootcause::Report;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use rootcause::report;
use sha2::Digest;
use sha2::Sha256;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::HashMap;
use std::io::Error as IoError;
use std::io::Result as IoResult;
use std::io::Write;
use std::mem::take;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc::Sender;
use tokio::sync::mpsc::channel;
use tokio::task::spawn_blocking;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Copy, Debug, Default)]
pub struct ArchiveAdapter;

impl ArchiveAdapter {
	pub fn index_port(&self) -> IndexArchive {
		Arc::new(move |archive, cancellation| {
			Box::pin(async move {
				spawn_blocking(move || index_for_application(archive, &cancellation))
					.await
					.context(ArchiveError::Io)
					.map_err(map_archive_report)?
			}) as PortFuture<_>
		})
	}

	pub fn extract_port(&self) -> ExtractApprovedFiles {
		Arc::new(move |archive, identity, candidates, begin_file, cancellation| {
			Box::pin(extract_for_application(
				archive,
				identity,
				candidates,
				begin_file,
				cancellation,
			)) as PortFuture<_>
		})
	}
}

fn index_for_application(archive: ArchivePath, cancellation: &CancellationToken) -> Result<ArchiveIndex, ErrorMarker> {
	let started = Instant::now();
	let core = index_archive(archive.as_path(), cancellation).map_err(map_archive_report)?;
	if cancellation.is_cancelled() {
		return Err(map_archive_report(report!(ArchiveError::Cancelled)));
	}
	if started.elapsed() > MAX_ARCHIVE_WORK {
		return Err(map_archive_report(report!(ArchiveError::WorkLimit)));
	}
	let archive_sha256 = digest(core.sha256)?;
	let package_root = core.discovery.package_root.join("/");
	let Some(configuration_ordinal) = core.discovery.configuration_ordinal else {
		let candidates = plain_candidates(&core, cancellation, started)?;
		return Ok(ArchiveIndex {
			identity: ArchiveIdentity::DataArchive {
				archive_sha256,
				package_root,
			},
			installer: IndexedInstaller::Plain {
				candidates,
				warnings: Vec::new(),
			},
		});
	};

	if cancellation.is_cancelled() {
		return Err(map_archive_report(report!(ArchiveError::Cancelled)));
	}
	if started.elapsed() > MAX_ARCHIVE_WORK {
		return Err(map_archive_report(report!(ArchiveError::WorkLimit)));
	}
	let configuration = read_member_bounded(&core, configuration_ordinal, MAX_XML_BYTES, cancellation)
		.map_err(map_archive_report)?;
	if cancellation.is_cancelled() {
		return Err(map_archive_report(report!(ArchiveError::Cancelled)));
	}
	if started.elapsed() > MAX_ARCHIVE_WORK {
		return Err(map_archive_report(report!(ArchiveError::WorkLimit)));
	}
	let document = parse(&configuration, cancellation).map_err(map_archive_report)?;
	if cancellation.is_cancelled() {
		return Err(map_archive_report(report!(ArchiveError::Cancelled)));
	}
	if started.elapsed() > MAX_ARCHIVE_WORK {
		return Err(map_archive_report(report!(ArchiveError::WorkLimit)));
	}
	let config_member = core.members[configuration_ordinal].path.as_str().to_owned();
	let config_sha256 = digest(Sha256::digest(&configuration).into())?;
	if cancellation.is_cancelled() {
		return Err(map_archive_report(report!(ArchiveError::Cancelled)));
	}
	if started.elapsed() > MAX_ARCHIVE_WORK {
		return Err(map_archive_report(report!(ArchiveError::WorkLimit)));
	}
	let mut converter = FomodConverter::new(&core, cancellation, started)?;
	let mut installer = converter.convert(&document)?;
	if cancellation.is_cancelled() {
		return Err(map_archive_report(report!(ArchiveError::Cancelled)));
	}
	if started.elapsed() > MAX_ARCHIVE_WORK {
		return Err(map_archive_report(report!(ArchiveError::WorkLimit)));
	}
	if core.discovery.ignored_script_alias {
		let mut ignored = None;
		for member in &core.members {
			if cancellation.is_cancelled() {
				return Err(map_archive_report(report!(ArchiveError::Cancelled)));
			}
			if started.elapsed() > MAX_ARCHIVE_WORK {
				return Err(map_archive_report(report!(ArchiveError::WorkLimit)));
			}

			let components = member.path.components();
			if components.len() >= 2
				&& case_fold_key(&components[components.len() - 2]) == "fomod"
				&& case_fold_key(&components[components.len() - 1]) == "script.xml"
			{
				ignored = Some(member.path.as_str().to_owned());
				break;
			}
		}
		let ignored = ignored.ok_or_else(|| report!(ErrorMarker::unsafe_archive()))?;
		installer.warnings.push(InstallWarning::FomodModuleConfigPreferred {
			selected_config_member: config_member.clone(),
			ignored_config_member: ignored,
		});
	}
	Ok(ArchiveIndex {
		identity: ArchiveIdentity::Fomod {
			archive_sha256,
			package_root,
			config_member,
			config_sha256,
		},
		installer: IndexedInstaller::Fomod(installer),
	})
}

fn plain_candidates(
	core: &ArchiveIndexCore,
	cancellation: &CancellationToken,
	archive_started: Instant,
) -> Result<Vec<InstallCandidate>, ErrorMarker> {
	if cancellation.is_cancelled() {
		return Err(map_archive_report(report!(ArchiveError::Cancelled)));
	}
	if archive_started.elapsed() > MAX_ARCHIVE_WORK {
		return Err(map_archive_report(report!(ArchiveError::WorkLimit)));
	}

	let mut candidates = Vec::new();
	for member in core.members.iter().filter(|member| member.kind == MemberKind::File) {
		if cancellation.is_cancelled() {
			return Err(map_archive_report(report!(ArchiveError::Cancelled)));
		}
		if archive_started.elapsed() > MAX_ARCHIVE_WORK {
			return Err(map_archive_report(report!(ArchiveError::WorkLimit)));
		}

		let components = member.path.components();
		if components_start_with(components, &core.discovery.package_root)
			&& components
				.get(core.discovery.package_root.len())
				.is_some_and(|component| case_fold_key(component) == "fomod")
		{
			continue;
		}
		let Some(destination) = member.path.strip_prefix(&core.discovery.data_root) else {
			return Err(report!(ErrorMarker::unsafe_archive()));
		};
		if destination.is_empty() {
			continue;
		}
		let destination = data_destination(destination)?;
		let order = u64::try_from(member.ordinal)
			.context(ArchiveError::InvalidArchive)
			.map_err(map_archive_report)?;
		candidates.push(InstallCandidate {
			candidate_id: order,
			origin: InstallCandidateOrigin::Required,
			phase: InstallationPhase::Required,
			declared_priority: 0,
			descriptor_order: order,
			source_member: member.path.as_str().to_owned(),
			destination,
		});
	}
	if cancellation.is_cancelled() {
		return Err(map_archive_report(report!(ArchiveError::Cancelled)));
	}
	if archive_started.elapsed() > MAX_ARCHIVE_WORK {
		return Err(map_archive_report(report!(ArchiveError::WorkLimit)));
	}
	if candidates.is_empty() {
		return Err(report!(ErrorMarker::unsupported_installer()));
	}
	Ok(candidates)
}

struct StagedOutputBudget {
	bytes: u64,
	limit: u64,
}

impl StagedOutputBudget {
	fn new() -> Self {
		Self {
			bytes: 0,
			limit: MAX_STAGED_OUTPUT_BYTES,
		}
	}

	fn charge(&mut self, member_bytes: u64, destination_count: usize) -> Result<(), ArchiveError> {
		let destination_count = u64::try_from(destination_count).context(ArchiveError::ExpansionLimit)?;
		let staged_bytes = member_bytes
			.checked_mul(destination_count)
			.ok_or_else(|| report!(ArchiveError::ExpansionLimit))?;
		self.bytes = self
			.bytes
			.checked_add(staged_bytes)
			.ok_or_else(|| report!(ArchiveError::ExpansionLimit))?;
		if self.bytes > self.limit {
			return Err(report!(ArchiveError::ExpansionLimit));
		}
		Ok(())
	}
}

struct FomodBudget {
	derived_work: usize,
	derived_candidates: usize,
	max_derived_work: usize,
	max_derived_candidates: usize,
}

impl FomodBudget {
	fn new() -> Self {
		Self {
			derived_work: 0,
			derived_candidates: 0,
			max_derived_work: MAX_FOMOD_DERIVED_WORK,
			max_derived_candidates: MAX_FOMOD_DERIVED_CANDIDATES,
		}
	}

	fn charge_work(&mut self) -> Result<(), ErrorMarker> {
		self.derived_work = self
			.derived_work
			.checked_add(1)
			.ok_or_else(|| map_archive_report(report!(ArchiveError::ExpansionLimit)))?;
		if self.derived_work > self.max_derived_work {
			return Err(map_archive_report(report!(ArchiveError::ExpansionLimit)));
		}
		Ok(())
	}

	fn charge_candidate(&mut self) -> Result<(), ErrorMarker> {
		self.derived_candidates = self
			.derived_candidates
			.checked_add(1)
			.ok_or_else(|| map_archive_report(report!(ArchiveError::ExpansionLimit)))?;
		if self.derived_candidates > self.max_derived_candidates {
			return Err(map_archive_report(report!(ArchiveError::ExpansionLimit)));
		}
		Ok(())
	}
}

struct FomodConverter<'a> {
	core: &'a ArchiveIndexCore,
	cancellation: &'a CancellationToken,
	archive_started: Instant,
	source_lookup: BTreeMap<String, usize>,
	destination_aliases: BTreeMap<String, String>,
	budget: FomodBudget,
	next_candidate: u64,
	next_descriptor: u64,
	next_pattern: u64,
	warnings: Vec<InstallWarning>,
}

impl<'a> FomodConverter<'a> {
	fn new(
		core: &'a ArchiveIndexCore,
		cancellation: &'a CancellationToken,
		archive_started: Instant,
	) -> Result<Self, ErrorMarker> {
		let mut source_lookup = BTreeMap::new();
		for member in core.members.iter().filter(|member| member.kind == MemberKind::File) {
			if cancellation.is_cancelled() {
				return Err(map_archive_report(report!(ArchiveError::Cancelled)));
			}
			if archive_started.elapsed() > MAX_ARCHIVE_WORK {
				return Err(map_archive_report(report!(ArchiveError::WorkLimit)));
			}
			source_lookup.insert(normalized_components_key(member.path.components()), member.ordinal);
		}
		Ok(Self {
			core,
			cancellation,
			archive_started,
			source_lookup,
			destination_aliases: BTreeMap::new(),
			budget: FomodBudget::new(),
			next_candidate: 0,
			next_descriptor: 0,
			next_pattern: 0,
			warnings: Vec::new(),
		})
	}

	fn convert(&mut self, document: &FomodDocument) -> Result<FomodInstaller, ErrorMarker> {
		let module_condition = match optional_child(&document.root, "moduleDependencies")? {
			Some(node) => self.condition(node, ConditionScope::Module, None, None, None)?,
			None => FomodCondition::Constant(true),
		};
		let required_candidates = match optional_child(&document.root, "requiredInstallFiles")? {
			Some(files) => self.file_list(
				files,
				InstallationPhase::Required,
				InstallCandidateOrigin::Required,
				None,
				None,
			)?,
			None => Vec::new(),
		};
		let groups = match optional_child(&document.root, "installSteps")? {
			Some(steps) => self.groups(steps)?,
			None => Vec::new(),
		};
		let conditional_candidates = match optional_child(&document.root, "conditionalFileInstalls")? {
			Some(patterns) => self.conditional_candidates(patterns)?,
			None => Vec::new(),
		};
		Ok(FomodInstaller {
			schema_version: document.schema_version.clone(),
			module_condition,
			groups,
			required_candidates,
			conditional_candidates,
			warnings: take(&mut self.warnings),
		})
	}

	fn groups(&mut self, steps: &NormalizedElement) -> Result<Vec<FomodGroup>, ErrorMarker> {
		let mut result = Vec::new();
		let mut used_step_ids = BTreeSet::new();
		for (step_index, step) in ordered_children(steps, "installStep")?.into_iter().enumerate() {
			if self.cancellation.is_cancelled() {
				return Err(map_archive_report(report!(ArchiveError::Cancelled)));
			}
			if self.archive_started.elapsed() > MAX_ARCHIVE_WORK {
				return Err(map_archive_report(report!(ArchiveError::WorkLimit)));
			}
			let step_label = required_attribute(step, "name")?;
			let step_id = unique_identifier(
				identifier(step_label, &format!("step-{}", step_index + 1)),
				&mut used_step_ids,
			);
			let step_condition = match optional_child(step, "visible")? {
				Some(visible) => {
					self.condition(visible, ConditionScope::StepVisibility, None, None, None)?
				}
				None => FomodCondition::Constant(true),
			};
			let file_groups = required_child(step, "optionalFileGroups")?;
			let mut used_group_ids = BTreeSet::new();
			for (group_index, group) in ordered_children(file_groups, "group")?.into_iter().enumerate() {
				if self.cancellation.is_cancelled() {
					return Err(map_archive_report(report!(ArchiveError::Cancelled)));
				}
				if self.archive_started.elapsed() > MAX_ARCHIVE_WORK {
					return Err(map_archive_report(report!(ArchiveError::WorkLimit)));
				}
				let label = required_attribute(group, "name")?.to_owned();
				let group_component = unique_identifier(
					identifier(&label, &format!("group-{}", group_index + 1)),
					&mut used_group_ids,
				);
				let group_id = format!("{step_id}.{group_component}");
				let mut cardinality = parse_cardinality(required_attribute(group, "type")?)?;
				let plugins = required_child(group, "plugins")?;
				let mut options = Vec::new();
				let mut used_option_ids = BTreeSet::from(["none".to_owned()]);
				for (option_index, plugin) in
					ordered_children(plugins, "plugin")?.into_iter().enumerate()
				{
					if self.cancellation.is_cancelled() {
						return Err(map_archive_report(report!(ArchiveError::Cancelled)));
					}
					if self.archive_started.elapsed() > MAX_ARCHIVE_WORK {
						return Err(map_archive_report(report!(ArchiveError::WorkLimit)));
					}
					let option_label = required_attribute(plugin, "name")?.to_owned();
					let option_id = unique_identifier(
						identifier(&option_label, &format!("option-{}", option_index + 1)),
						&mut used_option_ids,
					);
					options.push(self.option(plugin, &group_id, &option_id, option_label)?);
				}
				// SelectAll is equivalent only when the sole option cannot become unselectable.
				if options.len() == 1
					&& cardinality == FomodCardinality::SelectExactlyOne
					&& options[0].default_type != ResolvedOptionType::NotUsable
					&& options[0]
						.type_patterns
						.iter()
						.all(|pattern| pattern.option_type != ResolvedOptionType::NotUsable)
				{
					cardinality = FomodCardinality::SelectAll;
					self.warnings.push(InstallWarning::FomodMalformedGroupRepaired {
						group_id: group_id.clone(),
						repair: MalformedGroupRepair::SingleOptionExactlyOneToSelectAll,
					});
				}
				result.push(FomodGroup {
					id: group_id,
					label,
					description: String::new(),
					cardinality,
					condition: step_condition.clone(),
					options,
				});
			}
		}
		Ok(result)
	}

	fn option(
		&mut self,
		plugin: &NormalizedElement,
		group_id: &str,
		option_id: &str,
		label: String,
	) -> Result<FomodOption, ErrorMarker> {
		let description = optional_child(plugin, "description")?
			.map(|node| node.text.clone())
			.unwrap_or_default();
		let (default_type, type_patterns) = self.option_type(plugin, group_id, option_id)?;
		let flag_writes = match optional_child(plugin, "conditionFlags")? {
			Some(flags) => children(flags, "flag")
				.into_iter()
				.map(|flag| {
					Ok(FomodFlagWrite {
						name: required_attribute(flag, "name")?.to_owned(),
						value: flag.text.clone(),
					})
				})
				.collect::<Result<Vec<_>, ErrorMarker>>()?,
			None => Vec::new(),
		};
		let file_candidates = match optional_child(plugin, "files")? {
			Some(files) => self.option_files(files, group_id, option_id)?,
			None => Vec::new(),
		};
		if flag_writes.is_empty() && file_candidates.is_empty() {
			self.warnings.push(InstallWarning::FomodEmptyOptionAccepted {
				group_id: group_id.to_owned(),
				option_id: option_id.to_owned(),
			});
		}
		Ok(FomodOption {
			id: option_id.to_owned(),
			label,
			description,
			condition: FomodCondition::Constant(true),
			default_type,
			type_patterns,
			flag_writes,
			file_candidates,
		})
	}

	fn option_type(
		&mut self,
		plugin: &NormalizedElement,
		group_id: &str,
		option_id: &str,
	) -> Result<(ResolvedOptionType, Vec<FomodOptionTypePattern>), ErrorMarker> {
		let descriptor = required_child(plugin, "typeDescriptor")?;
		if let Some(node) = optional_child(descriptor, "type")? {
			return Ok((parse_option_type(required_attribute(node, "name")?)?, Vec::new()));
		}
		let dependency_type = required_child(descriptor, "dependencyType")?;
		let default_type = parse_option_type(required_attribute(
			required_child(dependency_type, "defaultType")?,
			"name",
		)?)?;
		let patterns = required_child(dependency_type, "patterns")?;
		let mut result = Vec::new();
		for pattern in children(patterns, "pattern") {
			if self.cancellation.is_cancelled() {
				return Err(map_archive_report(report!(ArchiveError::Cancelled)));
			}
			if self.archive_started.elapsed() > MAX_ARCHIVE_WORK {
				return Err(map_archive_report(report!(ArchiveError::WorkLimit)));
			}
			let condition = self.condition(
				required_child(pattern, "dependencies")?,
				ConditionScope::OptionTypePattern,
				Some(group_id),
				Some(option_id),
				None,
			)?;
			let option_type =
				parse_option_type(required_attribute(required_child(pattern, "type")?, "name")?)?;
			result.push(FomodOptionTypePattern { condition, option_type });
		}
		Ok((default_type, result))
	}

	fn option_files(
		&mut self,
		files: &NormalizedElement,
		group_id: &str,
		option_id: &str,
	) -> Result<Vec<InstallCandidate>, ErrorMarker> {
		let mut result = Vec::new();
		for operation in &files.children {
			if self.cancellation.is_cancelled() {
				return Err(map_archive_report(report!(ArchiveError::Cancelled)));
			}
			if self.archive_started.elapsed() > MAX_ARCHIVE_WORK {
				return Err(map_archive_report(report!(ArchiveError::WorkLimit)));
			}
			let always = boolean_attribute(operation, "alwaysInstall")?.unwrap_or(false);
			let usable = boolean_attribute(operation, "installIfUsable")?.unwrap_or(false);
			let trigger = if always {
				OptionFileTrigger::AlwaysInstall
			} else if usable {
				OptionFileTrigger::InstallIfUsable
			} else {
				OptionFileTrigger::Selected
			};
			result.extend(self.operation(
				operation,
				InstallationPhase::SelectedOrForced,
				InstallCandidateOrigin::Option {
					group_id: group_id.to_owned(),
					option_id: option_id.to_owned(),
					trigger,
				},
				Some(group_id),
				Some(option_id),
			)?);
		}
		Ok(result)
	}

	fn conditional_candidates(
		&mut self,
		container: &NormalizedElement,
	) -> Result<Vec<ConditionalCandidates>, ErrorMarker> {
		let patterns = if container.name.eq_ignore_ascii_case("patterns") {
			container
		} else {
			required_child(container, "patterns")?
		};
		let mut result = Vec::new();
		for pattern in children(patterns, "pattern") {
			if self.cancellation.is_cancelled() {
				return Err(map_archive_report(report!(ArchiveError::Cancelled)));
			}
			if self.archive_started.elapsed() > MAX_ARCHIVE_WORK {
				return Err(map_archive_report(report!(ArchiveError::WorkLimit)));
			}
			let pattern_order = self.next_pattern;
			self.next_pattern = self
				.next_pattern
				.checked_add(1)
				.ok_or_else(|| report!(ErrorMarker::unsafe_archive()))?;
			let condition = self.condition(
				required_child(pattern, "dependencies")?,
				ConditionScope::ConditionalFilePattern,
				None,
				None,
				Some(pattern_order),
			)?;
			let candidates = self.file_list(
				required_child(pattern, "files")?,
				InstallationPhase::Conditional,
				InstallCandidateOrigin::Conditional { pattern_order },
				None,
				None,
			)?;
			result.push(ConditionalCandidates { condition, candidates });
		}
		Ok(result)
	}

	fn file_list(
		&mut self,
		files: &NormalizedElement,
		phase: InstallationPhase,
		origin: InstallCandidateOrigin,
		group_id: Option<&str>,
		option_id: Option<&str>,
	) -> Result<Vec<InstallCandidate>, ErrorMarker> {
		let mut result = Vec::new();
		for operation in &files.children {
			if self.cancellation.is_cancelled() {
				return Err(map_archive_report(report!(ArchiveError::Cancelled)));
			}
			if self.archive_started.elapsed() > MAX_ARCHIVE_WORK {
				return Err(map_archive_report(report!(ArchiveError::WorkLimit)));
			}
			result.extend(self.operation(operation, phase, origin.clone(), group_id, option_id)?);
		}
		Ok(result)
	}

	fn operation(
		&mut self,
		operation: &NormalizedElement,
		phase: InstallationPhase,
		origin: InstallCandidateOrigin,
		group_id: Option<&str>,
		option_id: Option<&str>,
	) -> Result<Vec<InstallCandidate>, ErrorMarker> {
		if self.cancellation.is_cancelled() {
			return Err(map_archive_report(report!(ArchiveError::Cancelled)));
		}
		if self.archive_started.elapsed() > MAX_ARCHIVE_WORK {
			return Err(map_archive_report(report!(ArchiveError::WorkLimit)));
		}
		self.budget.charge_work()?;
		let source = required_attribute(operation, "source")?;
		let descriptor_order = self.next_descriptor;
		self.next_descriptor = self
			.next_descriptor
			.checked_add(1)
			.ok_or_else(|| report!(ErrorMarker::unsafe_archive()))?;
		if source.is_empty() {
			self.warnings.push(InstallWarning::FomodEmptySourceIgnored {
				descriptor_order,
				group_id: group_id.map(str::to_owned),
				option_id: option_id.map(str::to_owned),
			});
			return Ok(Vec::new());
		}
		let priority = optional_attribute(operation, "priority")
			.map(str::parse::<i32>)
			.transpose()
			.context(ArchiveError::UnsupportedInstaller)
			.map_err(map_archive_report)?
			.unwrap_or(0);
		let destination = optional_attribute(operation, "destination").unwrap_or(source);
		let destination_is_directory = destination.is_empty() || destination.ends_with(['/', '\\']);
		let source = source.trim_end_matches(['/', '\\']);
		let source_path = SafeArchivePath::new(source).map_err(map_archive_report)?;
		let source_members =
			self.resolve_source(&source_path, operation.name.eq_ignore_ascii_case("folder"))?;
		let mut destination = normalize_destination(destination)?;
		if operation.name.eq_ignore_ascii_case("file") && destination_is_directory {
			let basename = source_path
				.components()
				.last()
				.ok_or_else(|| report!(ErrorMarker::unsafe_archive()))?;
			destination = join_destination(&destination, basename);
		}
		let mut result = Vec::new();
		for ordinal in source_members {
			if self.cancellation.is_cancelled() {
				return Err(map_archive_report(report!(ArchiveError::Cancelled)));
			}
			if self.archive_started.elapsed() > MAX_ARCHIVE_WORK {
				return Err(map_archive_report(report!(ArchiveError::WorkLimit)));
			}
			self.budget.charge_candidate()?;
			let member = &self.core.members[ordinal];
			let final_destination = if operation.name.eq_ignore_ascii_case("folder") {
				let suffix = source_suffix(self.core, member, &source_path)?;
				join_destination(&destination, &suffix)
			} else {
				destination.clone()
			};
			let destination = data_destination(final_destination)?;
			let destination_identity = destination.comparison_key().to_owned();
			let alias_key = normalization_alias_key(destination.as_str());
			if self.destination_aliases
				.insert(alias_key, destination_identity.clone())
				.is_some_and(|existing| existing != destination_identity)
			{
				return Err(report!(ErrorMarker::unsafe_archive()));
			}
			let candidate_id = self.next_candidate;
			self.next_candidate = self
				.next_candidate
				.checked_add(1)
				.ok_or_else(|| report!(ErrorMarker::unsafe_archive()))?;
			result.push(InstallCandidate {
				candidate_id,
				origin: origin.clone(),
				phase,
				declared_priority: priority,
				descriptor_order,
				source_member: member.path.as_str().to_owned(),
				destination,
			});
		}
		Ok(result)
	}

	fn resolve_source(&mut self, source: &SafeArchivePath, folder: bool) -> Result<Vec<usize>, ErrorMarker> {
		let mut prefixes = vec![join_components(&self.core.discovery.package_root, source.components())];
		if self.core.discovery.data_root != self.core.discovery.package_root {
			prefixes.push(join_components(&self.core.discovery.data_root, source.components()));
		}

		let mut matching_prefixes = Vec::new();
		for prefix in prefixes {
			if self.cancellation.is_cancelled() {
				return Err(map_archive_report(report!(ArchiveError::Cancelled)));
			}
			if self.archive_started.elapsed() > MAX_ARCHIVE_WORK {
				return Err(map_archive_report(report!(ArchiveError::WorkLimit)));
			}
			self.budget.charge_work()?;
			let key = normalized_components_key(&prefix);
			let mut matches = Vec::new();
			if folder {
				let descendant_prefix = format!("{key}/");
				for (member_key, ordinal) in self.source_lookup.range(descendant_prefix.clone()..) {
					if !member_key.starts_with(&descendant_prefix) {
						break;
					}
					if self.cancellation.is_cancelled() {
						return Err(map_archive_report(report!(ArchiveError::Cancelled)));
					}
					if self.archive_started.elapsed() > MAX_ARCHIVE_WORK {
						return Err(map_archive_report(report!(ArchiveError::WorkLimit)));
					}
					self.budget.charge_work()?;
					matches.push(*ordinal);
				}
			} else if let Some(ordinal) = self.source_lookup.get(&key) {
				matches.push(*ordinal);
			}
			if !matches.is_empty() {
				matching_prefixes.push(matches);
			}
		}

		if matching_prefixes.is_empty() {
			return Err(report!(ErrorMarker::unsupported_installer()));
		}
		if matching_prefixes.len() != 1 || (!folder && matching_prefixes[0].len() != 1) {
			return Err(report!(ErrorMarker::ambiguous_install_plan()));
		}
		let mut matches = matching_prefixes.pop().unwrap_or_default();
		matches.sort_unstable();
		Ok(matches)
	}

	fn condition(
		&mut self,
		node: &NormalizedElement,
		scope: ConditionScope,
		group_id: Option<&str>,
		option_id: Option<&str>,
		pattern_order: Option<u64>,
	) -> Result<FomodCondition, ErrorMarker> {
		let operator = match optional_attribute(node, "operator").unwrap_or("And") {
			value if value.eq_ignore_ascii_case("And") => ConditionOperator::And,
			value if value.eq_ignore_ascii_case("Or") => ConditionOperator::Or,
			_ => return Err(report!(ErrorMarker::unsupported_installer())),
		};
		let mut conditions = Vec::new();
		for child in &node.children {
			if self.cancellation.is_cancelled() {
				return Err(map_archive_report(report!(ArchiveError::Cancelled)));
			}
			if self.archive_started.elapsed() > MAX_ARCHIVE_WORK {
				return Err(map_archive_report(report!(ArchiveError::WorkLimit)));
			}
			let condition = if child.name.eq_ignore_ascii_case("dependencies") {
				self.condition(child, scope, group_id, option_id, pattern_order)?
			} else if child.name.eq_ignore_ascii_case("flagDependency") {
				FomodCondition::FlagDependency {
					name: required_attribute(child, "flag")?.to_owned(),
					value: required_attribute(child, "value")?.to_owned(),
				}
			} else if child.name.eq_ignore_ascii_case("fileDependency") {
				let path = normalize_destination(required_attribute(child, "file")?)?;
				if path.is_empty() {
					return Err(report!(ErrorMarker::unsupported_installer()));
				}
				let path = data_path(path)?.as_str().to_owned();
				let state = parse_file_state(required_attribute(child, "state")?)?;
				FomodCondition::FileDependency { path, state }
			} else if child.name.eq_ignore_ascii_case("gameDependency") {
				FomodCondition::GameDependency {
					minimum_version: version(required_attribute(child, "version")?)?,
				}
			} else if child.name.eq_ignore_ascii_case("nvseDependency") {
				FomodCondition::NvseDependency {
					minimum_version: version(required_attribute(child, "version")?)?,
				}
			} else if child.name.eq_ignore_ascii_case("fommDependency") {
				FomodCondition::FommDependency {
					minimum_version: version(required_attribute(child, "version")?)?,
				}
			} else {
				return Err(report!(ErrorMarker::unsupported_installer()));
			};
			conditions.push(condition);
		}
		if conditions.is_empty() {
			self.warnings.push(InstallWarning::FomodEmptyConditionList {
				operator,
				scope,
				group_id: group_id.map(str::to_owned),
				option_id: option_id.map(str::to_owned),
				pattern_order,
			});
		}
		Ok(match operator {
			ConditionOperator::And => FomodCondition::All(conditions),
			ConditionOperator::Or => FomodCondition::Any(conditions),
		})
	}
}

async fn extract_for_application(
	archive: ArchivePath,
	identity: ArchiveIdentity,
	candidates: Vec<InstallCandidate>,
	begin_file: BeginInstallationFile,
	cancellation: CancellationToken,
) -> Result<(), ErrorMarker> {
	let started = Instant::now();
	if cancellation.is_cancelled() {
		return Err(report!(ErrorMarker::operation_cancelled()));
	}
	let index_cancellation = cancellation.clone();
	let indexed = spawn_blocking(move || {
		let core = index_archive(archive.as_path(), &index_cancellation).map_err(map_archive_report)?;
		let actual_identity = identity_for_extraction(&core, &index_cancellation)?;
		Ok((core, actual_identity))
	})
	.await
	.context(ArchiveError::Io)
	.map_err(map_archive_report)?;
	let (core, actual_identity) = indexed?;
	if actual_identity != identity {
		return Err(report!(ErrorMarker::unsafe_archive()));
	}

	let mut source_members = HashMap::<String, usize>::with_capacity(core.members.len());
	for member in &core.members {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}
		if started.elapsed() > MAX_ARCHIVE_WORK {
			return Err(map_archive_report(report!(ArchiveError::WorkLimit)));
		}
		if member.kind != MemberKind::File {
			continue;
		}
		if source_members
			.insert(member.path.as_str().to_owned(), member.ordinal)
			.is_some()
		{
			return Err(report!(ErrorMarker::unsafe_archive()));
		}
	}

	let mut destinations = BTreeMap::<usize, Vec<DataRelativePath>>::new();
	let mut destination_keys = BTreeSet::new();
	let mut destination_aliases = BTreeMap::<String, String>::new();
	for candidate in candidates {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}
		if started.elapsed() > MAX_ARCHIVE_WORK {
			return Err(map_archive_report(report!(ArchiveError::WorkLimit)));
		}
		let destination_key = candidate.destination.comparison_key().to_owned();
		if !destination_keys.insert(destination_key.clone()) {
			return Err(report!(ErrorMarker::ambiguous_install_plan()));
		}
		if destination_aliases
			.insert(
				normalization_alias_key(candidate.destination.as_str()),
				destination_key.clone(),
			)
			.is_some_and(|existing| existing != destination_key)
		{
			return Err(report!(ErrorMarker::unsafe_archive()));
		}
		let ordinal = source_members
			.get(&candidate.source_member)
			.copied()
			.ok_or_else(|| report!(ErrorMarker::unsafe_archive()))?;
		let member_destinations = destinations.entry(ordinal).or_default();
		if member_destinations.len() >= MAX_SOURCE_DESTINATION_FAN_OUT {
			return Err(map_archive_report(report!(ArchiveError::ExpansionLimit)));
		}
		member_destinations.push(candidate.destination);
	}
	let mut staged_output = StagedOutputBudget::new();
	let mut ordinals = Vec::with_capacity(destinations.len());
	for (ordinal, member_destinations) in &destinations {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}
		if started.elapsed() > MAX_ARCHIVE_WORK {
			return Err(map_archive_report(report!(ArchiveError::WorkLimit)));
		}
		let member = core
			.members
			.get(*ordinal)
			.ok_or_else(|| report!(ErrorMarker::unsafe_archive()))?;
		staged_output
			.charge(member.uncompressed_size, member_destinations.len())
			.map_err(map_archive_report)?;
		ordinals.push(*ordinal);
	}
	// Archive crates write synchronously while installation files are async.
	// The bounded channel caps bridge memory.
	let (sender, mut receiver) = channel::<ExtractionEvent>(8);
	let producer_cancellation = cancellation.child_token();
	let producer_token = producer_cancellation.clone();
	let producer = spawn_blocking(move || {
		extract_selected(&core, &ordinals, &producer_token, |member| {
			let member_destinations = destinations
				.get(&member.ordinal)
				.cloned()
				.ok_or_else(|| report!(ArchiveError::MissingMember))?;
			sender.blocking_send(ExtractionEvent::Start(member_destinations))
				.context(ArchiveError::Io)?;
			Ok(Box::new(ChannelWriter { sender: sender.clone() }))
		})
	});

	let mut current_files = Vec::new();
	while let Some(event) = receiver.recv().await {
		let result: Result<(), ErrorMarker> = async {
			if cancellation.is_cancelled() {
				return Err(report!(ErrorMarker::operation_cancelled()));
			}
			match event {
				ExtractionEvent::Start(destinations) => {
					finish_files(take(&mut current_files), &cancellation).await?;
					for destination in destinations {
						if cancellation.is_cancelled() {
							return Err(report!(ErrorMarker::operation_cancelled()));
						}
						let file = begin_file.call((destination, cancellation.clone())).await?;
						current_files.push(file);
					}
				}
				ExtractionEvent::Chunk(chunk) => {
					for file in &current_files {
						if cancellation.is_cancelled() {
							return Err(report!(ErrorMarker::operation_cancelled()));
						}
						file.write_chunk.call((chunk.clone(), cancellation.clone())).await?;
					}
				}
			}
			Ok(())
		}
		.await;
		if let Err(error) = result {
			producer_cancellation.cancel();
			drop(receiver);
			let _ = producer.await;
			return Err(error);
		}
	}
	let producer_result = producer.await.context(ArchiveError::Io).map_err(map_archive_report)?;
	producer_result.map_err(map_archive_report)?;
	finish_files(current_files, &cancellation).await
}

async fn finish_files(files: Vec<InstallationFile>, cancellation: &CancellationToken) -> Result<(), ErrorMarker> {
	for file in files {
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}
		file.finish.call((cancellation.clone(),)).await?;
	}
	Ok(())
}

fn identity_for_extraction(
	core: &ArchiveIndexCore,
	cancellation: &CancellationToken,
) -> Result<ArchiveIdentity, ErrorMarker> {
	let archive_sha256 = digest(core.sha256)?;
	let package_root = core.discovery.package_root.join("/");
	match core.discovery.configuration_ordinal {
		None => Ok(ArchiveIdentity::DataArchive {
			archive_sha256,
			package_root,
		}),
		Some(ordinal) => {
			let configuration = read_member_bounded(core, ordinal, MAX_XML_BYTES, cancellation)
				.map_err(map_archive_report)?;
			Ok(ArchiveIdentity::Fomod {
				archive_sha256,
				package_root,
				config_member: core.members[ordinal].path.as_str().to_owned(),
				config_sha256: digest(Sha256::digest(configuration).into())?,
			})
		}
	}
}

enum ExtractionEvent {
	Start(Vec<DataRelativePath>),
	Chunk(Vec<u8>),
}

struct ChannelWriter {
	sender: Sender<ExtractionEvent>,
}
impl Write for ChannelWriter {
	fn write(&mut self, bytes: &[u8]) -> IoResult<usize> {
		for chunk in bytes.chunks(COPY_BUFFER_BYTES) {
			self.sender
				.blocking_send(ExtractionEvent::Chunk(chunk.to_vec()))
				.map_err(IoError::other)?;
		}
		Ok(bytes.len())
	}
	fn flush(&mut self) -> IoResult<()> {
		Ok(())
	}
}

fn digest(bytes: [u8; 32]) -> Result<Sha256Digest, ErrorMarker> {
	let value = bytes.iter().map(|byte| format!("{byte:02x}")).collect::<String>();
	Sha256Digest::new(value)
		.context(ArchiveError::InvalidArchive)
		.map_err(map_archive_report)
}

fn map_archive_report(error: Report<ArchiveError>) -> Report<ErrorMarker> {
	let marker = match error.current_context() {
		ArchiveError::Cancelled => ErrorMarker::operation_cancelled(),
		ArchiveError::Io => ErrorMarker::io_failure(),
		ArchiveError::UnsupportedFormat | ArchiveError::UnsupportedInstaller => {
			ErrorMarker::unsupported_installer()
		}
		_ => ErrorMarker::unsafe_archive(),
	};
	error.context(marker)
}

fn data_path(value: String) -> Result<DataRelativePath, ErrorMarker> {
	DataRelativePath::new(value)
		.context(ArchiveError::UnsafePath)
		.map_err(map_archive_report)
}

fn data_destination(value: String) -> Result<DataRelativePath, ErrorMarker> {
	let value = SafeArchivePath::new(&value)
		.map(|path| path.as_str().to_owned())
		.map_err(map_archive_report)?;
	let destination = data_path(value)?;
	let first_component = destination.as_str().split('/').next().unwrap_or_default();
	if ["meta.toml", "Fallout - Invalidation.bsa"]
		.iter()
		.any(|reserved| case_fold_key(first_component) == case_fold_key(reserved))
	{
		return Err(report!(ErrorMarker::unsafe_archive()));
	}
	Ok(destination)
}

fn children<'a>(node: &'a NormalizedElement, name: &str) -> Vec<&'a NormalizedElement> {
	node.children
		.iter()
		.filter(|child| child.name.eq_ignore_ascii_case(name))
		.collect()
}
fn required_child<'a>(node: &'a NormalizedElement, name: &str) -> Result<&'a NormalizedElement, ErrorMarker> {
	optional_child(node, name)?.ok_or_else(|| report!(ErrorMarker::unsupported_installer()))
}
fn optional_child<'a>(node: &'a NormalizedElement, name: &str) -> Result<Option<&'a NormalizedElement>, ErrorMarker> {
	let found = children(node, name);
	if found.len() > 1 {
		return Err(report!(ErrorMarker::unsupported_installer()));
	}
	Ok(found.into_iter().next())
}
fn optional_attribute<'a>(node: &'a NormalizedElement, name: &str) -> Option<&'a str> {
	node.attributes
		.iter()
		.find(|attribute| attribute.namespace.is_none() && attribute.name.eq_ignore_ascii_case(name))
		.map(|attribute| attribute.value.as_str())
}
fn required_attribute<'a>(node: &'a NormalizedElement, name: &str) -> Result<&'a str, ErrorMarker> {
	optional_attribute(node, name).ok_or_else(|| report!(ErrorMarker::unsupported_installer()))
}
fn boolean_attribute(node: &NormalizedElement, name: &str) -> Result<Option<bool>, ErrorMarker> {
	optional_attribute(node, name)
		.map(|value| match value {
			value if value.eq_ignore_ascii_case("true") || value == "1" => Ok(true),
			value if value.eq_ignore_ascii_case("false") || value == "0" => Ok(false),
			_ => Err(report!(ErrorMarker::unsupported_installer())),
		})
		.transpose()
}
fn ordered_children<'a>(
	container: &'a NormalizedElement,
	name: &str,
) -> Result<Vec<&'a NormalizedElement>, ErrorMarker> {
	let mut values = children(container, name);
	match optional_attribute(container, "order").unwrap_or("Ascending") {
		value if value.eq_ignore_ascii_case("Explicit") => {}
		value if value.eq_ignore_ascii_case("Ascending") => {
			values.sort_by_key(|node| case_fold_key(optional_attribute(node, "name").unwrap_or_default()))
		}
		value if value.eq_ignore_ascii_case("Descending") => {
			values.sort_by_key(|node| case_fold_key(optional_attribute(node, "name").unwrap_or_default()));
			values.reverse();
		}
		_ => return Err(report!(ErrorMarker::unsupported_installer())),
	}
	Ok(values)
}
fn parse_cardinality(value: &str) -> Result<FomodCardinality, ErrorMarker> {
	match value.to_ascii_lowercase().as_str() {
		"selectexactlyone" => Ok(FomodCardinality::SelectExactlyOne),
		"selectatmostone" => Ok(FomodCardinality::SelectAtMostOne),
		"selectatleastone" => Ok(FomodCardinality::SelectAtLeastOne),
		"selectany" => Ok(FomodCardinality::SelectAny),
		"selectall" => Ok(FomodCardinality::SelectAll),
		_ => Err(report!(ErrorMarker::unsupported_installer())),
	}
}
fn parse_option_type(value: &str) -> Result<ResolvedOptionType, ErrorMarker> {
	match value.to_ascii_lowercase().as_str() {
		"required" => Ok(ResolvedOptionType::Required),
		"notusable" => Ok(ResolvedOptionType::NotUsable),
		"recommended" => Ok(ResolvedOptionType::Recommended),
		"optional" => Ok(ResolvedOptionType::Optional),
		"couldbeusable" => Ok(ResolvedOptionType::CouldBeUsable),
		_ => Err(report!(ErrorMarker::unsupported_installer())),
	}
}
fn parse_file_state(value: &str) -> Result<FileDependencyState, ErrorMarker> {
	match value.to_ascii_lowercase().as_str() {
		"missing" => Ok(FileDependencyState::Missing),
		"inactive" => Ok(FileDependencyState::Inactive),
		"active" => Ok(FileDependencyState::Active),
		_ => Err(report!(ErrorMarker::unsupported_installer())),
	}
}
fn version(raw: &str) -> Result<String, ErrorMarker> {
	if raw.is_empty()
		|| raw.split('.')
			.any(|part| part.is_empty() || part.parse::<u32>().is_err())
	{
		return Err(report!(ErrorMarker::unsupported_installer()));
	}
	Ok(raw.to_owned())
}

fn normalize_destination(raw: &str) -> Result<String, ErrorMarker> {
	let replaced = raw.replace('\\', "/");
	let trimmed = replaced.trim_end_matches('/');
	let stripped =
		if case_fold_key(trimmed) == "data" {
			""
		} else {
			trimmed.split_once('/').map_or(trimmed, |(first, rest)| {
				if case_fold_key(first) == "data" { rest } else { trimmed }
			})
		};
	if stripped.is_empty() {
		Ok(String::new())
	} else {
		SafeArchivePath::new(stripped)
			.map(|path| path.as_str().to_owned())
			.map_err(map_archive_report)
	}
}
fn join_destination(prefix: &str, suffix: &str) -> String {
	if prefix.is_empty() {
		suffix.to_owned()
	} else {
		format!("{prefix}/{suffix}")
	}
}
fn join_components(prefix: &[String], suffix: &[String]) -> Vec<String> {
	prefix.iter().chain(suffix).cloned().collect()
}
fn normalized_components_key(components: &[String]) -> String {
	case_fold_key(&components.join("/"))
}
fn components_start_with(path: &[String], prefix: &[String]) -> bool {
	path.len() >= prefix.len()
		&& path.iter()
			.zip(prefix)
			.all(|(left, right)| case_fold_key(left) == case_fold_key(right))
}
fn identifier(label: &str, fallback: &str) -> String {
	let mut result = String::new();
	let mut needs_separator = false;
	for character in label.chars() {
		if character.is_ascii_alphanumeric() {
			if needs_separator && !result.is_empty() {
				result.push('-');
			}
			result.push(character.to_ascii_lowercase());
			needs_separator = false;
		} else {
			needs_separator = true;
		}
	}
	if result.is_empty() { fallback.to_owned() } else { result }
}

fn unique_identifier(base: String, used: &mut BTreeSet<String>) -> String {
	if used.insert(base.clone()) {
		return base;
	}
	let mut suffix = 2_u64;
	loop {
		let candidate = format!("{base}-{suffix}");
		if used.insert(candidate.clone()) {
			return candidate;
		}
		suffix = suffix.saturating_add(1);
	}
}

fn source_suffix(
	core: &ArchiveIndexCore,
	member: &ArchiveMember,
	source: &SafeArchivePath,
) -> Result<String, ErrorMarker> {
	for root in [&core.discovery.package_root, &core.discovery.data_root] {
		let prefix = join_components(root, source.components());
		if components_start_with(member.path.components(), &prefix)
			&& member.path.components().len() > prefix.len()
		{
			return Ok(member.path.components()[prefix.len()..].join("/"));
		}
	}
	Err(report!(ErrorMarker::unsafe_archive()))
}

#[cfg(test)]
mod tests {
	use super::ArchiveAdapter;
	use super::FomodBudget;
	use super::FomodConverter;
	use super::StagedOutputBudget;
	use super::data_destination;
	use super::join_destination;
	use super::plain_candidates;
	use crate::error::ArchiveError;
	use crate::fomod::parse;
	use crate::index::index_archive;
	use crate::limits::COPY_BUFFER_BYTES;
	use crate::limits::MAX_SOURCE_DESTINATION_FAN_OUT;
	use crate::path::SafeArchivePath;
	use application::ErrorCode;
	use application::ErrorMarker;
	use application::installation::FomodInstaller;
	use application::installation::IndexedInstaller;
	use application::installation::InstallWarning;
	use application::installation::MalformedGroupRepair;
	use application::ports::BeginInstallationFile;
	use application::ports::InstallationFile;
	use application::ports::PortFuture;
	use domain::ArchivePath;
	use domain::DataRelativePath;
	use domain::FomodCardinality;
	use rars::ArchiveVersion;
	use rars::FeatureSet;
	use rars::rar15_40::StoredEntry;
	use rars::rar15_40::WriterOptions;
	use rars::rar15_40::write_stored_archive;
	use rootcause::report;
	use sevenz_rust2::ArchiveEntry;
	use sevenz_rust2::ArchiveWriter;
	use std::collections::BTreeMap;
	use std::error::Error;
	use std::fs::File;
	use std::fs::read;
	use std::fs::write as write_file;
	use std::io::Cursor;
	use std::io::Error as IoError;
	use std::io::ErrorKind as IoErrorKind;
	use std::io::Write;
	use std::path::Path;
	use std::sync::Arc;
	use std::sync::Mutex;
	use std::time::Instant;
	use tempfile::TempDir;
	use tokio_util::sync::CancellationToken;
	use zip::CompressionMethod;
	use zip::ZipWriter;
	use zip::write::SimpleFileOptions;
	type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

	fn archive_path(path: &Path) -> TestResult<ArchivePath> {
		Ok(ArchivePath::new(path.to_path_buf())?)
	}

	fn write_zip(path: &Path, entries: &[(&str, &[u8])]) -> TestResult {
		let mut writer = ZipWriter::new(File::create(path)?);
		for (name, contents) in entries {
			writer.start_file(*name, SimpleFileOptions::default())?;
			writer.write_all(contents)?;
		}
		writer.finish()?;
		Ok(())
	}

	fn single_option_installer(type_descriptor: &str) -> TestResult<FomodInstaller> {
		let temp = TempDir::new()?;
		let path = temp.path().join("single-option.zip");
		write_zip(&path, &[("Data/payload.txt", b"payload")])?;
		let cancellation = CancellationToken::new();
		let core = index_archive(&path, &cancellation)?;
		let xml = format!(
			concat!(
				r#"<config version="5.0"><moduleName>Single option</moduleName>"#,
				r#"<installSteps order="Explicit"><installStep name="Step">"#,
				r#"<optionalFileGroups order="Explicit">"#,
				r#"<group name="Choice" type="SelectExactlyOne"><plugins order="Explicit">"#,
				r#"<plugin name="Only"><typeDescriptor>{}</typeDescriptor></plugin>"#,
				"</plugins></group></optionalFileGroups></installStep></installSteps></config>",
			),
			type_descriptor,
		);
		let document = parse(xml.as_bytes(), &cancellation)?;
		let mut converter = FomodConverter::new(&core, &cancellation, Instant::now())?;
		Ok(converter.convert(&document)?)
	}

	#[test]
	fn single_not_usable_option_preserves_exactly_one_for_no_choice_error() -> TestResult {
		let installer = single_option_installer(r#"<type name="NotUsable" />"#)?;

		assert_eq!(installer.groups[0].cardinality, FomodCardinality::SelectExactlyOne);
		assert!(!installer.warnings.iter().any(|warning| matches!(
			warning,
			InstallWarning::FomodMalformedGroupRepaired {
				repair: MalformedGroupRepair::SingleOptionExactlyOneToSelectAll,
				..
			}
		)));
		Ok(())
	}

	#[test]
	fn dynamic_not_usable_option_type_prevents_single_option_repair() -> TestResult {
		for type_descriptor in [
			concat!(
				r#"<dependencyType><defaultType name="NotUsable" /><patterns><pattern>"#,
				r#"<dependencies><gameDependency version="1" /></dependencies>"#,
				r#"<type name="Optional" /></pattern></patterns></dependencyType>"#,
			),
			concat!(
				r#"<dependencyType><defaultType name="Optional" /><patterns>"#,
				r#"<pattern><dependencies><gameDependency version="1" /></dependencies>"#,
				r#"<type name="Required" /></pattern>"#,
				r#"<pattern><dependencies><gameDependency version="2" /></dependencies>"#,
				r#"<type name="NotUsable" /></pattern></patterns></dependencyType>"#,
			),
		] {
			let installer = single_option_installer(type_descriptor)?;

			assert_eq!(installer.groups[0].cardinality, FomodCardinality::SelectExactlyOne);
			assert!(!installer.warnings.iter().any(|warning| matches!(
				warning,
				InstallWarning::FomodMalformedGroupRepaired {
					repair: MalformedGroupRepair::SingleOptionExactlyOneToSelectAll,
					..
				}
			)));
		}
		Ok(())
	}

	#[test]
	fn guaranteed_usable_single_option_is_repaired_with_warning() -> TestResult {
		for type_descriptor in [
			r#"<type name="Optional" />"#,
			concat!(
				r#"<dependencyType><defaultType name="Optional" /><patterns><pattern>"#,
				r#"<dependencies><gameDependency version="1" /></dependencies>"#,
				r#"<type name="Required" /></pattern></patterns></dependencyType>"#,
			),
		] {
			let installer = single_option_installer(type_descriptor)?;

			assert_eq!(installer.groups[0].cardinality, FomodCardinality::SelectAll);
			assert!(installer.warnings.iter().any(|warning| matches!(
				warning,
				InstallWarning::FomodMalformedGroupRepaired {
					group_id,
					repair: MalformedGroupRepair::SingleOptionExactlyOneToSelectAll,
				} if group_id == "step.choice"
			)));
		}
		Ok(())
	}

	#[test]
	fn fomod_conversion_budget_enforces_work_and_candidate_limits() -> TestResult {
		let mut budget = FomodBudget {
			derived_work: 0,
			derived_candidates: 0,
			max_derived_work: 1,
			max_derived_candidates: 1,
		};
		budget.charge_work()?;
		let Err(work) = budget.charge_work() else {
			return Err("work cap was not enforced".into());
		};
		assert_eq!(work.current_context().code(), ErrorCode::UnsafeArchive);

		budget.charge_candidate()?;
		let Err(candidates) = budget.charge_candidate() else {
			return Err("candidate cap was not enforced".into());
		};
		assert_eq!(candidates.current_context().code(), ErrorCode::UnsafeArchive);
		Ok(())
	}

	#[test]
	fn fomod_conversion_reports_cancellation_with_its_archive_cause() -> TestResult {
		let temp = TempDir::new()?;
		let path = temp.path().join("fomod-cancellation.zip");
		write_zip(&path, &[("Data/file.txt", b"data")])?;
		let cancellation = CancellationToken::new();
		let core = index_archive(&path, &cancellation)?;
		let xml = concat!(
			r#"<config version="5.0"><moduleName>Cancelled</moduleName>"#,
			r#"<requiredInstallFiles><file source="Data/file.txt" />"#,
			"</requiredInstallFiles></config>",
		);
		let document = parse(xml.as_bytes(), &cancellation)?;
		let mut converter = FomodConverter::new(&core, &cancellation, Instant::now())?;
		cancellation.cancel();

		let Err(error) = converter.convert(&document) else {
			return Err("cancelled FOMOD conversion continued".into());
		};
		assert_eq!(error.current_context().code(), ErrorCode::OperationCancelled);
		assert!(error.iter_reports().any(|node| {
			node.downcast_current_context::<ArchiveError>() == Some(&ArchiveError::Cancelled)
		}));
		Ok(())
	}

	#[test]
	fn plain_candidate_conversion_reports_cancellation_with_its_archive_cause() -> TestResult {
		let temp = TempDir::new()?;
		let path = temp.path().join("plain-cancellation.zip");
		write_zip(&path, &[("Data/file.txt", b"data")])?;
		let core = index_archive(&path, &CancellationToken::new())?;
		let cancellation = CancellationToken::new();
		cancellation.cancel();

		let Err(error) = plain_candidates(&core, &cancellation, Instant::now()) else {
			return Err("cancelled plain candidate conversion continued".into());
		};
		assert_eq!(error.current_context().code(), ErrorCode::OperationCancelled);
		assert!(error.iter_reports().any(|node| {
			node.downcast_current_context::<ArchiveError>() == Some(&ArchiveError::Cancelled)
		}));
		Ok(())
	}

	#[test]
	fn staged_output_budget_counts_fan_out_and_rejects_overflow() -> TestResult {
		let mut budget = StagedOutputBudget { bytes: 0, limit: 10 };
		budget.charge(4, 2)?;
		budget.charge(1, 2)?;
		assert!(budget.charge(1, 1).is_err());

		let mut overflow = StagedOutputBudget {
			bytes: 0,
			limit: u64::MAX,
		};
		assert!(overflow.charge(u64::MAX, 2).is_err());
		Ok(())
	}

	#[test]
	fn final_fomod_destinations_revalidate_combined_archive_path_caps() -> TestResult {
		let component = "a".repeat(220);
		let byte_prefix = [component.as_str(), component.as_str()].join("/");
		let byte_suffix = [component.as_str(), component.as_str(), component.as_str()].join("/");
		SafeArchivePath::new(&byte_prefix)?;
		SafeArchivePath::new(&byte_suffix)?;
		assert!(data_destination(join_destination(&byte_prefix, &byte_suffix)).is_err());

		let component_prefix = (0..32).map(|index| format!("p{index}")).collect::<Vec<_>>().join("/");
		let component_suffix = (0..33).map(|index| format!("s{index}")).collect::<Vec<_>>().join("/");
		SafeArchivePath::new(&component_prefix)?;
		SafeArchivePath::new(&component_suffix)?;
		assert!(data_destination(join_destination(&component_prefix, &component_suffix)).is_err());
		Ok(())
	}

	#[tokio::test]
	async fn io_cause_remains_beneath_archive_context_and_application_marker() -> TestResult {
		let temp = TempDir::new()?;
		let missing = temp.path().join("missing.zip");
		let report = match ArchiveAdapter
			.index_port()
			.call((archive_path(&missing)?, CancellationToken::new()))
			.await
		{
			Ok(_) => return Err("missing archive unexpectedly indexed".into()),
			Err(report) => report,
		};

		assert_eq!(report.current_context().code(), ErrorCode::IoFailure);
		assert!(report
			.iter_reports()
			.any(|node| node.downcast_current_context::<ArchiveError>() == Some(&ArchiveError::Io)));
		let Some(io_error) = report
			.iter_reports()
			.find_map(|node| node.downcast_current_context::<IoError>())
		else {
			return Err("original I/O error was missing from the report tree".into());
		};
		assert_eq!(io_error.kind(), IoErrorKind::NotFound);
		Ok(())
	}

	#[tokio::test]
	async fn real_zip_indexes_and_streams_through_application_ports() -> TestResult {
		let temp = TempDir::new()?;
		let path = temp.path().join("fixture.fomod");
		let contents = vec![7_u8; COPY_BUFFER_BYTES * 3 + 17];
		write_zip(
			&path,
			&[
				("Data/meshes/a.nif", &contents),
				("fomod/info.xml", b"<fomod><Name>Decoration</Name></fomod>"),
			],
		)?;
		let adapter = ArchiveAdapter;
		let cancellation = CancellationToken::new();
		let index = adapter
			.index_port()
			.call((archive_path(&path)?, cancellation.clone()))
			.await?;
		let IndexedInstaller::Plain { candidates, .. } = index.installer else {
			return Err("plain ZIP became a FOMOD".into());
		};
		assert_eq!(candidates.len(), 1);
		assert_eq!(candidates[0].destination.as_str(), "meshes/a.nif");

		let written = Arc::new(Mutex::new(BTreeMap::<String, Vec<u8>>::new()));
		let finished = Arc::new(Mutex::new(Vec::<String>::new()));
		let begin = Arc::new({
			let written = Arc::clone(&written);
			let finished = Arc::clone(&finished);
			move |destination: DataRelativePath, _| {
				let key = destination.as_str().to_owned();
				let write_key = key.clone();
				let write_state = Arc::clone(&written);
				let finish_state = Arc::clone(&finished);
				let write_chunk = Arc::new(move |chunk: Vec<u8>, _| {
					assert!(chunk.len() <= COPY_BUFFER_BYTES);
					write_state
						.lock()
						.unwrap_or_else(|poisoned| poisoned.into_inner())
						.entry(write_key.clone())
						.or_default()
						.extend(chunk);
					Box::pin(async { Ok(()) }) as PortFuture<_>
				});
				let finish = Arc::new(move |_| {
					finish_state
						.lock()
						.unwrap_or_else(|poisoned| poisoned.into_inner())
						.push(key.clone());
					Box::pin(async { Ok(()) }) as PortFuture<_>
				});
				Box::pin(async move { Ok(InstallationFile { write_chunk, finish }) }) as PortFuture<_>
			}
		});
		adapter.extract_port()
			.call((archive_path(&path)?, index.identity, candidates, begin, cancellation))
			.await?;
		assert_eq!(
			written.lock()
				.unwrap_or_else(|poisoned| poisoned.into_inner())
				.get("meshes/a.nif"),
			Some(&contents)
		);
		assert_eq!(
			*finished.lock().unwrap_or_else(|poisoned| poisoned.into_inner()),
			["meshes/a.nif"]
		);
		Ok(())
	}

	#[tokio::test]
	async fn real_seven_zip_and_rar_fixtures_index_through_the_adapter() -> TestResult {
		let temp = TempDir::new()?;
		let seven_path = temp.path().join("fixture.7z");
		let mut seven_writer = ArchiveWriter::create(&seven_path)?;
		seven_writer
			.push_archive_entry(ArchiveEntry::new_file("Data/seven.txt"), Some(Cursor::new(b"seven")))?;
		seven_writer.finish()?;

		let rar_path = temp.path().join("fixture.rar");
		let rar = write_stored_archive(
			&[StoredEntry {
				name: b"Data/rar.txt",
				data: b"rar",
				file_time: 0,
				file_attr: 0x20,
				host_os: 3,
				password: None,
				file_comment: None,
			}],
			WriterOptions::new(ArchiveVersion::Rar15, FeatureSet::store_only()),
		)?;
		write_file(&rar_path, rar)?;

		let adapter = ArchiveAdapter;
		let index = adapter.index_port();
		for (path, expected_destination, expected_contents) in [
			(seven_path, "seven.txt", b"seven".as_slice()),
			(rar_path, "rar.txt", b"rar".as_slice()),
		] {
			let indexed = index.call((archive_path(&path)?, CancellationToken::new())).await?;
			let IndexedInstaller::Plain { candidates, .. } = indexed.installer else {
				return Err("fixture became a FOMOD".into());
			};
			assert_eq!(candidates[0].destination.as_str(), expected_destination);

			let written = Arc::new(Mutex::new(Vec::new()));
			let begin = Arc::new({
				let written = Arc::clone(&written);
				move |_, _| {
					let written = Arc::clone(&written);
					let write_chunk = Arc::new(move |chunk: Vec<u8>, _| {
						written.lock()
							.unwrap_or_else(|poisoned| poisoned.into_inner())
							.extend(chunk);
						Box::pin(async { Ok(()) }) as PortFuture<_>
					});
					let finish = Arc::new(move |_| Box::pin(async { Ok(()) }) as PortFuture<_>);
					Box::pin(async move { Ok(InstallationFile { write_chunk, finish }) })
						as PortFuture<_>
				}
			});
			adapter.extract_port()
				.call((
					archive_path(&path)?,
					indexed.identity,
					candidates,
					begin,
					CancellationToken::new(),
				))
				.await?;
			assert_eq!(
				written.lock()
					.unwrap_or_else(|poisoned| poisoned.into_inner())
					.as_slice(),
				expected_contents
			);
		}
		Ok(())
	}

	#[tokio::test]
	async fn renamed_fomod_parses_hardened_xml_and_builds_required_candidates() -> TestResult {
		let temp = TempDir::new()?;
		let path = temp.path().join("installer.fomod");
		let xml = br#"<?xml version="1.0" encoding="UTF-8"?>
<config xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:noNamespaceSchemaLocation="XmlScript5.0.xsd">
  <moduleName>Fixture</moduleName>
  <requiredInstallFiles><file source="Data/a.txt" destination="Data/a.txt" /></requiredInstallFiles>
</config>"#;
		write_zip(&path, &[("fomod/ModuleConfig.xml", xml), ("Data/a.txt", b"payload")])?;
		let index = ArchiveAdapter
			.index_port()
			.call((archive_path(&path)?, CancellationToken::new()))
			.await?;
		let IndexedInstaller::Fomod(installer) = index.installer else {
			return Err("configuration was not recognized".into());
		};
		assert_eq!(installer.schema_version, "5.0");
		assert_eq!(installer.required_candidates[0].destination.as_str(), "a.txt");
		Ok(())
	}

	#[tokio::test]
	async fn fomod_source_and_destination_identity_use_simple_unicode_case_folding() -> TestResult {
		let temp = TempDir::new()?;
		let path = temp.path().join("unicode-case.zip");
		let xml = r#"<config version="5.0"><moduleName>Unicode Case</moduleName>
  <requiredInstallFiles>
    <file source="Data/é.txt" destination="Σ.txt" />
    <file source="Data/other.txt" destination="ς.txt" />
  </requiredInstallFiles>
</config>"#;
		write_zip(
			&path,
			&[
				("fomod/ModuleConfig.xml", xml.as_bytes()),
				("Data/É.txt", b"case-folded source"),
				("Data/other.txt", b"other"),
			],
		)?;

		let index = ArchiveAdapter
			.index_port()
			.call((archive_path(&path)?, CancellationToken::new()))
			.await?;
		let IndexedInstaller::Fomod(installer) = index.installer else {
			return Err("configuration was not recognized".into());
		};
		assert_eq!(installer.required_candidates[0].source_member, "Data/É.txt");
		assert_eq!(
			installer.required_candidates[0].destination,
			installer.required_candidates[1].destination
		);
		Ok(())
	}

	#[tokio::test]
	async fn reserved_data_root_destinations_fail_during_plain_and_fomod_indexing() -> TestResult {
		let temp = TempDir::new()?;
		let index = ArchiveAdapter.index_port();
		for (case, reserved) in [
			("metadata", "MeTa.ToMl"),
			("metadata-descendant", "MeTa.ToMl/child.bin"),
			("invalidation", "fAlLoUt - iNvAlIdAtIoN.BsA"),
			("invalidation-descendant", "fAlLoUt - iNvAlIdAtIoN.BsA/child.bin"),
		] {
			let plain = temp.path().join(format!("plain-{case}.zip"));
			let plain_member = format!("Data/{reserved}");
			write_zip(&plain, &[(plain_member.as_str(), b"reserved")])?;
			let Err(error) = index.call((archive_path(&plain)?, CancellationToken::new())).await else {
				return Err("reserved plain destination produced an archive index".into());
			};
			assert_eq!(error.current_context().code(), ErrorCode::UnsafeArchive);

			let fomod = temp.path().join(format!("fomod-{case}.zip"));
			let xml = format!(
				concat!(
					r#"<config version="5.0"><moduleName>Reserved</moduleName>"#,
					"<requiredInstallFiles>",
					r#"<file source="payload.bin" destination="{}" />"#,
					"</requiredInstallFiles></config>",
				),
				reserved,
			);
			write_zip(
				&fomod,
				&[("fomod/ModuleConfig.xml", xml.as_bytes()), ("payload.bin", b"reserved")],
			)?;
			let Err(error) = index.call((archive_path(&fomod)?, CancellationToken::new())).await else {
				return Err("reserved FOMOD destination produced an archive index".into());
			};
			assert_eq!(error.current_context().code(), ErrorCode::UnsafeArchive);
		}
		Ok(())
	}

	#[tokio::test]
	async fn fomod_plan_rejects_unicode_normalization_alias_destinations() -> TestResult {
		let temp = TempDir::new()?;
		for (case, first, second) in [
			("canonical", "Café.txt", "Cafe\u{301}.txt"),
			("compatibility", "ﬃ.txt", "ffi.txt"),
		] {
			let path = temp.path().join(format!("alias-{case}.zip"));
			let xml = format!(
				concat!(
					r#"<config version="5.0"><moduleName>Alias</moduleName>"#,
					"<requiredInstallFiles>",
					r#"<file source="one.bin" destination="{}" />"#,
					r#"<file source="two.bin" destination="{}" />"#,
					"</requiredInstallFiles></config>",
				),
				first, second,
			);
			write_zip(
				&path,
				&[
					("fomod/ModuleConfig.xml", xml.as_bytes()),
					("one.bin", b"one"),
					("two.bin", b"two"),
				],
			)?;

			let Err(error) = ArchiveAdapter
				.index_port()
				.call((archive_path(&path)?, CancellationToken::new()))
				.await
			else {
				return Err("normalization aliases produced an archive plan".into());
			};
			assert_eq!(error.current_context().code(), ErrorCode::UnsafeArchive);
		}
		Ok(())
	}

	#[tokio::test]
	async fn ambiguous_fomod_file_and_folder_sources_fail_with_the_plan_code() -> TestResult {
		let temp = TempDir::new()?;
		for (case, element, source, entries) in [
			(
				"folder",
				"folder",
				"textures",
				[
					("textures/a.dds", b"a".as_slice()),
					("Data/textures/b.dds", b"b".as_slice()),
				],
			),
			(
				"file",
				"file",
				"a.txt",
				[("a.txt", b"a".as_slice()), ("Data/a.txt", b"b".as_slice())],
			),
		] {
			let path = temp.path().join(format!("ambiguous-{case}.zip"));
			let xml = format!(
				concat!(
					r#"<config version="5.0"><moduleName>Ambiguous</moduleName>"#,
					"<requiredInstallFiles>",
					r#"<{element} source="{source}" destination="output" />"#,
					"</requiredInstallFiles></config>",
				),
				element = element,
				source = source
			);
			let archive_entries = [("fomod/ModuleConfig.xml", xml.as_bytes()), entries[0], entries[1]];
			write_zip(&path, &archive_entries)?;

			let Err(error) = ArchiveAdapter
				.index_port()
				.call((archive_path(&path)?, CancellationToken::new()))
				.await
			else {
				return Err("ambiguous source produced an archive index".into());
			};
			assert_eq!(error.current_context().code(), ErrorCode::AmbiguousInstallPlan);
		}
		Ok(())
	}

	#[tokio::test]
	async fn fomod_uses_structural_ids_and_the_exact_folder_prefix() -> TestResult {
		let temp = TempDir::new()?;
		let path = temp.path().join("installer.zip");
		let xml = br#"<?xml version="1.0" encoding="UTF-8"?>
<config xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:noNamespaceSchemaLocation="ModConfig5.0.xsd">
  <moduleName>Fixture Module</moduleName>
  <installSteps order="Explicit">
    <installStep name="Core">
      <optionalFileGroups order="Explicit">
        <group name="Engine" type="SelectAny">
          <plugins order="Explicit">
            <plugin name="none"><typeDescriptor><type name="Optional" /></typeDescriptor></plugin>
            <plugin name="Enhanced Pack">
              <files><folder source="Wrapper" destination="textures" /></files>
              <typeDescriptor><type name="Recommended" /></typeDescriptor>
            </plugin>
          </plugins>
        </group>
      </optionalFileGroups>
    </installStep>
  </installSteps>
</config>"#;
		write_zip(
			&path,
			&[
				("Wrapper/fomod/ModuleConfig.xml", xml),
				("Wrapper/Wrapper/file.dds", b"texture"),
			],
		)?;

		let index = ArchiveAdapter
			.index_port()
			.call((archive_path(&path)?, CancellationToken::new()))
			.await?;
		let IndexedInstaller::Fomod(installer) = index.installer else {
			return Err("fixture was not a FOMOD".into());
		};
		assert_eq!(installer.groups[0].id, "core.engine");
		assert_eq!(installer.groups[0].options[0].id, "none-2");
		assert_eq!(installer.groups[0].options[1].id, "enhanced-pack");
		assert_eq!(
			installer.groups[0].options[1].file_candidates[0].destination.as_str(),
			"textures/file.dds"
		);
		Ok(())
	}

	#[tokio::test]
	async fn decorative_fomod_images_are_recognized_without_reading_or_resolving_members() -> TestResult {
		let temp = TempDir::new()?;
		let path = temp.path().join("images.zip");
		let xml = br#"<config version="5.0">
  <moduleName>Image Fixture</moduleName>
  <moduleImage path="../../outside.png" />
  <installSteps order="Explicit">
    <installStep name="Step"><optionalFileGroups order="Explicit">
      <group name="Group" type="SelectAny"><plugins order="Explicit">
        <plugin name="Option">
          <description>With an image</description>
          <image path="fomod/option.png" />
          <typeDescriptor><type name="Optional" /></typeDescriptor>
        </plugin>
      </plugins></group>
    </optionalFileGroups></installStep>
  </installSteps>
</config>"#;
		write_zip(
			&path,
			&[("fomod/ModuleConfig.xml", xml), ("fomod/option.png", b"not an image")],
		)?;

		let index = ArchiveAdapter
			.index_port()
			.call((archive_path(&path)?, CancellationToken::new()))
			.await?;
		let IndexedInstaller::Fomod(installer) = index.installer else {
			return Err("fixture was not a FOMOD".into());
		};
		assert_eq!(installer.groups[0].options[0].description, "With an image");
		Ok(())
	}

	#[tokio::test]
	async fn root_alias_prefers_module_config_and_does_not_read_info() -> TestResult {
		let temp = TempDir::new()?;
		let path = temp.path().join("aliases.fomod");
		let xml = concat!(
			r#"<config version="5.0"><moduleName>Fallback Name</moduleName>"#,
			r#"<requiredInstallFiles><file source="Data/a.txt" destination="a.txt" />"#,
			r#"</requiredInstallFiles></config>"#,
		)
		.as_bytes();
		write_zip(
			&path,
			&[
				("fomod/ModuleConfig.xml", xml),
				("fomod/script.xml", xml),
				("fomod/info.xml", b"<!DOCTYPE bad>"),
				("Data/a.txt", b"a"),
			],
		)?;

		let index = ArchiveAdapter
			.index_port()
			.call((archive_path(&path)?, CancellationToken::new()))
			.await?;
		let IndexedInstaller::Fomod(installer) = index.installer else {
			return Err("fixture was not a FOMOD".into());
		};
		assert!(installer.warnings.iter().any(|warning| matches!(
			warning,
			InstallWarning::FomodModuleConfigPreferred {
				selected_config_member,
				ignored_config_member,
			} if selected_config_member == "fomod/ModuleConfig.xml"
				&& ignored_config_member == "fomod/script.xml"
		)));
		Ok(())
	}

	#[tokio::test]
	async fn case_insensitive_wrapper_components_remain_in_one_data_root() -> TestResult {
		let temp = TempDir::new()?;
		let path = temp.path().join("case.zip");
		write_zip(&path, &[("Wrapper/Data/a.txt", b"a"), ("wrapper/data/b.txt", b"b")])?;

		let index = ArchiveAdapter
			.index_port()
			.call((archive_path(&path)?, CancellationToken::new()))
			.await?;
		let IndexedInstaller::Plain { candidates, .. } = index.installer else {
			return Err("fixture was not plain".into());
		};
		assert_eq!(
			candidates
				.iter()
				.map(|candidate| candidate.destination.as_str())
				.collect::<Vec<_>>(),
			["a.txt", "b.txt"]
		);
		Ok(())
	}

	#[tokio::test]
	async fn rejects_unsafe_archive_container_and_member_forms() -> TestResult {
		let temp = TempDir::new()?;
		let index = ArchiveAdapter.index_port();

		let traversal = temp.path().join("traversal.zip");
		write_zip(&traversal, &[("../escape.txt", b"escape")])?;
		assert!(index
			.call((archive_path(&traversal)?, CancellationToken::new()))
			.await
			.is_err());

		let symlink = temp.path().join("symlink.zip");
		let mut writer = ZipWriter::new(File::create(&symlink)?);
		writer.add_symlink("Data/link", "target", SimpleFileOptions::default())?;
		writer.finish()?;
		assert!(index
			.call((archive_path(&symlink)?, CancellationToken::new()))
			.await
			.is_err());

		let split = temp.path().join("split.zip");
		write_zip(&split, &[("Data/file.txt", b"data")])?;
		let mut split_bytes = read(&split)?;
		let footer = split_bytes
			.windows(4)
			.rposition(|signature| signature == b"PK\x05\x06")
			.ok_or("ZIP footer missing")?;
		split_bytes[footer + 4] = 1;
		write_file(&split, split_bytes)?;
		assert!(index
			.call((archive_path(&split)?, CancellationToken::new()))
			.await
			.is_err());

		let sfx = temp.path().join("sfx.rar");
		let rar = write_stored_archive(
			&[StoredEntry {
				name: b"Data/file.txt",
				data: b"data",
				file_time: 0,
				file_attr: 0x20,
				host_os: 3,
				password: None,
				file_comment: None,
			}],
			WriterOptions::new(ArchiveVersion::Rar15, FeatureSet::store_only()),
		)?;
		write_file(&sfx, [b"MZ".as_slice(), rar.as_slice()].concat())?;
		assert!(index
			.call((archive_path(&sfx)?, CancellationToken::new()))
			.await
			.is_err());

		let encrypted = temp.path().join("encrypted.rar");
		let rar = write_stored_archive(
			&[StoredEntry {
				name: b"Data/file.txt",
				data: b"data",
				file_time: 0,
				file_attr: 0x20,
				host_os: 3,
				password: Some(b"password"),
				file_comment: None,
			}],
			WriterOptions::new(ArchiveVersion::Rar15, FeatureSet::store_only()),
		)?;
		write_file(&encrypted, rar)?;
		assert!(index
			.call((archive_path(&encrypted)?, CancellationToken::new()))
			.await
			.is_err());

		let legacy = temp.path().join("legacy.omod");
		write_zip(&legacy, &[("Data/file.txt", b"data")])?;
		assert!(index
			.call((archive_path(&legacy)?, CancellationToken::new()))
			.await
			.is_err());

		let scripted = temp.path().join("scripted.zip");
		write_zip(&scripted, &[("fomod/script.cs", b"code"), ("Data/file.txt", b"data")])?;
		assert!(index
			.call((archive_path(&scripted)?, CancellationToken::new()))
			.await
			.is_err());

		let expansion = temp.path().join("expansion.zip");
		let mut writer = ZipWriter::new(File::create(&expansion)?);
		writer.start_file(
			"Data/zeros.bin",
			SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
		)?;
		writer.write_all(&vec![0_u8; 8 * 1024 * 1024])?;
		writer.finish()?;
		assert!(index
			.call((archive_path(&expansion)?, CancellationToken::new()))
			.await
			.is_err());

		Ok(())
	}

	#[tokio::test]
	async fn extraction_requires_the_exact_indexed_source_member() -> TestResult {
		let temp = TempDir::new()?;
		let path = temp.path().join("exact-source.zip");
		write_zip(&path, &[("Data/Exact.txt", b"payload")])?;
		let adapter = ArchiveAdapter;
		let indexed = adapter
			.index_port()
			.call((archive_path(&path)?, CancellationToken::new()))
			.await?;
		let IndexedInstaller::Plain { candidates, .. } = indexed.installer else {
			return Err("fixture was not plain".into());
		};
		let exact = candidates[0].clone();
		let mut missing = exact.clone();
		missing.source_member = "Data/exact.txt".to_owned();
		let opened = Arc::new(Mutex::new(0_usize));
		let begin: BeginInstallationFile = Arc::new({
			let opened = Arc::clone(&opened);
			move |_, _| {
				*opened.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) += 1;
				let write_chunk = Arc::new(move |_, _| Box::pin(async { Ok(()) }) as PortFuture<_>);
				let finish = Arc::new(move |_| Box::pin(async { Ok(()) }) as PortFuture<_>);
				Box::pin(async move { Ok(InstallationFile { write_chunk, finish }) }) as PortFuture<_>
			}
		});

		let Err(error) = adapter
			.extract_port()
			.call((
				archive_path(&path)?,
				indexed.identity.clone(),
				vec![missing],
				Arc::clone(&begin),
				CancellationToken::new(),
			))
			.await
		else {
			return Err("non-exact source member was accepted".into());
		};
		assert_eq!(error.current_context().code(), ErrorCode::UnsafeArchive);
		assert_eq!(*opened.lock().unwrap_or_else(|poisoned| poisoned.into_inner()), 0);

		adapter.extract_port()
			.call((
				archive_path(&path)?,
				indexed.identity,
				vec![exact],
				begin,
				CancellationToken::new(),
			))
			.await?;
		assert_eq!(*opened.lock().unwrap_or_else(|poisoned| poisoned.into_inner()), 1);
		Ok(())
	}

	#[tokio::test]
	async fn extraction_large_member_plan_honors_cancellation_and_selects_one() -> TestResult {
		const MEMBER_COUNT: usize = 4_096;

		let temp = TempDir::new()?;
		let path = temp.path().join("many-members.zip");
		let mut writer = ZipWriter::new(File::create(&path)?);
		for index in 0..MEMBER_COUNT {
			writer.start_file(format!("Data/{index:04}.txt"), SimpleFileOptions::default())?;
		}
		writer.finish()?;
		let adapter = ArchiveAdapter;
		let indexed = adapter
			.index_port()
			.call((archive_path(&path)?, CancellationToken::new()))
			.await?;
		let IndexedInstaller::Plain { candidates, .. } = indexed.installer else {
			return Err("fixture was not plain".into());
		};
		assert_eq!(candidates.len(), MEMBER_COUNT);
		let opened = Arc::new(Mutex::new(0_usize));
		let begin: BeginInstallationFile = Arc::new({
			let opened = Arc::clone(&opened);
			move |_, _| {
				*opened.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) += 1;
				let write_chunk = Arc::new(move |_, _| Box::pin(async { Ok(()) }) as PortFuture<_>);
				let finish = Arc::new(move |_| Box::pin(async { Ok(()) }) as PortFuture<_>);
				Box::pin(async move { Ok(InstallationFile { write_chunk, finish }) }) as PortFuture<_>
			}
		});
		let cancellation = CancellationToken::new();
		cancellation.cancel();
		let Err(error) = adapter
			.extract_port()
			.call((
				archive_path(&path)?,
				indexed.identity.clone(),
				candidates.clone(),
				Arc::clone(&begin),
				cancellation,
			))
			.await
		else {
			return Err("cancelled large member plan continued".into());
		};
		assert_eq!(error.current_context().code(), ErrorCode::OperationCancelled);
		assert_eq!(*opened.lock().unwrap_or_else(|poisoned| poisoned.into_inner()), 0);

		adapter.extract_port()
			.call((
				archive_path(&path)?,
				indexed.identity,
				candidates,
				begin,
				CancellationToken::new(),
			))
			.await?;
		assert_eq!(
			*opened.lock().unwrap_or_else(|poisoned| poisoned.into_inner()),
			MEMBER_COUNT
		);
		Ok(())
	}

	#[tokio::test]
	async fn extraction_revalidates_the_archive_identity_before_writing() -> TestResult {
		let temp = TempDir::new()?;
		let path = temp.path().join("identity.zip");
		write_zip(&path, &[("Data/file.txt", b"original")])?;
		let adapter = ArchiveAdapter;
		let indexed = adapter
			.index_port()
			.call((archive_path(&path)?, CancellationToken::new()))
			.await?;
		let IndexedInstaller::Plain { candidates, .. } = indexed.installer else {
			return Err("fixture was not plain".into());
		};
		write_zip(&path, &[("Data/file.txt", b"changed")])?;

		let began_file = Arc::new(Mutex::new(false));
		let begin = Arc::new({
			let began_file = Arc::clone(&began_file);
			move |_, _| {
				*began_file.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = true;
				let write_chunk = Arc::new(move |_, _| Box::pin(async { Ok(()) }) as PortFuture<_>);
				let finish = Arc::new(move |_| Box::pin(async { Ok(()) }) as PortFuture<_>);
				Box::pin(async move { Ok(InstallationFile { write_chunk, finish }) }) as PortFuture<_>
			}
		});
		let result = adapter
			.extract_port()
			.call((
				archive_path(&path)?,
				indexed.identity,
				candidates,
				begin,
				CancellationToken::new(),
			))
			.await;
		assert!(result.is_err());
		assert!(!*began_file.lock().unwrap_or_else(|poisoned| poisoned.into_inner()));
		Ok(())
	}

	#[tokio::test]
	async fn extraction_rejects_normalization_aliases_before_opening_files() -> TestResult {
		let temp = TempDir::new()?;
		let path = temp.path().join("destination-alias.zip");
		write_zip(&path, &[("Data/file.txt", b"payload")])?;
		let adapter = ArchiveAdapter;
		let indexed = adapter
			.index_port()
			.call((archive_path(&path)?, CancellationToken::new()))
			.await?;
		let IndexedInstaller::Plain { candidates, .. } = indexed.installer else {
			return Err("fixture was not plain".into());
		};
		let mut first = candidates[0].clone();
		first.destination = DataRelativePath::new("copies/ﬃ.txt".to_owned())?;
		let mut second = candidates[0].clone();
		second.candidate_id = first.candidate_id + 1;
		second.destination = DataRelativePath::new("copies/ffi.txt".to_owned())?;
		let opened = Arc::new(Mutex::new(false));
		let begin = Arc::new({
			let opened = Arc::clone(&opened);
			move |_, _| {
				*opened.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = true;
				let write_chunk = Arc::new(move |_, _| Box::pin(async { Ok(()) }) as PortFuture<_>);
				let finish = Arc::new(move |_| Box::pin(async { Ok(()) }) as PortFuture<_>);
				Box::pin(async move { Ok(InstallationFile { write_chunk, finish }) }) as PortFuture<_>
			}
		});

		let Err(error) = adapter
			.extract_port()
			.call((
				archive_path(&path)?,
				indexed.identity,
				vec![first, second],
				begin,
				CancellationToken::new(),
			))
			.await
		else {
			return Err("normalization aliases reached output sinks".into());
		};
		assert_eq!(error.current_context().code(), ErrorCode::UnsafeArchive);
		assert!(!*opened.lock().unwrap_or_else(|poisoned| poisoned.into_inner()));
		Ok(())
	}

	#[tokio::test]
	async fn extraction_rejects_source_destination_fan_out_before_opening_files() -> TestResult {
		let temp = TempDir::new()?;
		let path = temp.path().join("fan-out.zip");
		write_zip(&path, &[("Data/file.txt", b"payload")])?;
		let adapter = ArchiveAdapter;
		let indexed = adapter
			.index_port()
			.call((archive_path(&path)?, CancellationToken::new()))
			.await?;
		let IndexedInstaller::Plain { candidates, .. } = indexed.installer else {
			return Err("fixture was not plain".into());
		};
		let template = candidates[0].clone();
		let mut candidates = Vec::new();
		for index in 0..=MAX_SOURCE_DESTINATION_FAN_OUT {
			let mut candidate = template.clone();
			candidate.candidate_id = index as u64;
			candidate.destination = DataRelativePath::new(format!("copies/{index}.txt"))?;
			candidates.push(candidate);
		}
		let opened = Arc::new(Mutex::new(0_usize));
		let begin = Arc::new({
			let opened = Arc::clone(&opened);
			move |_, _| {
				*opened.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) += 1;
				let write_chunk = Arc::new(move |_, _| Box::pin(async { Ok(()) }) as PortFuture<_>);
				let finish = Arc::new(move |_| Box::pin(async { Ok(()) }) as PortFuture<_>);
				Box::pin(async move { Ok(InstallationFile { write_chunk, finish }) }) as PortFuture<_>
			}
		});

		let Err(error) = adapter
			.extract_port()
			.call((
				archive_path(&path)?,
				indexed.identity,
				candidates,
				begin,
				CancellationToken::new(),
			))
			.await
		else {
			return Err("unbounded fan-out was accepted".into());
		};
		assert_eq!(error.current_context().code(), ErrorCode::UnsafeArchive);
		assert_eq!(*opened.lock().unwrap_or_else(|poisoned| poisoned.into_inner()), 0);
		Ok(())
	}

	#[tokio::test]
	async fn cancellation_keeps_an_unfinished_partial_file() -> TestResult {
		let temp = TempDir::new()?;
		let path = temp.path().join("cancel.zip");
		write_zip(&path, &[("Data/large.bin", &vec![1_u8; COPY_BUFFER_BYTES * 4])])?;
		let adapter = ArchiveAdapter;
		let cancellation = CancellationToken::new();
		let index = adapter
			.index_port()
			.call((archive_path(&path)?, cancellation.clone()))
			.await?;
		let IndexedInstaller::Plain { candidates, .. } = index.installer else {
			return Err("fixture was not plain".into());
		};
		let partial = Arc::new(Mutex::new(Vec::new()));
		let finished = Arc::new(Mutex::new(false));
		let begin = Arc::new({
			let partial = Arc::clone(&partial);
			let finished = Arc::clone(&finished);
			let operation = cancellation.clone();
			move |_, _| {
				let partial = Arc::clone(&partial);
				let operation = operation.clone();
				let write_chunk = Arc::new(move |chunk: Vec<u8>, token: CancellationToken| {
					let partial = Arc::clone(&partial);
					let operation = operation.clone();
					Box::pin(async move {
						if token.is_cancelled() {
							return Err(report!(ErrorMarker::operation_cancelled()));
						}
						partial.lock()
							.unwrap_or_else(|poisoned| poisoned.into_inner())
							.extend(chunk);
						operation.cancel();
						Ok(())
					}) as PortFuture<_>
				});
				let finish = Arc::new({
					let finished = Arc::clone(&finished);
					move |_| {
						*finished.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) =
							true;
						Box::pin(async { Ok(()) }) as PortFuture<_>
					}
				});
				Box::pin(async move { Ok(InstallationFile { write_chunk, finish }) }) as PortFuture<_>
			}
		});
		let Err(error) = adapter
			.extract_port()
			.call((archive_path(&path)?, index.identity, candidates, begin, cancellation))
			.await
		else {
			return Err("cancelled extraction succeeded".into());
		};
		assert_eq!(error.current_context().code(), ErrorCode::OperationCancelled);
		assert!(error.iter_reports().any(|node| {
			node.downcast_current_context::<ErrorMarker>()
				.is_some_and(|marker| marker.code() == ErrorCode::OperationCancelled)
		}));
		assert!(!partial
			.lock()
			.unwrap_or_else(|poisoned| poisoned.into_inner())
			.is_empty());
		assert!(!*finished.lock().unwrap_or_else(|poisoned| poisoned.into_inner()));
		Ok(())
	}
}
