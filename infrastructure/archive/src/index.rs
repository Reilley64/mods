use crate::error::ArchiveError;
use crate::limits::COPY_BUFFER_BYTES;
use crate::limits::MAX_ARCHIVE_BYTES;
use crate::limits::MAX_ARCHIVE_MEMBERS;
use crate::limits::MAX_ARCHIVE_WORK;
use crate::limits::MAX_COMPRESSION_RATIO;
use crate::limits::MAX_MEMBER_BYTES;
use crate::limits::MAX_TOTAL_UNCOMPRESSED_BYTES;
use crate::path::SafeArchivePath;
use crate::path::validate_unique_paths;
use crate::rar::index as index_rar;
use crate::seven_zip::index as index_seven_zip;
use crate::source::SourceFile;
use crate::zip::index as index_zip;
use domain::case_fold_key;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use rootcause::report;
use sha2::Digest;
use sha2::Sha256;
use std::collections::BTreeSet;
use std::io::Read;
use std::path::Path;
use std::time::Instant;
use tokio_util::sync::CancellationToken;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ArchiveFormat {
	Zip,
	SevenZip,
	Rar,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MemberKind {
	File,
	Directory,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ArchiveMember {
	pub(crate) ordinal: usize,
	pub(crate) path: SafeArchivePath,
	pub(crate) kind: MemberKind,
	pub(crate) uncompressed_size: u64,
	pub(crate) compressed_size: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CompressionRatioValidation {
	MemberCompressedSize,
	ValidatedArchiveBlocks,
}

pub(crate) struct ArchiveMemberCollector {
	members: Vec<ArchiveMember>,
	total_uncompressed: u64,
	compression_ratio_validation: CompressionRatioValidation,
}

impl ArchiveMemberCollector {
	pub(crate) fn with_declared_count(member_count: usize) -> Result<Self, ArchiveError> {
		Self::new(member_count, CompressionRatioValidation::MemberCompressedSize)
	}

	pub(crate) fn with_validated_archive_blocks(member_count: usize) -> Result<Self, ArchiveError> {
		Self::new(member_count, CompressionRatioValidation::ValidatedArchiveBlocks)
	}

	fn new(
		member_count: usize,
		compression_ratio_validation: CompressionRatioValidation,
	) -> Result<Self, ArchiveError> {
		if member_count > MAX_ARCHIVE_MEMBERS {
			return Err(report!(ArchiveError::ExpansionLimit));
		}
		Ok(Self {
			members: Vec::with_capacity(member_count),
			total_uncompressed: 0,
			compression_ratio_validation,
		})
	}

	pub(crate) fn push(
		&mut self,
		member: ArchiveMember,
		cancellation: &CancellationToken,
		started: Instant,
	) -> Result<(), ArchiveError> {
		if cancellation.is_cancelled() {
			return Err(report!(ArchiveError::Cancelled));
		}
		if started.elapsed() > MAX_ARCHIVE_WORK {
			return Err(report!(ArchiveError::WorkLimit));
		}
		self.total_uncompressed = validate_next_member(
			self.members.len(),
			self.total_uncompressed,
			&member,
			self.compression_ratio_validation,
		)?;
		self.members.push(member);
		Ok(())
	}

	pub(crate) fn into_members(self) -> Vec<ArchiveMember> {
		self.members
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct InstallerDiscovery {
	pub(crate) package_root: Vec<String>,
	pub(crate) data_root: Vec<String>,
	pub(crate) configuration_ordinal: Option<usize>,
	pub(crate) ignored_script_alias: bool,
}

#[derive(Debug)]
pub(crate) struct ArchiveIndexCore {
	pub(crate) source: SourceFile,
	pub(crate) format: ArchiveFormat,
	pub(crate) sha256: [u8; 32],
	pub(crate) members: Vec<ArchiveMember>,
	pub(crate) discovery: InstallerDiscovery,
}

pub(crate) fn index_archive(source: &Path, cancellation: &CancellationToken) -> Result<ArchiveIndexCore, ArchiveError> {
	let started = Instant::now();
	if cancellation.is_cancelled() {
		return Err(report!(ArchiveError::Cancelled));
	}
	if source
		.extension()
		.and_then(|extension| extension.to_str())
		.is_some_and(|extension| extension.eq_ignore_ascii_case("omod"))
	{
		return Err(report!(ArchiveError::UnsupportedInstaller));
	}
	if source
		.extension()
		.and_then(|extension| extension.to_str())
		.is_some_and(|extension| extension.chars().all(|character| character.is_ascii_digit()))
	{
		return Err(report!(ArchiveError::SplitArchive));
	}

	let source_file = SourceFile::open(source)?;
	let (sha256, prefix) = hash_and_prefix(&source_file, cancellation, started)?;
	let format = sniff_format(&prefix)?;
	let members = match format {
		ArchiveFormat::Zip => index_zip(source_file.duplicate()?, cancellation, started)?,
		ArchiveFormat::SevenZip => index_seven_zip(source_file.duplicate()?, cancellation, started)?,
		ArchiveFormat::Rar => index_rar(&source_file, sha256, cancellation, started)?,
	};
	let compression_ratio_validation = if format == ArchiveFormat::SevenZip {
		CompressionRatioValidation::ValidatedArchiveBlocks
	} else {
		CompressionRatioValidation::MemberCompressedSize
	};
	validate_members(&members, compression_ratio_validation)?;
	let discovery = discover_installer(&members)?;

	if cancellation.is_cancelled() {
		return Err(report!(ArchiveError::Cancelled));
	}
	let (verified_sha256, verified_prefix) = hash_and_prefix(&source_file, cancellation, started)?;
	if verified_sha256 != sha256 || sniff_format(&verified_prefix)? != format {
		return Err(report!(ArchiveError::IdentityChanged));
	}

	Ok(ArchiveIndexCore {
		source: source_file,
		format,
		sha256,
		members,
		discovery,
	})
}

pub(crate) fn verify_identity(
	index: &ArchiveIndexCore,
	cancellation: &CancellationToken,
	started: Instant,
) -> Result<(), ArchiveError> {
	let (sha256, prefix) = hash_and_prefix(&index.source, cancellation, started)?;
	if sha256 != index.sha256 || sniff_format(&prefix)? != index.format {
		return Err(report!(ArchiveError::IdentityChanged));
	}
	Ok(())
}

fn hash_and_prefix(
	source: &SourceFile,
	cancellation: &CancellationToken,
	started: Instant,
) -> Result<([u8; 32], Vec<u8>), ArchiveError> {
	let mut file = source.duplicate()?;
	let mut hasher = Sha256::new();
	let mut buffer = vec![0; COPY_BUFFER_BYTES];
	let mut prefix = Vec::with_capacity(8);
	let mut total = 0_u64;
	loop {
		if cancellation.is_cancelled() {
			return Err(report!(ArchiveError::Cancelled));
		}
		if started.elapsed() > MAX_ARCHIVE_WORK {
			return Err(report!(ArchiveError::WorkLimit));
		}
		let count = file.read(&mut buffer).context(ArchiveError::Io)?;
		if count == 0 {
			break;
		}
		total = total
			.checked_add(count as u64)
			.ok_or_else(|| report!(ArchiveError::ExpansionLimit))?;
		if total > MAX_ARCHIVE_BYTES {
			return Err(report!(ArchiveError::ExpansionLimit));
		}
		if prefix.len() < 8 {
			let wanted = (8 - prefix.len()).min(count);
			prefix.extend_from_slice(&buffer[..wanted]);
		}
		hasher.update(&buffer[..count]);
	}
	Ok((hasher.finalize().into(), prefix))
}

fn sniff_format(prefix: &[u8]) -> Result<ArchiveFormat, ArchiveError> {
	if prefix.starts_with(b"PK\x03\x04") || prefix.starts_with(b"PK\x05\x06") {
		return Ok(ArchiveFormat::Zip);
	}
	if prefix.starts_with(b"7z\xBC\xAF\x27\x1C") {
		return Ok(ArchiveFormat::SevenZip);
	}
	if prefix.starts_with(b"Rar!\x1A\x07\x00") || prefix.starts_with(b"Rar!\x1A\x07\x01\x00") {
		return Ok(ArchiveFormat::Rar);
	}
	Err(report!(ArchiveError::UnsupportedFormat))
}

fn validate_members(
	members: &[ArchiveMember],
	compression_ratio_validation: CompressionRatioValidation,
) -> Result<(), ArchiveError> {
	let mut total = 0_u64;
	for (expected_ordinal, member) in members.iter().enumerate() {
		total = validate_next_member(expected_ordinal, total, member, compression_ratio_validation)?;
	}
	validate_unique_paths(members.iter().map(|member| &member.path))
}

fn validate_next_member(
	expected_ordinal: usize,
	total_uncompressed: u64,
	member: &ArchiveMember,
	compression_ratio_validation: CompressionRatioValidation,
) -> Result<u64, ArchiveError> {
	if expected_ordinal >= MAX_ARCHIVE_MEMBERS
		|| member.ordinal != expected_ordinal
		|| member.uncompressed_size > MAX_MEMBER_BYTES
	{
		return Err(report!(ArchiveError::ExpansionLimit));
	}
	let total_uncompressed = total_uncompressed
		.checked_add(member.uncompressed_size)
		.ok_or_else(|| report!(ArchiveError::ExpansionLimit))?;
	if total_uncompressed > MAX_TOTAL_UNCOMPRESSED_BYTES {
		return Err(report!(ArchiveError::ExpansionLimit));
	}
	if compression_ratio_validation == CompressionRatioValidation::MemberCompressedSize
		&& member.uncompressed_size > 0
		&& (member.compressed_size == 0
			|| member.uncompressed_size
				> member.compressed_size
					.checked_mul(MAX_COMPRESSION_RATIO)
					.ok_or_else(|| report!(ArchiveError::ExpansionLimit))?)
	{
		return Err(report!(ArchiveError::ExpansionLimit));
	}
	Ok(total_uncompressed)
}

fn discover_installer(members: &[ArchiveMember]) -> Result<InstallerDiscovery, ArchiveError> {
	let files: Vec<&ArchiveMember> = members
		.iter()
		.filter(|member| member.kind == MemberKind::File)
		.collect();
	if files.is_empty() {
		return Err(report!(ArchiveError::InvalidArchive));
	}

	for member in &files {
		let components = member.path.components();
		if components.len() >= 2
			&& case_fold_key(&components[components.len() - 2]) == "fomod"
			&& case_fold_key(&components[components.len() - 1]) == "script.cs"
		{
			return Err(report!(ArchiveError::UnsupportedInstaller));
		}
	}

	let mut configurations = Vec::new();
	for member in &files {
		let components = member.path.components();
		if components.len() < 2 || case_fold_key(&components[components.len() - 2]) != "fomod" {
			continue;
		}
		let filename_key = case_fold_key(&components[components.len() - 1]);
		if filename_key == "moduleconfig.xml" || filename_key == "script.xml" {
			configurations.push((
				member,
				components[..components.len() - 2].to_vec(),
				filename_key == "script.xml",
			));
		}
	}

	if !configurations.is_empty() {
		let roots: BTreeSet<String> = configurations
			.iter()
			.map(|(_, root, _)| {
				root.iter()
					.map(|part| case_fold_key(part))
					.collect::<Vec<_>>()
					.join("/")
			})
			.collect();
		if roots.len() != 1 {
			return Err(report!(ArchiveError::UnsupportedInstaller));
		}
		let package_root = configurations[0].1.clone();
		if files.iter()
			.any(|member| !member.path.starts_with_components(&package_root))
		{
			return Err(report!(ArchiveError::UnsupportedInstaller));
		}
		let primary: Vec<_> = configurations.iter().filter(|(_, _, alias)| !alias).collect();
		let aliases: Vec<_> = configurations.iter().filter(|(_, _, alias)| *alias).collect();
		if primary.len() > 1 || aliases.len() > 1 {
			return Err(report!(ArchiveError::DuplicatePath));
		}
		let selected = primary
			.first()
			.or_else(|| aliases.first())
			.ok_or_else(|| report!(ArchiveError::InvalidArchive))?;
		let data_root = find_data_root(&files, &package_root)?;
		return Ok(InstallerDiscovery {
			package_root,
			data_root,
			configuration_ordinal: Some(selected.0.ordinal),
			ignored_script_alias: !primary.is_empty() && !aliases.is_empty(),
		});
	}

	let package_root = common_wrapper_prefix(&files);
	let data_root = find_data_root(&files, &package_root)?;
	Ok(InstallerDiscovery {
		package_root,
		data_root,
		configuration_ordinal: None,
		ignored_script_alias: false,
	})
}

fn common_wrapper_prefix(files: &[&ArchiveMember]) -> Vec<String> {
	let first = files[0].path.components();
	let mut prefix = Vec::new();
	for (index, component) in first.iter().enumerate().take(first.len().saturating_sub(1)) {
		if is_data_content_root(component)
			|| files.iter().any(|member| {
				member.path.components().len() <= index + 1
					|| case_fold_key(&member.path.components()[index]) != case_fold_key(component)
			}) {
			break;
		}
		prefix.push(component.clone());
	}
	prefix
}

fn is_data_content_root(component: &str) -> bool {
	matches!(
		case_fold_key(component).as_str(),
		"data" | "meshes"
			| "textures" | "sound"
			| "music" | "menus" | "interface"
			| "scripts" | "nvse" | "video"
	)
}

fn find_data_root(files: &[&ArchiveMember], package_root: &[String]) -> Result<Vec<String>, ArchiveError> {
	let mut data_root: Option<Vec<String>> = None;
	for member in files {
		let components = member.path.components();
		if components.len() <= package_root.len()
			|| !member.path.starts_with_components(package_root)
			|| case_fold_key(&components[package_root.len()]) != "data"
		{
			continue;
		}
		let candidate = components[..=package_root.len()].to_vec();
		if data_root.as_ref().is_some_and(|current| {
			current.len() != candidate.len()
				|| current
					.iter()
					.zip(&candidate)
					.any(|(left, right)| case_fold_key(left) != case_fold_key(right))
		}) {
			return Err(report!(ArchiveError::UnsupportedInstaller));
		}
		data_root.get_or_insert(candidate);
	}
	Ok(data_root.unwrap_or_else(|| package_root.to_vec()))
}

#[cfg(test)]
mod tests {
	use super::ArchiveMember;
	use super::ArchiveMemberCollector;
	use super::CompressionRatioValidation;
	use super::MemberKind;
	use super::discover_installer;
	use super::validate_members;
	use super::validate_next_member;
	use crate::error::ArchiveError;
	use crate::limits::MAX_ARCHIVE_MEMBERS;
	use crate::limits::MAX_COMPRESSION_RATIO;
	use crate::limits::MAX_MEMBER_BYTES;
	use crate::path::SafeArchivePath;
	use rootcause::Result;
	use rootcause::report;
	use std::time::Instant;
	use tokio_util::sync::CancellationToken;

	fn member(ordinal: usize, path: &str) -> Result<ArchiveMember, ArchiveError> {
		Ok(ArchiveMember {
			ordinal,
			path: SafeArchivePath::new(path)?,
			kind: MemberKind::File,
			uncompressed_size: 1,
			compressed_size: 1,
		})
	}

	#[test]
	fn discovers_wrapped_fomod_and_data_roots() -> Result<(), ArchiveError> {
		let discovery = discover_installer(&[
			member(0, "Wrapper/fomod/ModuleConfig.xml")?,
			member(1, "Wrapper/Data/meshes/a.nif")?,
		])?;
		assert_eq!(discovery.package_root, ["Wrapper"]);
		assert_eq!(discovery.data_root, ["Wrapper", "Data"]);
		assert_eq!(discovery.configuration_ordinal, Some(0));
		Ok(())
	}

	#[test]
	fn common_wrapper_roots_use_simple_unicode_case_folding() -> Result<(), ArchiveError> {
		let discovery = discover_installer(&[member(0, "ÉΣ/meshes/a.nif")?, member(1, "éς/textures/b.dds")?])?;

		assert_eq!(discovery.package_root, ["ÉΣ"]);
		Ok(())
	}

	#[test]
	fn unicode_case_folded_content_root_is_not_stripped_as_a_wrapper() -> Result<(), ArchiveError> {
		let discovery = discover_installer(&[member(0, "ſcripts/foo.pex")?])?;

		assert!(discovery.package_root.is_empty());
		assert!(discovery.data_root.is_empty());
		Ok(())
	}

	#[test]
	fn fomod_roots_use_simple_unicode_case_folded_keys() -> Result<(), ArchiveError> {
		let discovery = discover_installer(&[
			member(0, "ÉΣ/fomod/ModuleConfig.xml")?,
			member(1, "éς/fomod/script.xml")?,
			member(2, "ÉΣ/Data/meshes/a.nif")?,
			member(3, "éς/data/textures/b.dds")?,
		])?;

		assert_eq!(discovery.package_root, ["ÉΣ"]);
		assert_eq!(discovery.data_root, ["ÉΣ", "Data"]);
		assert!(discovery.ignored_script_alias);
		Ok(())
	}

	#[test]
	fn unicode_case_folded_script_alias_is_discovered() -> Result<(), ArchiveError> {
		let discovery =
			discover_installer(&[member(0, "fomod/ModuleConfig.xml")?, member(1, "fomod/ſcript.xml")?])?;

		assert_eq!(discovery.configuration_ordinal, Some(0));
		assert!(discovery.ignored_script_alias);
		Ok(())
	}

	#[test]
	fn rejects_executable_installer() -> Result<(), ArchiveError> {
		assert!(discover_installer(&[member(0, "fomod/script.cs")?]).is_err());
		Ok(())
	}

	#[test]
	fn rejects_unicode_case_folded_executable_installer() -> Result<(), ArchiveError> {
		let Err(error) = discover_installer(&[member(0, "fomod/ſcript.cs")?]) else {
			return Err(report!(ArchiveError::InvalidArchive));
		};

		assert_eq!(error.current_context(), &ArchiveError::UnsupportedInstaller);
		Ok(())
	}

	#[test]
	fn enforces_member_and_compression_limits() -> Result<(), ArchiveError> {
		let mut oversized = member(0, "Data/large.bin")?;
		oversized.uncompressed_size = MAX_MEMBER_BYTES + 1;
		assert!(validate_members(&[oversized], CompressionRatioValidation::MemberCompressedSize).is_err());

		let mut compressed = member(0, "Data/compressed.bin")?;
		compressed.uncompressed_size = MAX_COMPRESSION_RATIO + 1;
		compressed.compressed_size = 1;
		assert!(validate_members(&[compressed], CompressionRatioValidation::MemberCompressedSize).is_err());
		Ok(())
	}
	#[test]
	fn archive_block_ratio_validation_does_not_use_member_compressed_sizes() -> Result<(), ArchiveError> {
		let mut later_solid_member = member(0, "Data/later.bin")?;
		later_solid_member.compressed_size = 0;
		validate_members(
			&[later_solid_member],
			CompressionRatioValidation::ValidatedArchiveBlocks,
		)?;

		let mut oversized = member(0, "Data/large.bin")?;
		oversized.uncompressed_size = MAX_MEMBER_BYTES + 1;
		let validation = validate_members(&[oversized], CompressionRatioValidation::ValidatedArchiveBlocks);
		assert!(validation.is_err());
		Ok(())
	}

	#[test]
	fn rejects_declared_and_iterated_member_counts_without_large_allocations() -> Result<(), ArchiveError> {
		let Err(declared) = ArchiveMemberCollector::with_declared_count(MAX_ARCHIVE_MEMBERS + 1) else {
			return Err(report!(ArchiveError::InvalidArchive));
		};
		assert_eq!(declared.current_context(), &ArchiveError::ExpansionLimit);

		let mut over_count = member(MAX_ARCHIVE_MEMBERS, "Data/extra.bin")?;
		over_count.ordinal = MAX_ARCHIVE_MEMBERS;
		let Err(iterated) = validate_next_member(
			MAX_ARCHIVE_MEMBERS,
			0,
			&over_count,
			CompressionRatioValidation::MemberCompressedSize,
		) else {
			return Err(report!(ArchiveError::InvalidArchive));
		};
		assert_eq!(iterated.current_context(), &ArchiveError::ExpansionLimit);
		Ok(())
	}

	#[test]
	fn rejects_aggregate_size_while_members_are_collected() -> Result<(), ArchiveError> {
		let cancellation = CancellationToken::new();
		let started = Instant::now();
		let mut collector = ArchiveMemberCollector::with_declared_count(5)?;
		for ordinal in 0..5 {
			let mut archive_member = member(ordinal, &format!("Data/{ordinal}.bin"))?;
			archive_member.uncompressed_size = MAX_MEMBER_BYTES;
			archive_member.compressed_size = MAX_MEMBER_BYTES;
			let result = collector.push(archive_member, &cancellation, started);
			if ordinal < 4 {
				result?;
			} else if let Err(error) = result {
				assert_eq!(error.current_context(), &ArchiveError::ExpansionLimit);
			} else {
				return Err(report!(ArchiveError::InvalidArchive));
			}
		}
		Ok(())
	}
}
