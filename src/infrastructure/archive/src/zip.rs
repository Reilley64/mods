use crate::entry::validate_entry_kind;
use crate::error::ArchiveError;
use crate::index::ArchiveMember;
use crate::index::ArchiveMemberCollector;
use crate::index::MemberKind;
use crate::limits::MAX_ARCHIVE_MEMBERS;
use crate::limits::MAX_ARCHIVE_METADATA_BYTES;
use crate::limits::MAX_ARCHIVE_WORK;
use crate::path::SafeArchivePath;
use rootcause::Report;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use rootcause::report;
use std::fs::File;
use std::io::Read;
use std::io::Seek;
use std::io::SeekFrom;
use std::time::Instant;
use tokio_util::sync::CancellationToken;
use zip::CompressionMethod;
use zip::ZipArchive;
use zip::read::HasZipMetadata;
use zip::read::ZipFile;
use zip::result::ZipError;
use zip::result::ZipResult;

pub(crate) fn index(
	mut source: File,
	cancellation: &CancellationToken,
	started: Instant,
) -> Result<Vec<ArchiveMember>, ArchiveError> {
	preflight_metadata(&mut source, cancellation, started)?;
	let mut archive = ZipArchive::new(source).map_err(map_error)?;
	let mut members = ArchiveMemberCollector::with_declared_count(archive.len())?;
	for ordinal in 0..archive.len() {
		if cancellation.is_cancelled() {
			return Err(report!(ArchiveError::Cancelled));
		}
		if started.elapsed() > MAX_ARCHIVE_WORK {
			return Err(report!(ArchiveError::WorkLimit));
		}
		let entry = archive.by_index_raw(ordinal).map_err(map_error)?;
		if let Some(error) = unsupported_entry_error(&entry) {
			return Err(report!(error));
		}
		let name = entry.name();
		if name.contains('\u{fffd}') {
			return Err(report!(ArchiveError::NonLosslessName));
		}
		let is_directory = entry.is_dir();
		let normalized_name = if is_directory {
			name.trim_end_matches(['/', '\\'])
		} else {
			name
		};
		let path = SafeArchivePath::new(normalized_name)?;

		validate_entry_kind(
			is_directory,
			Some(entry.get_metadata().external_attributes.into()),
			entry.unix_mode().map(u64::from),
		)?;

		members.push(
			ArchiveMember {
				ordinal,
				path,
				kind: if is_directory {
					MemberKind::Directory
				} else {
					MemberKind::File
				},
				uncompressed_size: entry.size(),
				compressed_size: entry.compressed_size(),
			},
			cancellation,
			started,
		)?;
	}
	Ok(members.into_members())
}

fn unsupported_entry_error<R: Read + ?Sized>(entry: &ZipFile<'_, R>) -> Option<ArchiveError> {
	let compression = entry.compression();
	if entry.encrypted() || compression == CompressionMethod::AES {
		return Some(ArchiveError::Encrypted);
	}
	match compression {
		CompressionMethod::Stored
		| CompressionMethod::Deflated
		| CompressionMethod::Deflate64
		| CompressionMethod::Bzip2 => None,
		_ => Some(ArchiveError::UnsupportedFormat),
	}
}

// zip allocates from declared entry counts before callers can enforce project limits, so validate the
// bounded end records without constructing its general-purpose parser.
fn preflight_metadata(file: &mut File, cancellation: &CancellationToken, started: Instant) -> Result<(), ArchiveError> {
	const END_OF_CENTRAL_DIRECTORY_BYTES: u64 = 22;
	const MAX_COMMENT_BYTES: u64 = u16::MAX as u64;
	const ZIP64_LOCATOR_BYTES: u64 = 20;
	const ZIP64_END_MINIMUM_BYTES: u64 = 56;
	const CENTRAL_DIRECTORY_HEADER_BYTES: u64 = 46;

	if cancellation.is_cancelled() {
		return Err(report!(ArchiveError::Cancelled));
	}
	if started.elapsed() > MAX_ARCHIVE_WORK {
		return Err(report!(ArchiveError::WorkLimit));
	}
	let length = file.metadata().context(ArchiveError::Io)?.len();
	if length < END_OF_CENTRAL_DIRECTORY_BYTES {
		return Err(report!(ArchiveError::InvalidArchive));
	}

	let tail_length = length.min(END_OF_CENTRAL_DIRECTORY_BYTES + MAX_COMMENT_BYTES);
	let tail_start = length
		.checked_sub(tail_length)
		.ok_or_else(|| report!(ArchiveError::InvalidArchive))?;
	if cancellation.is_cancelled() {
		return Err(report!(ArchiveError::Cancelled));
	}
	if started.elapsed() > MAX_ARCHIVE_WORK {
		return Err(report!(ArchiveError::WorkLimit));
	}
	file.seek(SeekFrom::Start(tail_start)).context(ArchiveError::Io)?;
	let tail_length = usize::try_from(tail_length).context(ArchiveError::InvalidArchive)?;
	let mut tail = vec![0; tail_length];
	if cancellation.is_cancelled() {
		return Err(report!(ArchiveError::Cancelled));
	}
	if started.elapsed() > MAX_ARCHIVE_WORK {
		return Err(report!(ArchiveError::WorkLimit));
	}
	file.read_exact(&mut tail).context(ArchiveError::Io)?;

	let mut footer_position = None;
	for position in (0..=tail.len() - END_OF_CENTRAL_DIRECTORY_BYTES as usize).rev() {
		if cancellation.is_cancelled() {
			return Err(report!(ArchiveError::Cancelled));
		}
		if started.elapsed() > MAX_ARCHIVE_WORK {
			return Err(report!(ArchiveError::WorkLimit));
		}
		if tail[position..position + 4] != *b"PK\x05\x06" {
			continue;
		}
		let comment_length = usize::from(u16::from_le_bytes([tail[position + 20], tail[position + 21]]));
		if position
			.checked_add(END_OF_CENTRAL_DIRECTORY_BYTES as usize)
			.and_then(|end| end.checked_add(comment_length))
			== Some(tail.len())
		{
			footer_position = Some(position);
			break;
		}
	}
	let footer_position = footer_position.ok_or_else(|| report!(ArchiveError::InvalidArchive))?;
	let footer_offset = tail_start
		.checked_add(u64::try_from(footer_position).context(ArchiveError::InvalidArchive)?)
		.ok_or_else(|| report!(ArchiveError::InvalidArchive))?;
	let footer = &tail[footer_position..footer_position + END_OF_CENTRAL_DIRECTORY_BYTES as usize];
	let disk = u16::from_le_bytes([footer[4], footer[5]]);
	let central_disk = u16::from_le_bytes([footer[6], footer[7]]);
	let classic_entries_on_disk = u16::from_le_bytes([footer[8], footer[9]]);
	let classic_total_entries = u16::from_le_bytes([footer[10], footer[11]]);
	let classic_central_size = u32::from_le_bytes([footer[12], footer[13], footer[14], footer[15]]);
	let classic_central_offset = u32::from_le_bytes([footer[16], footer[17], footer[18], footer[19]]);
	if disk != 0 || central_disk != 0 {
		return Err(report!(ArchiveError::SplitArchive));
	}

	let needs_zip64 = classic_entries_on_disk == u16::MAX
		|| classic_total_entries == u16::MAX
		|| classic_central_size == u32::MAX
		|| classic_central_offset == u32::MAX;
	let locator_offset = footer_offset.checked_sub(ZIP64_LOCATOR_BYTES);
	let mut locator = [0_u8; ZIP64_LOCATOR_BYTES as usize];
	if let Some(locator_offset) = locator_offset {
		if cancellation.is_cancelled() {
			return Err(report!(ArchiveError::Cancelled));
		}
		if started.elapsed() > MAX_ARCHIVE_WORK {
			return Err(report!(ArchiveError::WorkLimit));
		}
		file.seek(SeekFrom::Start(locator_offset)).context(ArchiveError::Io)?;
		file.read_exact(&mut locator).context(ArchiveError::Io)?;
		if cancellation.is_cancelled() {
			return Err(report!(ArchiveError::Cancelled));
		}
		if started.elapsed() > MAX_ARCHIVE_WORK {
			return Err(report!(ArchiveError::WorkLimit));
		}
	}
	let has_zip64_locator = locator[..4] == *b"PK\x06\x07";

	let (entries_on_disk, total_entries, central_size, central_offset, metadata_boundary) = if needs_zip64
		|| has_zip64_locator
	{
		let Some(locator_offset) = locator_offset else {
			return Err(report!(ArchiveError::InvalidArchive));
		};
		if !has_zip64_locator {
			return Err(report!(ArchiveError::InvalidArchive));
		}
		let zip64_disk = u32::from_le_bytes([locator[4], locator[5], locator[6], locator[7]]);
		let zip64_offset = u64::from_le_bytes([
			locator[8],
			locator[9],
			locator[10],
			locator[11],
			locator[12],
			locator[13],
			locator[14],
			locator[15],
		]);
		let total_disks = u32::from_le_bytes([locator[16], locator[17], locator[18], locator[19]]);
		if zip64_disk != 0 || total_disks != 1 {
			return Err(report!(ArchiveError::SplitArchive));
		}
		let minimum_end = zip64_offset
			.checked_add(ZIP64_END_MINIMUM_BYTES)
			.ok_or_else(|| report!(ArchiveError::InvalidArchive))?;
		if minimum_end > locator_offset || minimum_end > length {
			return Err(report!(ArchiveError::InvalidArchive));
		}

		if cancellation.is_cancelled() {
			return Err(report!(ArchiveError::Cancelled));
		}
		if started.elapsed() > MAX_ARCHIVE_WORK {
			return Err(report!(ArchiveError::WorkLimit));
		}
		file.seek(SeekFrom::Start(zip64_offset)).context(ArchiveError::Io)?;
		let mut zip64 = [0_u8; ZIP64_END_MINIMUM_BYTES as usize];
		file.read_exact(&mut zip64).context(ArchiveError::Io)?;
		if cancellation.is_cancelled() {
			return Err(report!(ArchiveError::Cancelled));
		}
		if started.elapsed() > MAX_ARCHIVE_WORK {
			return Err(report!(ArchiveError::WorkLimit));
		}
		if zip64[..4] != *b"PK\x06\x06" {
			return Err(report!(ArchiveError::InvalidArchive));
		}
		let record_size = u64::from_le_bytes(
			zip64[4..12]
				.try_into()
				.map_err(|_| report!(ArchiveError::InvalidArchive))?,
		);
		if record_size > MAX_ARCHIVE_METADATA_BYTES {
			return Err(report!(ArchiveError::ExpansionLimit));
		}
		if record_size < ZIP64_END_MINIMUM_BYTES - 12
			|| zip64_offset
				.checked_add(12)
				.and_then(|offset| offset.checked_add(record_size))
				!= Some(locator_offset)
		{
			return Err(report!(ArchiveError::InvalidArchive));
		}
		let zip64_disk = u32::from_le_bytes(
			zip64[16..20]
				.try_into()
				.map_err(|_| report!(ArchiveError::InvalidArchive))?,
		);
		let zip64_central_disk = u32::from_le_bytes(
			zip64[20..24]
				.try_into()
				.map_err(|_| report!(ArchiveError::InvalidArchive))?,
		);
		let entries_on_disk = u64::from_le_bytes(
			zip64[24..32]
				.try_into()
				.map_err(|_| report!(ArchiveError::InvalidArchive))?,
		);
		let total_entries = u64::from_le_bytes(
			zip64[32..40]
				.try_into()
				.map_err(|_| report!(ArchiveError::InvalidArchive))?,
		);
		let central_size = u64::from_le_bytes(
			zip64[40..48]
				.try_into()
				.map_err(|_| report!(ArchiveError::InvalidArchive))?,
		);
		let central_offset = u64::from_le_bytes(
			zip64[48..56]
				.try_into()
				.map_err(|_| report!(ArchiveError::InvalidArchive))?,
		);
		if zip64_disk != 0 || zip64_central_disk != 0 || entries_on_disk != total_entries {
			return Err(report!(ArchiveError::SplitArchive));
		}
		if classic_entries_on_disk != u16::MAX && u64::from(classic_entries_on_disk) != entries_on_disk {
			return Err(report!(ArchiveError::SplitArchive));
		}
		if classic_total_entries != u16::MAX && u64::from(classic_total_entries) != total_entries {
			return Err(report!(ArchiveError::InvalidArchive));
		}
		if classic_central_size != u32::MAX && u64::from(classic_central_size) != central_size {
			return Err(report!(ArchiveError::InvalidArchive));
		}
		if classic_central_offset != u32::MAX && u64::from(classic_central_offset) != central_offset {
			return Err(report!(ArchiveError::InvalidArchive));
		}
		(
			entries_on_disk,
			total_entries,
			central_size,
			central_offset,
			zip64_offset,
		)
	} else {
		(
			u64::from(classic_entries_on_disk),
			u64::from(classic_total_entries),
			u64::from(classic_central_size),
			u64::from(classic_central_offset),
			footer_offset,
		)
	};

	if entries_on_disk != total_entries {
		return Err(report!(ArchiveError::SplitArchive));
	}
	if total_entries > MAX_ARCHIVE_MEMBERS as u64 || central_size > MAX_ARCHIVE_METADATA_BYTES {
		return Err(report!(ArchiveError::ExpansionLimit));
	}
	let minimum_central_size = total_entries
		.checked_mul(CENTRAL_DIRECTORY_HEADER_BYTES)
		.ok_or_else(|| report!(ArchiveError::InvalidArchive))?;
	let central_end = central_offset
		.checked_add(central_size)
		.ok_or_else(|| report!(ArchiveError::InvalidArchive))?;
	if minimum_central_size > central_size || central_offset > length || central_end != metadata_boundary {
		return Err(report!(ArchiveError::InvalidArchive));
	}

	if cancellation.is_cancelled() {
		return Err(report!(ArchiveError::Cancelled));
	}
	if started.elapsed() > MAX_ARCHIVE_WORK {
		return Err(report!(ArchiveError::WorkLimit));
	}
	file.seek(SeekFrom::Start(0)).context(ArchiveError::Io)?;
	Ok(())
}

pub(crate) struct SupportedZipArchive {
	archive: ZipArchive<File>,
}

impl SupportedZipArchive {
	pub(crate) fn by_index(&mut self, ordinal: usize) -> ZipResult<ZipFile<'_, File>> {
		{
			let entry = self.archive.by_index_raw(ordinal)?;
			if let Some(error) = unsupported_entry_error(&entry) {
				return Err(match error {
					ArchiveError::Encrypted => {
						ZipError::UnsupportedArchive(ZipError::PASSWORD_REQUIRED)
					}
					_ => ZipError::UnsupportedArchive(
						"ZIP compression method is outside the supported safety subset",
					),
				});
			}
		}
		self.archive.by_index(ordinal)
	}
}

pub(crate) fn open(
	mut source: File,
	cancellation: &CancellationToken,
	started: Instant,
) -> Result<SupportedZipArchive, ArchiveError> {
	preflight_metadata(&mut source, cancellation, started)?;
	let archive = ZipArchive::new(source).map_err(map_error)?;
	Ok(SupportedZipArchive { archive })
}

pub(crate) fn map_error(error: ZipError) -> Report<ArchiveError> {
	let detail = error.to_string().to_ascii_lowercase();
	let context = match &error {
		ZipError::InvalidPassword => ArchiveError::Encrypted,
		ZipError::UnsupportedArchive(message) if *message == ZipError::PASSWORD_REQUIRED => {
			ArchiveError::Encrypted
		}
		ZipError::CompressionMethodNotSupported(_) | ZipError::UnsupportedArchive(_) => {
			ArchiveError::UnsupportedFormat
		}
		_ if detail.contains("multi-disk") || detail.contains("multi disk") || detail.contains("split") => {
			ArchiveError::SplitArchive
		}
		_ if detail.contains("password") || detail.contains("encrypt") => ArchiveError::Encrypted,
		_ => ArchiveError::InvalidArchive,
	};
	report!(error).context(context)
}

#[cfg(test)]
mod tests {
	use super::index;
	use super::map_error;
	use super::open;
	use crate::error::ArchiveError;
	use crate::limits::MAX_ARCHIVE_MEMBERS;
	use crate::limits::MAX_ARCHIVE_METADATA_BYTES;
	use std::error::Error;
	use std::fs::File;
	use std::io::Error as IoError;
	use std::io::Read;
	use std::io::Seek;
	use std::io::SeekFrom;
	use std::io::Write;
	use std::time::Instant;
	use tempfile::tempfile;
	use tokio_util::sync::CancellationToken;
	use zip::CompressionMethod;
	use zip::ZipWriter;
	use zip::write::SimpleFileOptions;

	type TestResult = std::result::Result<(), Box<dyn Error>>;

	#[test]
	fn normal_zip_is_indexed_and_reopened_for_extraction() -> TestResult {
		let cancellation = CancellationToken::new();
		let Ok(members) = index(archive_with_entry()?, &cancellation, Instant::now()) else {
			return Err(IoError::other("normal ZIP fixture was rejected").into());
		};
		assert_eq!(members.len(), 1);
		assert_eq!(members[0].path.as_str(), "Data/file.bin");

		let Ok(mut archive) = open(archive_with_entry()?, &cancellation, Instant::now()) else {
			return Err(IoError::other("normal ZIP fixture could not be reopened").into());
		};
		let mut entry = archive.by_index(0)?;
		let mut contents = Vec::new();
		entry.read_to_end(&mut contents)?;
		assert_eq!(contents, b"safe payload");
		Ok(())
	}

	#[test]
	fn zip64_metadata_is_accepted_before_parser_construction() -> TestResult {
		let bytes = zip64_archive_bytes()?;
		let Ok(members) = index(file_from_bytes(&bytes)?, &CancellationToken::new(), Instant::now()) else {
			return Err(IoError::other("valid ZIP64 fixture was rejected").into());
		};
		assert_eq!(members.len(), 1);
		assert_eq!(members[0].path.as_str(), "Data/file.bin");
		Ok(())
	}

	#[test]
	fn zip64_oversized_count_is_rejected_before_parser_construction() -> TestResult {
		let mut bytes = zip64_archive_bytes()?;
		let zip64_end = bytes
			.windows(4)
			.position(|window| window == b"PK\x06\x06")
			.ok_or_else(|| IoError::other("ZIP64 fixture end record is missing"))?;
		let count = u64::try_from(MAX_ARCHIVE_MEMBERS + 1)?;
		bytes[zip64_end + 24..zip64_end + 32].copy_from_slice(&count.to_le_bytes());
		bytes[zip64_end + 32..zip64_end + 40].copy_from_slice(&count.to_le_bytes());

		let Err(error) = index(file_from_bytes(&bytes)?, &CancellationToken::new(), Instant::now()) else {
			return Err(IoError::other("oversized ZIP64 count reached the ZIP parser").into());
		};
		assert_eq!(error.current_context(), &ArchiveError::ExpansionLimit);
		Ok(())
	}

	#[test]
	fn oversized_central_directory_is_rejected_before_parser_construction() -> TestResult {
		let mut bytes = archive_bytes(archive_with_entry()?)?;
		let footer = end_of_central_directory(&bytes)?;
		let size = u32::try_from(MAX_ARCHIVE_METADATA_BYTES + 1)?;
		bytes[footer + 12..footer + 16].copy_from_slice(&size.to_le_bytes());

		let Err(error) = index(file_from_bytes(&bytes)?, &CancellationToken::new(), Instant::now()) else {
			return Err(IoError::other("oversized central directory reached the ZIP parser").into());
		};
		assert_eq!(error.current_context(), &ArchiveError::ExpansionLimit);
		Ok(())
	}

	#[test]
	fn oversized_zip64_end_record_is_rejected_before_parser_construction() -> TestResult {
		let mut bytes = zip64_archive_bytes()?;
		let zip64_end = bytes
			.windows(4)
			.position(|window| window == b"PK\x06\x06")
			.ok_or_else(|| IoError::other("ZIP64 fixture end record is missing"))?;
		let size = MAX_ARCHIVE_METADATA_BYTES + 1;
		bytes[zip64_end + 4..zip64_end + 12].copy_from_slice(&size.to_le_bytes());

		let Err(error) = index(file_from_bytes(&bytes)?, &CancellationToken::new(), Instant::now()) else {
			return Err(IoError::other("oversized ZIP64 end record reached the ZIP parser").into());
		};
		assert_eq!(error.current_context(), &ArchiveError::ExpansionLimit);
		Ok(())
	}

	#[test]
	fn overlapping_central_directory_is_rejected_before_parser_construction() -> TestResult {
		let mut bytes = archive_bytes(archive_with_entry()?)?;
		let footer = end_of_central_directory(&bytes)?;
		let size = u32::from_le_bytes(bytes[footer + 12..footer + 16].try_into()?);
		let overlapping_size = size
			.checked_add(1)
			.ok_or_else(|| IoError::other("ZIP fixture central directory size overflowed"))?;
		bytes[footer + 12..footer + 16].copy_from_slice(&overlapping_size.to_le_bytes());

		let Err(error) = index(file_from_bytes(&bytes)?, &CancellationToken::new(), Instant::now()) else {
			return Err(IoError::other("overlapping central directory reached the ZIP parser").into());
		};
		assert_eq!(error.current_context(), &ArchiveError::InvalidArchive);
		Ok(())
	}

	#[test]
	fn multidisk_metadata_is_rejected_before_parser_construction() -> TestResult {
		let mut bytes = archive_bytes(archive_with_entry()?)?;
		let footer = end_of_central_directory(&bytes)?;
		bytes[footer + 4..footer + 6].copy_from_slice(&1_u16.to_le_bytes());

		let Err(error) = index(file_from_bytes(&bytes)?, &CancellationToken::new(), Instant::now()) else {
			return Err(IoError::other("multidisk metadata reached the ZIP parser").into());
		};
		assert_eq!(error.current_context(), &ArchiveError::SplitArchive);
		Ok(())
	}

	#[test]
	fn malformed_zip64_locator_offset_is_rejected_before_parser_construction() -> TestResult {
		let mut bytes = zip64_archive_bytes()?;
		let footer = end_of_central_directory(&bytes)?;
		let locator = footer - 20;
		bytes[locator + 8..locator + 16].copy_from_slice(&u64::MAX.to_le_bytes());

		let Err(error) = index(file_from_bytes(&bytes)?, &CancellationToken::new(), Instant::now()) else {
			return Err(IoError::other("malformed ZIP64 locator reached the ZIP parser").into());
		};
		assert_eq!(error.current_context(), &ArchiveError::InvalidArchive);
		Ok(())
	}

	#[test]
	fn metadata_preflight_honors_cancellation_before_parser_construction() -> TestResult {
		let cancellation = CancellationToken::new();
		cancellation.cancel();
		let Err(error) = index(archive_with_entry()?, &cancellation, Instant::now()) else {
			return Err(IoError::other("cancelled metadata preflight reached the ZIP parser").into());
		};
		assert_eq!(error.current_context(), &ArchiveError::Cancelled);
		Ok(())
	}

	#[test]
	fn unsupported_compression_is_rejected_from_raw_metadata() -> TestResult {
		// The payload is not a valid Zstandard stream. Reject it before decoder construction.
		let file = archive_with_compression(93)?;
		let Err(error) = index(file, &CancellationToken::new(), Instant::now()) else {
			return Err(IoError::other("unsupported ZIP compression was accepted").into());
		};
		assert_eq!(error.current_context(), &ArchiveError::UnsupportedFormat);
		Ok(())
	}

	#[test]
	fn extraction_rechecks_compression_before_constructing_a_decoder() -> TestResult {
		let file = archive_with_compression(93)?;
		let Ok(mut archive) = open(file, &CancellationToken::new(), Instant::now()) else {
			return Err(IoError::other("valid ZIP fixture was rejected").into());
		};
		let Err(error) = archive.by_index(0) else {
			return Err(IoError::other("unsupported ZIP compression reached a decoder").into());
		};
		assert_eq!(map_error(error).current_context(), &ArchiveError::UnsupportedFormat);
		Ok(())
	}

	#[test]
	fn aes_compression_is_reported_as_encrypted() -> TestResult {
		let file = archive_with_compression(99)?;
		let Err(error) = index(file, &CancellationToken::new(), Instant::now()) else {
			return Err(IoError::other("AES ZIP compression was accepted").into());
		};
		assert_eq!(error.current_context(), &ArchiveError::Encrypted);
		Ok(())
	}

	fn archive_with_entry() -> std::result::Result<File, Box<dyn Error>> {
		let file = tempfile()?;
		let mut writer = ZipWriter::new(file);
		writer.start_file(
			"Data/file.bin",
			SimpleFileOptions::default().compression_method(CompressionMethod::Stored),
		)?;
		writer.write_all(b"safe payload")?;
		Ok(writer.finish()?)
	}

	fn zip64_archive_bytes() -> std::result::Result<Vec<u8>, Box<dyn Error>> {
		let mut bytes = archive_bytes(archive_with_entry()?)?;
		let footer = end_of_central_directory(&bytes)?;
		let entries = u16::from_le_bytes(bytes[footer + 10..footer + 12].try_into()?);
		let central_size = u32::from_le_bytes(bytes[footer + 12..footer + 16].try_into()?);
		let central_offset = u32::from_le_bytes(bytes[footer + 16..footer + 20].try_into()?);

		let mut end_record = Vec::with_capacity(56);
		end_record.extend_from_slice(b"PK\x06\x06");
		end_record.extend_from_slice(&44_u64.to_le_bytes());
		end_record.extend_from_slice(&45_u16.to_le_bytes());
		end_record.extend_from_slice(&45_u16.to_le_bytes());
		end_record.extend_from_slice(&0_u32.to_le_bytes());
		end_record.extend_from_slice(&0_u32.to_le_bytes());
		end_record.extend_from_slice(&u64::from(entries).to_le_bytes());
		end_record.extend_from_slice(&u64::from(entries).to_le_bytes());
		end_record.extend_from_slice(&u64::from(central_size).to_le_bytes());
		end_record.extend_from_slice(&u64::from(central_offset).to_le_bytes());

		let mut locator = Vec::with_capacity(20);
		locator.extend_from_slice(b"PK\x06\x07");
		locator.extend_from_slice(&0_u32.to_le_bytes());
		locator.extend_from_slice(&u64::try_from(footer)?.to_le_bytes());
		locator.extend_from_slice(&1_u32.to_le_bytes());

		let mut classic_footer = bytes.split_off(footer);
		classic_footer[8..12].fill(0xff);
		classic_footer[12..20].fill(0xff);
		bytes.extend_from_slice(&end_record);
		bytes.extend_from_slice(&locator);
		bytes.extend_from_slice(&classic_footer);
		Ok(bytes)
	}

	fn archive_bytes(mut file: File) -> std::result::Result<Vec<u8>, Box<dyn Error>> {
		file.seek(SeekFrom::Start(0))?;
		let mut bytes = Vec::new();
		file.read_to_end(&mut bytes)?;
		Ok(bytes)
	}

	fn file_from_bytes(bytes: &[u8]) -> std::result::Result<File, Box<dyn Error>> {
		let mut file = tempfile()?;
		file.write_all(bytes)?;
		file.seek(SeekFrom::Start(0))?;
		Ok(file)
	}

	fn end_of_central_directory(bytes: &[u8]) -> std::result::Result<usize, IoError> {
		bytes.windows(4)
			.rposition(|window| window == b"PK\x05\x06")
			.ok_or_else(|| IoError::other("ZIP fixture footer is missing"))
	}

	fn archive_with_compression(method: u16) -> std::result::Result<File, Box<dyn Error>> {
		let file = tempfile()?;
		let mut writer = ZipWriter::new(file);
		writer.start_file(
			"Data/file.bin",
			SimpleFileOptions::default().compression_method(CompressionMethod::Stored),
		)?;
		writer.write_all(b"deliberately not encoded with the declared method")?;
		let mut file = writer.finish()?;

		file.seek(SeekFrom::Start(0))?;
		let mut bytes = Vec::new();
		file.read_to_end(&mut bytes)?;
		patch_compression_method(&mut bytes, b"PK\x03\x04", 8, method)?;
		patch_compression_method(&mut bytes, b"PK\x01\x02", 10, method)?;
		file.seek(SeekFrom::Start(0))?;
		file.write_all(&bytes)?;
		file.seek(SeekFrom::Start(0))?;
		Ok(file)
	}

	fn patch_compression_method(
		archive: &mut [u8],
		signature: &[u8; 4],
		method_offset: usize,
		method: u16,
	) -> std::result::Result<(), IoError> {
		let header = archive
			.windows(signature.len())
			.position(|window| window == signature)
			.ok_or_else(|| IoError::other("ZIP fixture header is missing"))?;
		archive[header + method_offset..header + method_offset + 2].copy_from_slice(&method.to_le_bytes());
		Ok(())
	}
}
