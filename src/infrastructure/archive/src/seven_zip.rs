use crate::entry::validate_entry_kind;
use crate::error::ArchiveError;
use crate::index::ArchiveMember;
use crate::index::ArchiveMemberCollector;
use crate::index::MemberKind;
use crate::limits::MAX_ARCHIVE_MEMBERS;
use crate::limits::MAX_ARCHIVE_METADATA_BYTES;
use crate::limits::MAX_ARCHIVE_WORK;
use crate::limits::MAX_COMPRESSION_RATIO;
use crate::limits::MAX_DICTIONARY_BYTES;
use crate::path::SafeArchivePath;
use rootcause::Report;
use rootcause::Result;
use rootcause::report;
use sevenz_rust2::ArchiveReader;
use sevenz_rust2::EncoderMethod;
use sevenz_rust2::Error;
use sevenz_rust2::Password;
use std::fs::File;
use std::io::Read;
use std::io::Seek;
use std::io::SeekFrom;
use std::time::Instant;
use tokio_util::sync::CancellationToken;

const SEVEN_ZIP_SIGNATURE: &[u8; 6] = b"7z\xbc\xaf'\x1c";
const SEVEN_ZIP_SIGNATURE_HEADER_BYTES: u64 = 32;
const SEVEN_ZIP_AES_METHOD: &[u8] = &[0x06, 0xf1, 0x07, 0x01];
const SEVEN_ZIP_COPY_METHOD: &[u8] = &[0x00];
const SEVEN_ZIP_LZMA_METHOD: &[u8] = &[0x03, 0x01, 0x01];
const SEVEN_ZIP_LZMA2_METHOD: &[u8] = &[0x21];
const MAX_ENCODED_HEADER_CODERS: u64 = 4;

const K_END: u8 = 0x00;
const K_HEADER: u8 = 0x01;
const K_ARCHIVE_PROPERTIES: u8 = 0x02;
const K_ADDITIONAL_STREAMS_INFO: u8 = 0x03;
const K_MAIN_STREAMS_INFO: u8 = 0x04;
const K_FILES_INFO: u8 = 0x05;
const K_PACK_INFO: u8 = 0x06;
const K_UNPACK_INFO: u8 = 0x07;
const K_SUB_STREAMS_INFO: u8 = 0x08;
const K_SIZE: u8 = 0x09;
const K_CRC: u8 = 0x0a;
const K_FOLDER: u8 = 0x0b;
const K_CODERS_UNPACK_SIZE: u8 = 0x0c;
const K_NUM_UNPACK_STREAM: u8 = 0x0d;
const K_EMPTY_STREAM: u8 = 0x0e;
const K_C_TIME: u8 = 0x12;
const K_A_TIME: u8 = 0x13;
const K_M_TIME: u8 = 0x14;
const K_WIN_ATTRIBUTES: u8 = 0x15;
const K_ENCODED_HEADER: u8 = 0x17;

pub(crate) fn index(
	source: File,
	cancellation: &CancellationToken,
	started: Instant,
) -> Result<Vec<ArchiveMember>, ArchiveError> {
	let reader = open_controlled(source, cancellation, started)?;
	// sevenz-rust2 assigns packed bytes only to a block's first file and reports zero for later solid
	// members. Validate the raw blocks before using the block-level ratio proof for every member.
	let mut members = ArchiveMemberCollector::with_validated_archive_blocks(reader.archive().files.len())?;
	validate_compression_blocks(&reader, cancellation, started)?;
	for (ordinal, entry) in reader.archive().files.iter().enumerate() {
		if cancellation.is_cancelled() {
			return Err(report!(ArchiveError::Cancelled));
		}
		if started.elapsed() > MAX_ARCHIVE_WORK {
			return Err(report!(ArchiveError::WorkLimit));
		}
		if entry.name.contains('\u{fffd}') {
			return Err(report!(ArchiveError::NonLosslessName));
		}
		if entry.is_anti_item {
			return Err(report!(ArchiveError::UnsafeEntryKind));
		}
		let windows_attributes = entry
			.has_windows_attributes
			.then_some(u64::from(entry.windows_attributes));
		let unix_mode = windows_attributes.map(|attributes| attributes >> 16);
		validate_entry_kind(entry.is_directory, windows_attributes, unix_mode)?;
		let normalized_name = if entry.is_directory {
			entry.name.trim_end_matches(['/', '\\'])
		} else {
			entry.name.as_str()
		};
		members.push(
			ArchiveMember {
				ordinal,
				path: SafeArchivePath::new(normalized_name)?,
				kind: if entry.is_directory {
					MemberKind::Directory
				} else {
					MemberKind::File
				},
				uncompressed_size: entry.size,
				compressed_size: entry.compressed_size,
			},
			cancellation,
			started,
		)?;
	}
	Ok(members.into_members())
}

pub(crate) fn open_controlled(
	mut source: File,
	cancellation: &CancellationToken,
	started: Instant,
) -> Result<ArchiveReader<File>, ArchiveError> {
	preflight_header(&mut source, cancellation, started)?;

	if cancellation.is_cancelled() {
		return Err(report!(ArchiveError::Cancelled));
	}
	if started.elapsed() > MAX_ARCHIVE_WORK {
		return Err(report!(ArchiveError::WorkLimit));
	}

	let reader = ArchiveReader::new(source, Password::empty()).map_err(map_error)?;
	validate_coders(&reader, cancellation, started)?;
	Ok(reader)
}

fn preflight_header(source: &mut File, cancellation: &CancellationToken, started: Instant) -> Result<(), ArchiveError> {
	if cancellation.is_cancelled() {
		return Err(report!(ArchiveError::Cancelled));
	}
	if started.elapsed() > MAX_ARCHIVE_WORK {
		return Err(report!(ArchiveError::WorkLimit));
	}

	let file_length = source
		.seek(SeekFrom::End(0))
		.map_err(|error| report!(error).context(ArchiveError::Io))?;
	source.seek(SeekFrom::Start(0))
		.map_err(|error| report!(error).context(ArchiveError::Io))?;
	let mut signature_header = [0_u8; SEVEN_ZIP_SIGNATURE_HEADER_BYTES as usize];
	source.read_exact(&mut signature_header)
		.map_err(|error| report!(error).context(ArchiveError::Io))?;

	if &signature_header[..SEVEN_ZIP_SIGNATURE.len()] != SEVEN_ZIP_SIGNATURE || signature_header[6] != 0 {
		return Err(report!(ArchiveError::InvalidArchive));
	}
	// sevenz-rust2 falls back to scanning the file when this region is all zero. That recovery path
	// has no trustworthy raw-header range to preflight, so retained-file parsing fails closed.
	if signature_header[12..].iter().all(|byte| *byte == 0) {
		return Err(report!(ArchiveError::InvalidArchive));
	}
	let expected_start_header_crc = u32::from_le_bytes(
		signature_header[8..12]
			.try_into()
			.map_err(|_| report!(ArchiveError::InvalidArchive))?,
	);
	if crc32fast::hash(&signature_header[12..]) != expected_start_header_crc {
		return Err(report!(ArchiveError::InvalidArchive));
	}

	let next_header_offset = u64::from_le_bytes(
		signature_header[12..20]
			.try_into()
			.map_err(|_| report!(ArchiveError::InvalidArchive))?,
	);
	let next_header_size = u64::from_le_bytes(
		signature_header[20..28]
			.try_into()
			.map_err(|_| report!(ArchiveError::InvalidArchive))?,
	);
	let expected_next_header_crc = u32::from_le_bytes(
		signature_header[28..32]
			.try_into()
			.map_err(|_| report!(ArchiveError::InvalidArchive))?,
	);
	if next_header_size == 0 || next_header_size > MAX_ARCHIVE_METADATA_BYTES {
		return Err(report!(ArchiveError::ExpansionLimit));
	}
	let next_header_start = SEVEN_ZIP_SIGNATURE_HEADER_BYTES
		.checked_add(next_header_offset)
		.ok_or_else(|| report!(ArchiveError::InvalidArchive))?;
	let next_header_end = next_header_start
		.checked_add(next_header_size)
		.filter(|end| *end <= file_length)
		.ok_or_else(|| report!(ArchiveError::InvalidArchive))?;

	source.seek(SeekFrom::Start(next_header_start))
		.map_err(|error| report!(error).context(ArchiveError::Io))?;
	let next_header_size = usize::try_from(next_header_size).map_err(|_| report!(ArchiveError::ExpansionLimit))?;
	let mut next_header = vec![0_u8; next_header_size];
	let mut bytes_read = 0;
	while bytes_read < next_header.len() {
		if cancellation.is_cancelled() {
			return Err(report!(ArchiveError::Cancelled));
		}
		if started.elapsed() > MAX_ARCHIVE_WORK {
			return Err(report!(ArchiveError::WorkLimit));
		}
		let chunk_end = bytes_read.saturating_add(64 * 1024).min(next_header.len());
		source.read_exact(&mut next_header[bytes_read..chunk_end])
			.map_err(|error| report!(error).context(ArchiveError::Io))?;
		bytes_read = chunk_end;
	}
	if crc32fast::hash(&next_header) != expected_next_header_crc {
		return Err(report!(ArchiveError::InvalidArchive));
	}

	let marker = *next_header
		.first()
		.ok_or_else(|| report!(ArchiveError::InvalidArchive))?;
	if marker == K_HEADER {
		return validate_raw_header(&next_header, cancellation, started).map_err(Report::new);
	}
	if marker != K_ENCODED_HEADER {
		return Err(report!(ArchiveError::InvalidArchive));
	}

	let plan = preflight_encoded_header(
		&next_header[1..],
		file_length,
		next_header_start,
		next_header_end,
		cancellation,
		started,
	)
	.map_err(Report::new)?;
	if let Some(expected_crc) = plan.packed_crc {
		source.seek(SeekFrom::Start(plan.packed_start))
			.map_err(|error| report!(error).context(ArchiveError::Io))?;
		let mut hasher = crc32fast::Hasher::new();
		let mut remaining = plan.packed_size;
		let mut buffer = [0_u8; 64 * 1024];
		while remaining > 0 {
			if cancellation.is_cancelled() {
				return Err(report!(ArchiveError::Cancelled));
			}
			if started.elapsed() > MAX_ARCHIVE_WORK {
				return Err(report!(ArchiveError::WorkLimit));
			}
			let wanted = usize::try_from(remaining.min(buffer.len() as u64))
				.map_err(|_| report!(ArchiveError::InvalidArchive))?;
			source.read_exact(&mut buffer[..wanted])
				.map_err(|error| report!(error).context(ArchiveError::Io))?;
			hasher.update(&buffer[..wanted]);
			remaining -= wanted as u64;
		}
		if hasher.finalize() != expected_crc {
			return Err(report!(ArchiveError::InvalidArchive));
		}
	}

	let decoded_header = decode_encoded_header(source, &plan, cancellation, started)?;
	validate_raw_header(&decoded_header, cancellation, started).map_err(Report::new)
}

#[derive(Clone, Copy)]
enum EncodedHeaderDecoder {
	Copy,
	Lzma { properties: u8, dictionary: u32 },
	Lzma2 { dictionary: u32 },
}

struct EncodedHeaderCoder {
	decoder: EncodedHeaderDecoder,
	unpacked_size: u64,
}

struct EncodedHeaderPlan {
	packed_start: u64,
	packed_size: u64,
	packed_crc: Option<u32>,
	unpacked_size: u64,
	unpacked_crc: Option<u32>,
	coders: Vec<EncodedHeaderCoder>,
}

// sevenz-rust2 has no public memory-limited encoded-header API. Parse its narrow StreamsInfo
// envelope here, then use lzma-rust2 directly so all decoder allocations are policy-bounded.
fn preflight_encoded_header(
	header: &[u8],
	file_length: u64,
	next_header_start: u64,
	next_header_end: u64,
	cancellation: &CancellationToken,
	started: Instant,
) -> std::result::Result<EncodedHeaderPlan, ArchiveError> {
	let mut header = HeaderCursor::new(header);
	if header.read_byte()? != K_PACK_INFO {
		return Err(ArchiveError::InvalidArchive);
	}
	let pack_position = header.read_variable_u64()?;
	let pack_stream_count = header.read_limited_count(MAX_ARCHIVE_MEMBERS)?;
	if pack_stream_count != 1 {
		return Err(ArchiveError::InvalidArchive);
	}
	if header.read_byte()? != K_SIZE {
		return Err(ArchiveError::InvalidArchive);
	}
	let packed_size = header.read_variable_u64()?;
	if packed_size == 0 || packed_size > MAX_ARCHIVE_METADATA_BYTES {
		return Err(ArchiveError::ExpansionLimit);
	}
	let mut marker = header.read_byte()?;
	let packed_crc = if marker == K_CRC {
		let mut crcs = read_defined_crcs(&mut header, pack_stream_count, cancellation, started)?;
		marker = header.read_byte()?;
		crcs.pop().flatten()
	} else {
		None
	};
	if marker != K_END {
		return Err(ArchiveError::InvalidArchive);
	}

	let packed_start = SEVEN_ZIP_SIGNATURE_HEADER_BYTES
		.checked_add(pack_position)
		.ok_or(ArchiveError::InvalidArchive)?;
	let packed_end = packed_start
		.checked_add(packed_size)
		.filter(|end| *end <= file_length && *end <= next_header_start)
		.ok_or(ArchiveError::InvalidArchive)?;
	if packed_start >= packed_end || next_header_start >= next_header_end {
		return Err(ArchiveError::InvalidArchive);
	}

	if header.read_byte()? != K_UNPACK_INFO || header.read_byte()? != K_FOLDER {
		return Err(ArchiveError::InvalidArchive);
	}
	let block_count = header.read_limited_count(MAX_ARCHIVE_MEMBERS)?;
	if block_count != 1 || header.read_byte()? != 0 {
		return Err(ArchiveError::InvalidArchive);
	}

	let coder_count = header.read_limited_count(MAX_ENCODED_HEADER_CODERS as usize)?;
	if coder_count == 0 {
		return Err(ArchiveError::InvalidArchive);
	}
	let mut decoders = Vec::with_capacity(coder_count);
	let mut total_input_streams = 0_usize;
	let mut total_output_streams = 0_usize;
	let mut total_dictionary_bytes = 0_u64;
	for _ in 0..coder_count {
		if cancellation.is_cancelled() {
			return Err(ArchiveError::Cancelled);
		}
		if started.elapsed() > MAX_ARCHIVE_WORK {
			return Err(ArchiveError::WorkLimit);
		}
		let flags = header.read_byte()?;
		if flags & 0xc0 != 0 {
			return Err(ArchiveError::InvalidArchive);
		}
		let method_id_size = usize::from(flags & 0x0f);
		if method_id_size == 0 {
			return Err(ArchiveError::InvalidArchive);
		}
		let method = header.read_bytes(method_id_size)?;
		let is_simple = flags & 0x10 == 0;
		let (input_streams, output_streams) = if is_simple {
			(1, 1)
		} else {
			(
				header.read_limited_count(MAX_ARCHIVE_MEMBERS)?,
				header.read_limited_count(MAX_ARCHIVE_MEMBERS)?,
			)
		};
		if input_streams != 1 || output_streams != 1 {
			return Err(ArchiveError::InvalidArchive);
		}
		total_input_streams = total_input_streams
			.checked_add(input_streams)
			.filter(|total| *total <= MAX_ARCHIVE_MEMBERS)
			.ok_or(ArchiveError::ExpansionLimit)?;
		total_output_streams = total_output_streams
			.checked_add(output_streams)
			.filter(|total| *total <= MAX_ARCHIVE_MEMBERS)
			.ok_or(ArchiveError::ExpansionLimit)?;

		let properties = if flags & 0x20 != 0 {
			let property_size = header.read_limited_count(MAX_ARCHIVE_METADATA_BYTES as usize)?;
			header.read_bytes(property_size)?
		} else {
			&[]
		};
		if method == SEVEN_ZIP_AES_METHOD {
			return Err(ArchiveError::Encrypted);
		}
		let (decoder, dictionary_bytes) = if method == SEVEN_ZIP_LZMA_METHOD {
			let [properties, dictionary @ ..] = properties else {
				return Err(ArchiveError::InvalidArchive);
			};
			let dictionary: [u8; 4] = dictionary.try_into().map_err(|_| ArchiveError::InvalidArchive)?;
			let dictionary = u32::from_le_bytes(dictionary);
			(
				EncodedHeaderDecoder::Lzma {
					properties: *properties,
					dictionary,
				},
				u64::from(dictionary),
			)
		} else if method == SEVEN_ZIP_LZMA2_METHOD {
			let [property] = properties else {
				return Err(ArchiveError::InvalidArchive);
			};
			if *property > 40 {
				return Err(ArchiveError::InvalidArchive);
			}
			let dictionary = if *property == 40 {
				u32::MAX
			} else {
				(2 | u32::from(property & 1)) << (u32::from(property / 2) + 11)
			};
			(EncodedHeaderDecoder::Lzma2 { dictionary }, u64::from(dictionary))
		} else if method == SEVEN_ZIP_COPY_METHOD && properties.is_empty() {
			(EncodedHeaderDecoder::Copy, 0)
		} else {
			return Err(ArchiveError::InvalidArchive);
		};
		total_dictionary_bytes = total_dictionary_bytes
			.checked_add(dictionary_bytes)
			.ok_or(ArchiveError::DictionaryLimit)?;
		if total_dictionary_bytes > MAX_DICTIONARY_BYTES {
			return Err(ArchiveError::DictionaryLimit);
		}
		decoders.push(decoder);
	}

	let bind_pair_count = total_output_streams
		.checked_sub(1)
		.ok_or(ArchiveError::InvalidArchive)?;
	if total_input_streams.checked_sub(bind_pair_count) != Some(1) {
		return Err(ArchiveError::InvalidArchive);
	}
	let mut bind_pairs = Vec::with_capacity(bind_pair_count);
	for _ in 0..bind_pair_count {
		if cancellation.is_cancelled() {
			return Err(ArchiveError::Cancelled);
		}
		if started.elapsed() > MAX_ARCHIVE_WORK {
			return Err(ArchiveError::WorkLimit);
		}
		let input_index = header.read_limited_count(MAX_ARCHIVE_MEMBERS)?;
		let output_index = header.read_limited_count(MAX_ARCHIVE_MEMBERS)?;
		if input_index >= total_input_streams
			|| output_index >= total_output_streams
			|| bind_pairs.iter().any(|(bound_input, bound_output)| {
				*bound_input == input_index || *bound_output == output_index
			}) {
			return Err(ArchiveError::InvalidArchive);
		}
		bind_pairs.push((input_index, output_index));
	}
	let packed_input = (0..total_input_streams)
		.find(|input| !bind_pairs.iter().any(|(bound_input, _)| *bound_input == *input))
		.ok_or(ArchiveError::InvalidArchive)?;
	let mut coder_order = Vec::with_capacity(coder_count);
	let mut coder_index = packed_input;
	for _ in 0..coder_count {
		if cancellation.is_cancelled() {
			return Err(ArchiveError::Cancelled);
		}
		if started.elapsed() > MAX_ARCHIVE_WORK {
			return Err(ArchiveError::WorkLimit);
		}
		if coder_index >= coder_count || coder_order.contains(&coder_index) {
			return Err(ArchiveError::InvalidArchive);
		}
		coder_order.push(coder_index);
		let Some((next_input, _)) = bind_pairs.iter().find(|(_, output)| *output == coder_index) else {
			break;
		};
		coder_index = *next_input;
	}
	if coder_order.len() != coder_count {
		return Err(ArchiveError::InvalidArchive);
	}

	if header.read_byte()? != K_CODERS_UNPACK_SIZE {
		return Err(ArchiveError::InvalidArchive);
	}
	let final_output = (0..total_output_streams)
		.find(|output| !bind_pairs.iter().any(|(_, bound_output)| *bound_output == *output))
		.ok_or(ArchiveError::InvalidArchive)?;
	let mut unpack_sizes = Vec::with_capacity(total_output_streams);
	for _ in 0..total_output_streams {
		if cancellation.is_cancelled() {
			return Err(ArchiveError::Cancelled);
		}
		if started.elapsed() > MAX_ARCHIVE_WORK {
			return Err(ArchiveError::WorkLimit);
		}
		unpack_sizes.push(header.read_variable_u64()?);
	}
	let unpacked_size = *unpack_sizes.get(final_output).ok_or(ArchiveError::InvalidArchive)?;
	if unpacked_size == 0 || unpacked_size > MAX_ARCHIVE_METADATA_BYTES {
		return Err(ArchiveError::ExpansionLimit);
	}
	marker = header.read_byte()?;
	let unpacked_crc = if marker == K_CRC {
		let mut crcs = read_defined_crcs(&mut header, block_count, cancellation, started)?;
		marker = header.read_byte()?;
		crcs.pop().flatten()
	} else {
		None
	};
	if marker != K_END {
		return Err(ArchiveError::InvalidArchive);
	}

	marker = header.read_byte()?;
	if marker == K_SUB_STREAMS_INFO {
		marker = header.read_byte()?;
		if marker == K_NUM_UNPACK_STREAM {
			if header.read_limited_count(MAX_ARCHIVE_MEMBERS)? != 1 {
				return Err(ArchiveError::InvalidArchive);
			}
			marker = header.read_byte()?;
		}
		if marker == K_SIZE {
			return Err(ArchiveError::InvalidArchive);
		}
		if marker == K_CRC {
			read_defined_crcs(&mut header, 1, cancellation, started)?;
			marker = header.read_byte()?;
		}
		if marker != K_END {
			return Err(ArchiveError::InvalidArchive);
		}
		marker = header.read_byte()?;
	}
	if marker != K_END || !header.is_empty() {
		return Err(ArchiveError::InvalidArchive);
	}

	let coders = coder_order
		.into_iter()
		.map(|index| EncodedHeaderCoder {
			decoder: decoders[index],
			unpacked_size: unpack_sizes[index],
		})
		.collect();
	Ok(EncodedHeaderPlan {
		packed_start,
		packed_size,
		packed_crc,
		unpacked_size,
		unpacked_crc,
		coders,
	})
}

fn decode_encoded_header(
	source: &mut File,
	plan: &EncodedHeaderPlan,
	cancellation: &CancellationToken,
	started: Instant,
) -> Result<Vec<u8>, ArchiveError> {
	source.seek(SeekFrom::Start(plan.packed_start))
		.map_err(|error| report!(error).context(ArchiveError::Io))?;
	let packed = source.take(plan.packed_size);
	let mut decoder: Box<dyn Read + '_> = Box::new(packed);
	for coder in &plan.coders {
		decoder = match coder.decoder {
			EncodedHeaderDecoder::Copy => decoder,
			EncodedHeaderDecoder::Lzma { properties, dictionary } => Box::new(
				lzma_rust2::LzmaReader::new_with_props(
					decoder,
					coder.unpacked_size,
					properties,
					dictionary,
					None,
				)
				.map_err(|error| report!(error).context(ArchiveError::InvalidArchive))?,
			),
			EncodedHeaderDecoder::Lzma2 { dictionary } => {
				Box::new(lzma_rust2::Lzma2Reader::new(decoder, dictionary, None))
			}
		};
	}

	let expected_size = usize::try_from(plan.unpacked_size).map_err(|_| report!(ArchiveError::ExpansionLimit))?;
	let mut decoded = Vec::with_capacity(expected_size.min(64 * 1024));
	let mut buffer = [0_u8; 64 * 1024];
	while decoded.len() < expected_size {
		if cancellation.is_cancelled() {
			return Err(report!(ArchiveError::Cancelled));
		}
		if started.elapsed() > MAX_ARCHIVE_WORK {
			return Err(report!(ArchiveError::WorkLimit));
		}
		let wanted = (expected_size - decoded.len()).min(buffer.len());
		let count = decoder
			.read(&mut buffer[..wanted])
			.map_err(|error| report!(error).context(ArchiveError::InvalidArchive))?;
		if count == 0 {
			return Err(report!(ArchiveError::InvalidArchive));
		}
		decoded.extend_from_slice(&buffer[..count]);
	}
	if decoder
		.read(&mut buffer[..1])
		.map_err(|error| report!(error).context(ArchiveError::InvalidArchive))?
		!= 0
	{
		return Err(report!(ArchiveError::InvalidArchive));
	}
	if plan.unpacked_crc
		.is_some_and(|expected_crc| crc32fast::hash(&decoded) != expected_crc)
	{
		return Err(report!(ArchiveError::InvalidArchive));
	}
	Ok(decoded)
}

#[derive(Clone, Copy)]
struct FolderShape {
	output_streams: usize,
	has_crc: bool,
	unpack_streams: usize,
}

fn validate_raw_header(
	header: &[u8],
	cancellation: &CancellationToken,
	started: Instant,
) -> std::result::Result<(), ArchiveError> {
	let mut header = HeaderCursor::new(header);
	if header.read_byte()? != K_HEADER {
		return Err(ArchiveError::InvalidArchive);
	}
	let mut marker = header.read_byte()?;
	if marker == K_ARCHIVE_PROPERTIES {
		loop {
			if cancellation.is_cancelled() {
				return Err(ArchiveError::Cancelled);
			}
			if started.elapsed() > MAX_ARCHIVE_WORK {
				return Err(ArchiveError::WorkLimit);
			}
			let property = header.read_byte()?;
			if property == K_END {
				break;
			}
			let size = header.read_limited_count(MAX_ARCHIVE_METADATA_BYTES as usize)?;
			header.read_bytes(size)?;
		}
		marker = header.read_byte()?;
	}
	if marker == K_ADDITIONAL_STREAMS_INFO {
		return Err(ArchiveError::InvalidArchive);
	}
	if marker == K_MAIN_STREAMS_INFO {
		validate_raw_streams_info(&mut header, cancellation, started)?;
		marker = header.read_byte()?;
	}
	if marker == K_FILES_INFO {
		let file_count = header.read_limited_count(MAX_ARCHIVE_MEMBERS)?;
		loop {
			if cancellation.is_cancelled() {
				return Err(ArchiveError::Cancelled);
			}
			if started.elapsed() > MAX_ARCHIVE_WORK {
				return Err(ArchiveError::WorkLimit);
			}
			let property = header.read_byte()?;
			if property == K_END {
				break;
			}
			let size = header.read_limited_count(MAX_ARCHIVE_METADATA_BYTES as usize)?;
			if matches!(
				property,
				K_EMPTY_STREAM | K_C_TIME | K_A_TIME | K_M_TIME | K_WIN_ATTRIBUTES
			) && file_count == 0
			{
				return Err(ArchiveError::InvalidArchive);
			}
			header.read_bytes(size)?;
		}
		marker = header.read_byte()?;
	}
	if marker != K_END || !header.is_empty() {
		return Err(ArchiveError::InvalidArchive);
	}
	Ok(())
}

fn validate_raw_streams_info(
	header: &mut HeaderCursor<'_>,
	cancellation: &CancellationToken,
	started: Instant,
) -> std::result::Result<(), ArchiveError> {
	let mut marker = header.read_byte()?;
	if marker == K_PACK_INFO {
		header.read_variable_u64()?;
		let pack_stream_count = header.read_limited_count(MAX_ARCHIVE_MEMBERS)?;
		marker = header.read_byte()?;
		if marker == K_SIZE {
			for _ in 0..pack_stream_count {
				if cancellation.is_cancelled() {
					return Err(ArchiveError::Cancelled);
				}
				if started.elapsed() > MAX_ARCHIVE_WORK {
					return Err(ArchiveError::WorkLimit);
				}
				header.read_variable_u64()?;
			}
			marker = header.read_byte()?;
		}
		if marker == K_CRC {
			read_defined_crcs(header, pack_stream_count, cancellation, started)?;
			marker = header.read_byte()?;
		}
		if marker != K_END {
			return Err(ArchiveError::InvalidArchive);
		}
		marker = header.read_byte()?;
	}

	let mut folders = Vec::new();
	if marker == K_UNPACK_INFO {
		if header.read_byte()? != K_FOLDER {
			return Err(ArchiveError::InvalidArchive);
		}
		let folder_count = header.read_limited_count(MAX_ARCHIVE_MEMBERS)?;
		if header.read_byte()? != 0 {
			return Err(ArchiveError::InvalidArchive);
		}
		folders.reserve_exact(folder_count);
		let mut total_coders = 0_usize;
		let mut total_inputs = 0_usize;
		let mut total_outputs = 0_usize;
		let mut total_packed_streams = 0_usize;
		for _ in 0..folder_count {
			if cancellation.is_cancelled() {
				return Err(ArchiveError::Cancelled);
			}
			if started.elapsed() > MAX_ARCHIVE_WORK {
				return Err(ArchiveError::WorkLimit);
			}
			let coder_count = header.read_limited_count(MAX_ARCHIVE_MEMBERS)?;
			if coder_count == 0 {
				return Err(ArchiveError::InvalidArchive);
			}
			total_coders = total_coders
				.checked_add(coder_count)
				.filter(|total| *total <= MAX_ARCHIVE_MEMBERS)
				.ok_or(ArchiveError::ExpansionLimit)?;
			let mut folder_inputs = 0_usize;
			let mut folder_outputs = 0_usize;
			for _ in 0..coder_count {
				if cancellation.is_cancelled() {
					return Err(ArchiveError::Cancelled);
				}
				if started.elapsed() > MAX_ARCHIVE_WORK {
					return Err(ArchiveError::WorkLimit);
				}
				let flags = header.read_byte()?;
				if flags & 0xc0 != 0 {
					return Err(ArchiveError::InvalidArchive);
				}
				let method_size = usize::from(flags & 0x0f);
				if method_size == 0 {
					return Err(ArchiveError::InvalidArchive);
				}
				header.read_bytes(method_size)?;
				let (inputs, outputs) = if flags & 0x10 == 0 {
					(1, 1)
				} else {
					(
						header.read_limited_count(MAX_ARCHIVE_MEMBERS)?,
						header.read_limited_count(MAX_ARCHIVE_MEMBERS)?,
					)
				};
				if inputs == 0 || outputs == 0 {
					return Err(ArchiveError::InvalidArchive);
				}
				folder_inputs = folder_inputs
					.checked_add(inputs)
					.filter(|total| *total <= MAX_ARCHIVE_MEMBERS)
					.ok_or(ArchiveError::ExpansionLimit)?;
				folder_outputs = folder_outputs
					.checked_add(outputs)
					.filter(|total| *total <= MAX_ARCHIVE_MEMBERS)
					.ok_or(ArchiveError::ExpansionLimit)?;
				if flags & 0x20 != 0 {
					let property_size =
						header.read_limited_count(MAX_ARCHIVE_METADATA_BYTES as usize)?;
					header.read_bytes(property_size)?;
				}
			}
			total_inputs = total_inputs
				.checked_add(folder_inputs)
				.filter(|total| *total <= MAX_ARCHIVE_MEMBERS)
				.ok_or(ArchiveError::ExpansionLimit)?;
			total_outputs = total_outputs
				.checked_add(folder_outputs)
				.filter(|total| *total <= MAX_ARCHIVE_MEMBERS)
				.ok_or(ArchiveError::ExpansionLimit)?;

			let bind_pair_count = folder_outputs.checked_sub(1).ok_or(ArchiveError::InvalidArchive)?;
			let mut bound_inputs = vec![false; folder_inputs];
			let mut bound_outputs = vec![false; folder_outputs];
			for _ in 0..bind_pair_count {
				if cancellation.is_cancelled() {
					return Err(ArchiveError::Cancelled);
				}
				if started.elapsed() > MAX_ARCHIVE_WORK {
					return Err(ArchiveError::WorkLimit);
				}
				let input = header.read_limited_count(MAX_ARCHIVE_MEMBERS)?;
				let output = header.read_limited_count(MAX_ARCHIVE_MEMBERS)?;
				if input >= folder_inputs
					|| output >= folder_outputs || bound_inputs[input]
					|| bound_outputs[output]
				{
					return Err(ArchiveError::InvalidArchive);
				}
				bound_inputs[input] = true;
				bound_outputs[output] = true;
			}
			let packed_stream_count = folder_inputs
				.checked_sub(bind_pair_count)
				.ok_or(ArchiveError::InvalidArchive)?;
			if packed_stream_count == 0 {
				return Err(ArchiveError::InvalidArchive);
			}
			total_packed_streams = total_packed_streams
				.checked_add(packed_stream_count)
				.filter(|total| *total <= MAX_ARCHIVE_MEMBERS)
				.ok_or(ArchiveError::ExpansionLimit)?;
			if packed_stream_count > 1 {
				let mut packed_inputs = vec![false; folder_inputs];
				for _ in 0..packed_stream_count {
					if cancellation.is_cancelled() {
						return Err(ArchiveError::Cancelled);
					}
					if started.elapsed() > MAX_ARCHIVE_WORK {
						return Err(ArchiveError::WorkLimit);
					}
					let input = header.read_limited_count(MAX_ARCHIVE_MEMBERS)?;
					if input >= folder_inputs || bound_inputs[input] || packed_inputs[input] {
						return Err(ArchiveError::InvalidArchive);
					}
					packed_inputs[input] = true;
				}
			}
			folders.push(FolderShape {
				output_streams: folder_outputs,
				has_crc: false,
				unpack_streams: 1,
			});
		}

		if header.read_byte()? != K_CODERS_UNPACK_SIZE {
			return Err(ArchiveError::InvalidArchive);
		}
		for folder in &folders {
			if cancellation.is_cancelled() {
				return Err(ArchiveError::Cancelled);
			}
			if started.elapsed() > MAX_ARCHIVE_WORK {
				return Err(ArchiveError::WorkLimit);
			}
			for _ in 0..folder.output_streams {
				if cancellation.is_cancelled() {
					return Err(ArchiveError::Cancelled);
				}
				if started.elapsed() > MAX_ARCHIVE_WORK {
					return Err(ArchiveError::WorkLimit);
				}
				header.read_variable_u64()?;
			}
		}
		marker = header.read_byte()?;
		if marker == K_CRC {
			let crcs = read_defined_crcs(header, folder_count, cancellation, started)?;
			for (folder, crc) in folders.iter_mut().zip(crcs) {
				if cancellation.is_cancelled() {
					return Err(ArchiveError::Cancelled);
				}
				if started.elapsed() > MAX_ARCHIVE_WORK {
					return Err(ArchiveError::WorkLimit);
				}
				folder.has_crc = crc.is_some();
			}
			marker = header.read_byte()?;
		}
		if marker != K_END {
			return Err(ArchiveError::InvalidArchive);
		}
		marker = header.read_byte()?;
	}

	if marker == K_SUB_STREAMS_INFO {
		marker = header.read_byte()?;
		if marker == K_NUM_UNPACK_STREAM {
			let mut total_unpack_streams = 0_usize;
			for folder in &mut folders {
				if cancellation.is_cancelled() {
					return Err(ArchiveError::Cancelled);
				}
				if started.elapsed() > MAX_ARCHIVE_WORK {
					return Err(ArchiveError::WorkLimit);
				}
				folder.unpack_streams = header.read_limited_count(MAX_ARCHIVE_MEMBERS)?;
				total_unpack_streams = total_unpack_streams
					.checked_add(folder.unpack_streams)
					.filter(|total| *total <= MAX_ARCHIVE_MEMBERS)
					.ok_or(ArchiveError::ExpansionLimit)?;
			}
			marker = header.read_byte()?;
		}
		if marker == K_SIZE {
			for folder in &folders {
				if cancellation.is_cancelled() {
					return Err(ArchiveError::Cancelled);
				}
				if started.elapsed() > MAX_ARCHIVE_WORK {
					return Err(ArchiveError::WorkLimit);
				}
				let explicit_sizes = folder.unpack_streams.saturating_sub(1);
				for _ in 0..explicit_sizes {
					if cancellation.is_cancelled() {
						return Err(ArchiveError::Cancelled);
					}
					if started.elapsed() > MAX_ARCHIVE_WORK {
						return Err(ArchiveError::WorkLimit);
					}
					header.read_variable_u64()?;
				}
			}
			marker = header.read_byte()?;
		}
		if marker == K_CRC {
			let mut digest_count = 0_usize;
			for folder in &folders {
				if cancellation.is_cancelled() {
					return Err(ArchiveError::Cancelled);
				}
				if started.elapsed() > MAX_ARCHIVE_WORK {
					return Err(ArchiveError::WorkLimit);
				}
				let count = if folder.unpack_streams == 1 && folder.has_crc {
					0
				} else {
					folder.unpack_streams
				};
				digest_count = digest_count
					.checked_add(count)
					.filter(|value| *value <= MAX_ARCHIVE_MEMBERS)
					.ok_or(ArchiveError::ExpansionLimit)?;
			}
			read_defined_crcs(header, digest_count, cancellation, started)?;
			marker = header.read_byte()?;
		}
		if marker != K_END {
			return Err(ArchiveError::InvalidArchive);
		}
		marker = header.read_byte()?;
	}
	if marker != K_END {
		return Err(ArchiveError::InvalidArchive);
	}
	Ok(())
}

fn read_defined_crcs(
	header: &mut HeaderCursor<'_>,
	count: usize,
	cancellation: &CancellationToken,
	started: Instant,
) -> std::result::Result<Vec<Option<u32>>, ArchiveError> {
	if count > MAX_ARCHIVE_MEMBERS {
		return Err(ArchiveError::ExpansionLimit);
	}
	let all_defined = header.read_byte()? != 0;
	let defined_bits = if all_defined {
		None
	} else {
		Some(header.read_bytes(count.div_ceil(8))?)
	};
	let mut crcs = Vec::with_capacity(count);
	for index in 0..count {
		if cancellation.is_cancelled() {
			return Err(ArchiveError::Cancelled);
		}
		if started.elapsed() > MAX_ARCHIVE_WORK {
			return Err(ArchiveError::WorkLimit);
		}
		let is_defined = defined_bits.is_none_or(|bits| bits[index / 8] & (0x80_u8 >> (index % 8)) != 0);
		crcs.push(if is_defined { Some(header.read_u32()?) } else { None });
	}
	Ok(crcs)
}

struct HeaderCursor<'a> {
	bytes: &'a [u8],
	position: usize,
}

impl<'a> HeaderCursor<'a> {
	fn new(bytes: &'a [u8]) -> Self {
		Self { bytes, position: 0 }
	}

	fn read_byte(&mut self) -> std::result::Result<u8, ArchiveError> {
		let byte = *self.bytes.get(self.position).ok_or(ArchiveError::InvalidArchive)?;
		self.position += 1;
		Ok(byte)
	}

	fn read_bytes(&mut self, length: usize) -> std::result::Result<&'a [u8], ArchiveError> {
		let end = self.position.checked_add(length).ok_or(ArchiveError::InvalidArchive)?;
		let bytes = self.bytes.get(self.position..end).ok_or(ArchiveError::InvalidArchive)?;
		self.position = end;
		Ok(bytes)
	}

	fn read_u32(&mut self) -> std::result::Result<u32, ArchiveError> {
		let bytes: [u8; 4] = self
			.read_bytes(4)?
			.try_into()
			.map_err(|_| ArchiveError::InvalidArchive)?;
		Ok(u32::from_le_bytes(bytes))
	}

	fn read_variable_u64(&mut self) -> std::result::Result<u64, ArchiveError> {
		let first = u64::from(self.read_byte()?);
		let mut mask = 0x80_u64;
		let mut value = 0_u64;
		for index in 0..8 {
			if first & mask == 0 {
				return Ok(value | ((first & (mask - 1)) << (8 * index)));
			}
			value |= u64::from(self.read_byte()?) << (8 * index);
			mask >>= 1;
		}
		Ok(value)
	}

	fn read_limited_count(&mut self, maximum: usize) -> std::result::Result<usize, ArchiveError> {
		let value = self.read_variable_u64()?;
		if value > maximum as u64 {
			return Err(ArchiveError::ExpansionLimit);
		}
		usize::try_from(value).map_err(|_| ArchiveError::ExpansionLimit)
	}

	fn is_empty(&self) -> bool {
		self.position == self.bytes.len()
	}
}

fn validate_compression_blocks(
	reader: &ArchiveReader<File>,
	cancellation: &CancellationToken,
	started: Instant,
) -> Result<(), ArchiveError> {
	let archive = reader.archive();
	let first_pack_streams = archive.stream_map.block_first_pack_stream_index();
	if first_pack_streams.len() != archive.blocks.len()
		|| archive.stream_map.file_block_index.len() != archive.files.len()
		|| first_pack_streams.first().is_some_and(|first| *first != 0)
	{
		return Err(report!(ArchiveError::InvalidArchive));
	}

	let mut mapped_unpack_sizes = vec![0_u64; archive.blocks.len()];
	for (entry, block_index) in archive.files.iter().zip(&archive.stream_map.file_block_index) {
		if cancellation.is_cancelled() {
			return Err(report!(ArchiveError::Cancelled));
		}
		if started.elapsed() > MAX_ARCHIVE_WORK {
			return Err(report!(ArchiveError::WorkLimit));
		}
		let Some(block_index) = block_index else {
			if entry.size > 0 {
				return Err(report!(ArchiveError::InvalidArchive));
			}
			continue;
		};
		let mapped_unpack_size = mapped_unpack_sizes
			.get_mut(*block_index)
			.ok_or_else(|| report!(ArchiveError::InvalidArchive))?;
		*mapped_unpack_size = mapped_unpack_size
			.checked_add(entry.size)
			.ok_or_else(|| report!(ArchiveError::ExpansionLimit))?;
	}

	for (block_index, block) in archive.blocks.iter().enumerate() {
		if cancellation.is_cancelled() {
			return Err(report!(ArchiveError::Cancelled));
		}
		if started.elapsed() > MAX_ARCHIVE_WORK {
			return Err(report!(ArchiveError::WorkLimit));
		}
		let unpack_size = block.get_unpack_size();
		if mapped_unpack_sizes[block_index] != unpack_size {
			return Err(report!(ArchiveError::InvalidArchive));
		}
		let first_pack_stream = first_pack_streams[block_index];
		let next_pack_stream = first_pack_streams
			.get(block_index + 1)
			.copied()
			.unwrap_or_else(|| archive.pack_sizes().len());
		let pack_sizes = archive
			.pack_sizes()
			.get(first_pack_stream..next_pack_stream)
			.ok_or_else(|| report!(ArchiveError::InvalidArchive))?;
		validate_block_compression_ratio(unpack_size, pack_sizes, cancellation, started)?;
	}
	Ok(())
}

fn validate_block_compression_ratio(
	unpack_size: u64,
	pack_sizes: &[u64],
	cancellation: &CancellationToken,
	started: Instant,
) -> Result<(), ArchiveError> {
	let mut packed_size = 0_u64;
	for stream_size in pack_sizes {
		if cancellation.is_cancelled() {
			return Err(report!(ArchiveError::Cancelled));
		}
		if started.elapsed() > MAX_ARCHIVE_WORK {
			return Err(report!(ArchiveError::WorkLimit));
		}
		packed_size = packed_size
			.checked_add(*stream_size)
			.ok_or_else(|| report!(ArchiveError::ExpansionLimit))?;
	}
	if unpack_size == 0 {
		return Ok(());
	}
	let maximum_unpack_size = packed_size
		.checked_mul(MAX_COMPRESSION_RATIO)
		.ok_or_else(|| report!(ArchiveError::ExpansionLimit))?;
	if packed_size == 0 || unpack_size > maximum_unpack_size {
		return Err(report!(ArchiveError::ExpansionLimit));
	}
	Ok(())
}

fn validate_coders(
	reader: &ArchiveReader<File>,
	cancellation: &CancellationToken,
	started: Instant,
) -> Result<(), ArchiveError> {
	for block in &reader.archive().blocks {
		if cancellation.is_cancelled() {
			return Err(report!(ArchiveError::Cancelled));
		}
		if started.elapsed() > MAX_ARCHIVE_WORK {
			return Err(report!(ArchiveError::WorkLimit));
		}
		let mut total_dictionary_bytes = 0_u64;
		for coder in &block.coders {
			if cancellation.is_cancelled() {
				return Err(report!(ArchiveError::Cancelled));
			}
			if started.elapsed() > MAX_ARCHIVE_WORK {
				return Err(report!(ArchiveError::WorkLimit));
			}
			let method = coder.encoder_method_id();
			if method == SEVEN_ZIP_AES_METHOD {
				return Err(report!(ArchiveError::Encrypted));
			}
			let dictionary_size = if method == EncoderMethod::ID_LZMA {
				let [_, dictionary @ ..] = coder.properties() else {
					return Err(report!(ArchiveError::InvalidArchive));
				};
				let dictionary: [u8; 4] = dictionary
					.try_into()
					.map_err(|_| report!(ArchiveError::InvalidArchive))?;
				u64::from(u32::from_le_bytes(dictionary))
			} else if method == EncoderMethod::ID_LZMA2 {
				let [property] = coder.properties() else {
					return Err(report!(ArchiveError::InvalidArchive));
				};
				if *property > 40 {
					return Err(report!(ArchiveError::InvalidArchive));
				}
				if *property == 40 {
					u64::from(u32::MAX)
				} else {
					u64::from(2 | u32::from(property & 1)) << (u32::from(property / 2) + 11)
				}
			} else if method == EncoderMethod::ID_PPMD {
				let [_, dictionary @ ..] = coder.properties() else {
					return Err(report!(ArchiveError::InvalidArchive));
				};
				let dictionary: [u8; 4] = dictionary
					.try_into()
					.map_err(|_| report!(ArchiveError::InvalidArchive))?;
				u64::from(u32::from_le_bytes(dictionary))
			} else if method == EncoderMethod::ID_ZSTD {
				return Err(report!(ArchiveError::DictionaryLimit));
			} else {
				0
			};
			total_dictionary_bytes = total_dictionary_bytes
				.checked_add(dictionary_size)
				.ok_or_else(|| report!(ArchiveError::DictionaryLimit))?;
			if total_dictionary_bytes > MAX_DICTIONARY_BYTES {
				return Err(report!(ArchiveError::DictionaryLimit));
			}
		}
	}
	Ok(())
}

pub(crate) fn map_error(error: Error) -> Report<ArchiveError> {
	let detail = error.to_string().to_ascii_lowercase();
	let context = if detail.contains("password") || detail.contains("encrypt") {
		ArchiveError::Encrypted
	} else if detail.contains("volume") || detail.contains("split") {
		ArchiveError::SplitArchive
	} else if detail.contains("mem") || detail.contains("dictionary") {
		ArchiveError::DictionaryLimit
	} else {
		ArchiveError::InvalidArchive
	};
	report!(error).context(context)
}

#[cfg(test)]
mod tests {
	use super::K_CODERS_UNPACK_SIZE;
	use super::K_EMPTY_STREAM;
	use super::K_ENCODED_HEADER;
	use super::K_END;
	use super::K_FILES_INFO;
	use super::K_FOLDER;
	use super::K_HEADER;
	use super::K_MAIN_STREAMS_INFO;
	use super::K_PACK_INFO;
	use super::K_SIZE;
	use super::K_UNPACK_INFO;
	use super::SEVEN_ZIP_AES_METHOD;
	use super::SEVEN_ZIP_LZMA_METHOD;
	use super::index;
	use super::open_controlled;
	use super::preflight_encoded_header;
	use super::validate_block_compression_ratio;
	use super::validate_raw_header;
	use crate::error::ArchiveError;
	use crate::limits::MAX_ARCHIVE_MEMBERS;
	use crate::limits::MAX_ARCHIVE_METADATA_BYTES;
	use crate::limits::MAX_COMPRESSION_RATIO;
	use crate::limits::MAX_DICTIONARY_BYTES;
	use sevenz_rust2::ArchiveEntry;
	use sevenz_rust2::ArchiveWriter;
	use sevenz_rust2::SourceReader;
	use std::error::Error as StdError;
	use std::fs::File;
	use std::io::Cursor;
	use std::io::Error as IoError;
	use std::io::Read;
	use std::io::Seek;
	use std::io::SeekFrom;
	use std::io::Write;
	use std::time::Instant;
	use tempfile::tempfile;
	use tokio_util::sync::CancellationToken;

	type TestResult<T = ()> = std::result::Result<T, Box<dyn StdError>>;

	fn push_variable_u64(bytes: &mut Vec<u8>, mut value: u64) {
		let mut first = 0_u8;
		let mut mask = 0x80_u8;
		let mut extra_bytes = 0;
		while extra_bytes < 8 {
			if value < 1_u64 << (7 * (extra_bytes + 1)) {
				first |= (value >> (8 * extra_bytes)) as u8;
				break;
			}
			first |= mask;
			mask >>= 1;
			extra_bytes += 1;
		}
		bytes.push(first);
		for _ in 0..extra_bytes {
			bytes.push(value as u8);
			value >>= 8;
		}
	}

	fn encoded_header(method: &[u8], properties: &[u8], unpacked_size: u64) -> TestResult<Vec<u8>> {
		let mut header = vec![K_PACK_INFO];
		push_variable_u64(&mut header, 0);
		push_variable_u64(&mut header, 1);
		header.push(K_SIZE);
		push_variable_u64(&mut header, 1);
		header.push(K_END);
		header.extend([K_UNPACK_INFO, K_FOLDER]);
		push_variable_u64(&mut header, 1);
		header.push(0);
		push_variable_u64(&mut header, 1);
		header.push(0x20 | u8::try_from(method.len())?);
		header.extend(method);
		push_variable_u64(&mut header, u64::try_from(properties.len())?);
		header.extend(properties);
		header.push(K_CODERS_UNPACK_SIZE);
		push_variable_u64(&mut header, unpacked_size);
		header.extend([K_END, K_END]);
		Ok(header)
	}

	fn preflight_synthetic_encoded_header(header: &[u8]) -> TestResult<std::result::Result<(), ArchiveError>> {
		let next_header_start = 33;
		let header_length = u64::try_from(header.len())?;
		Ok(preflight_encoded_header(
			header,
			next_header_start + header_length,
			next_header_start,
			next_header_start + header_length,
			&CancellationToken::new(),
			Instant::now(),
		)
		.map(|_| ()))
	}

	fn synthetic_archive(packed: &[u8], next_header: &[u8]) -> TestResult<File> {
		let mut signature_header = [0_u8; 32];
		signature_header[..6].copy_from_slice(super::SEVEN_ZIP_SIGNATURE);
		signature_header[6..8].copy_from_slice(&[0, 4]);
		signature_header[12..20].copy_from_slice(&u64::try_from(packed.len())?.to_le_bytes());
		signature_header[20..28].copy_from_slice(&u64::try_from(next_header.len())?.to_le_bytes());
		signature_header[28..32].copy_from_slice(&crc32fast::hash(next_header).to_le_bytes());
		let start_header_crc = crc32fast::hash(&signature_header[12..]);
		signature_header[8..12].copy_from_slice(&start_header_crc.to_le_bytes());

		let mut file = tempfile()?;
		file.write_all(&signature_header)?;
		file.write_all(packed)?;
		file.write_all(next_header)?;
		file.seek(SeekFrom::Start(0))?;
		Ok(file)
	}

	fn oversized_files_header() -> TestResult<Vec<u8>> {
		let file_count = u64::try_from(MAX_ARCHIVE_MEMBERS)? + 1;
		let mut header = vec![K_HEADER, K_FILES_INFO];
		push_variable_u64(&mut header, file_count);
		header.push(K_EMPTY_STREAM);
		push_variable_u64(&mut header, file_count.div_ceil(8));
		header.resize(header.len() + usize::try_from(file_count.div_ceil(8))?, 0xff);
		header.extend([K_END, K_END]);
		header.resize(usize::try_from(file_count)? + 1, 0);
		Ok(header)
	}

	fn copy_encoded_header(packed_size: u64, unpacked_size: u64) -> Vec<u8> {
		let mut header = vec![K_ENCODED_HEADER, K_PACK_INFO];
		push_variable_u64(&mut header, 0);
		push_variable_u64(&mut header, 1);
		header.push(K_SIZE);
		push_variable_u64(&mut header, packed_size);
		header.extend([K_END, K_UNPACK_INFO, K_FOLDER]);
		push_variable_u64(&mut header, 1);
		header.push(0);
		push_variable_u64(&mut header, 1);
		header.extend([1, 0, K_CODERS_UNPACK_SIZE]);
		push_variable_u64(&mut header, unpacked_size);
		header.extend([K_END, K_END]);
		header
	}

	#[test]
	fn cancelled_large_raw_count_scan_stops_before_backend_parsing() -> TestResult {
		let mut header = vec![K_HEADER, K_MAIN_STREAMS_INFO, K_PACK_INFO];
		push_variable_u64(&mut header, 0);
		push_variable_u64(&mut header, u64::try_from(MAX_ARCHIVE_MEMBERS)?);
		header.push(K_SIZE);
		header.resize(header.len() + MAX_ARCHIVE_MEMBERS, 0);
		header.extend([K_END, K_END, K_END]);
		let cancellation = CancellationToken::new();
		cancellation.cancel();

		assert_eq!(
			validate_raw_header(&header, &cancellation, Instant::now()),
			Err(ArchiveError::Cancelled)
		);
		Ok(())
	}

	#[test]
	fn rejects_plain_header_member_count_before_backend_parsing() -> TestResult {
		let file = synthetic_archive(&[], &oversized_files_header()?)?;

		let error = open_controlled(file, &CancellationToken::new(), Instant::now())
			.err()
			.ok_or_else(|| IoError::other("oversized plain header was accepted"))?;
		assert_eq!(*error.current_context(), ArchiveError::ExpansionLimit);
		Ok(())
	}

	#[test]
	fn rejects_encoded_padded_header_member_count_before_backend_parsing() -> TestResult {
		let decoded_header = oversized_files_header()?;
		let decoded_size = u64::try_from(decoded_header.len())?;
		let next_header = copy_encoded_header(decoded_size, decoded_size);
		let file = synthetic_archive(&decoded_header, &next_header)?;

		let error = open_controlled(file, &CancellationToken::new(), Instant::now())
			.err()
			.ok_or_else(|| IoError::other("oversized encoded header was accepted"))?;
		assert_eq!(*error.current_context(), ArchiveError::ExpansionLimit);
		Ok(())
	}

	#[test]
	fn rejects_aggregate_block_dictionaries_before_payload_decode() -> TestResult {
		let dictionary = u32::try_from(MAX_DICTIONARY_BYTES)?;
		let mut header = vec![K_HEADER, K_MAIN_STREAMS_INFO, K_PACK_INFO];
		push_variable_u64(&mut header, 0);
		push_variable_u64(&mut header, 1);
		header.extend([K_SIZE, 1, K_END, K_UNPACK_INFO, K_FOLDER, 1, 0, 2]);
		for _ in 0..2 {
			header.push(0x20 | u8::try_from(SEVEN_ZIP_LZMA_METHOD.len())?);
			header.extend(SEVEN_ZIP_LZMA_METHOD);
			header.extend([5, 0x5d]);
			header.extend(dictionary.to_le_bytes());
		}
		header.extend([
			1,
			0,
			K_CODERS_UNPACK_SIZE,
			1,
			1,
			K_END,
			super::K_SUB_STREAMS_INFO,
			K_END,
			K_END,
			K_END,
		]);
		let file = synthetic_archive(b"x", &header)?;

		let error = open_controlled(file, &CancellationToken::new(), Instant::now())
			.err()
			.ok_or_else(|| IoError::other("aggregate dictionaries were accepted"))?;
		assert_eq!(*error.current_context(), ArchiveError::DictionaryLimit);
		Ok(())
	}

	#[test]
	fn indexes_an_ordinary_archive_with_a_plain_header() -> TestResult {
		let file = tempfile()?;
		let mut writer = ArchiveWriter::new(file)?;
		writer.set_encrypt_header(false);
		writer.push_archive_entry(
			ArchiveEntry::new_file("a"),
			Some(Cursor::new(b"ordinary content".to_vec())),
		)?;
		let mut file = writer.finish()?;

		let mut signature_header = [0_u8; 32];
		file.seek(SeekFrom::Start(0))?;
		file.read_exact(&mut signature_header)?;
		let next_header_offset = u64::from_le_bytes(signature_header[12..20].try_into()?);
		file.seek(SeekFrom::Start(32 + next_header_offset))?;
		let mut marker = [0_u8; 1];
		file.read_exact(&mut marker)?;
		assert_eq!(marker[0], K_HEADER);

		file.seek(SeekFrom::Start(0))?;
		let Ok(members) = index(file, &CancellationToken::new(), Instant::now()) else {
			return Err(IoError::other("ordinary plain-header fixture was rejected").into());
		};
		assert_eq!(members.len(), 1);
		assert_eq!(members[0].path.as_str(), "a");
		Ok(())
	}

	#[test]
	fn indexes_all_members_of_a_solid_compression_block() -> TestResult {
		let file = tempfile()?;
		let mut writer = ArchiveWriter::new(file)?;
		writer.set_encrypt_header(false);
		writer.push_archive_entries(
			vec![
				ArchiveEntry::new_file("Data/first.txt"),
				ArchiveEntry::new_file("Data/second.txt"),
			],
			vec![
				SourceReader::new(Cursor::new(b"first file".to_vec())),
				SourceReader::new(Cursor::new(b"second file".to_vec())),
			],
		)?;
		let mut file = writer.finish()?;

		let Ok(parsed) = open_controlled(file.try_clone()?, &CancellationToken::new(), Instant::now()) else {
			return Err(IoError::other("solid 7z fixture was rejected while parsing").into());
		};
		assert_eq!(parsed.archive().blocks.len(), 1);
		assert_eq!(parsed.archive().files.len(), 2);
		assert_eq!(parsed.archive().files[1].compressed_size, 0);
		drop(parsed);

		file.seek(SeekFrom::Start(0))?;
		let Ok(members) = index(file, &CancellationToken::new(), Instant::now()) else {
			return Err(IoError::other("solid 7z fixture was rejected while indexing").into());
		};
		assert_eq!(members.len(), 2);
		assert_eq!(members[0].path.as_str(), "Data/first.txt");
		assert_eq!(members[1].path.as_str(), "Data/second.txt");
		Ok(())
	}

	#[test]
	fn indexes_an_ordinary_solid_archive_with_an_encoded_header() -> TestResult {
		let file = tempfile()?;
		let mut writer = ArchiveWriter::new(file)?;
		let entries = (0..64)
			.map(|index| ArchiveEntry::new_file(&format!("Data/shared-prefix/member-{index:03}.txt")))
			.collect::<Vec<_>>();
		let sources = (0..entries.len())
			.map(|index| SourceReader::new(Cursor::new(format!("solid member {index:03}").into_bytes())))
			.collect::<Vec<_>>();
		writer.push_archive_entries(entries, sources)?;
		let mut file = writer.finish()?;

		let mut signature_header = [0_u8; 32];
		file.seek(SeekFrom::Start(0))?;
		file.read_exact(&mut signature_header)?;
		let next_header_offset = u64::from_le_bytes(signature_header[12..20].try_into()?);
		file.seek(SeekFrom::Start(32 + next_header_offset))?;
		let mut marker = [0_u8; 1];
		file.read_exact(&mut marker)?;
		assert_eq!(marker[0], super::K_ENCODED_HEADER);

		file.seek(SeekFrom::Start(0))?;
		let Ok(members) = index(file, &CancellationToken::new(), Instant::now()) else {
			return Err(IoError::other("ordinary encoded-header 7z fixture was rejected").into());
		};
		assert_eq!(members.len(), 64);
		assert_eq!(members[0].path.as_str(), "Data/shared-prefix/member-000.txt");
		assert_eq!(members[63].path.as_str(), "Data/shared-prefix/member-063.txt");
		Ok(())
	}

	#[test]
	fn rejects_encoded_header_dictionary_before_backend_parsing() -> TestResult {
		let header = encoded_header(SEVEN_ZIP_LZMA_METHOD, &[0x5d, 0, 0, 0, 0x10], 256 * 1024 * 1024)?;

		assert_eq!(
			preflight_synthetic_encoded_header(&header)?,
			Err(ArchiveError::DictionaryLimit)
		);
		Ok(())
	}

	#[test]
	fn rejects_encoded_header_unpack_size_before_backend_parsing() -> TestResult {
		let header = encoded_header(
			SEVEN_ZIP_LZMA_METHOD,
			&[0x5d, 0, 0, 0x10, 0],
			MAX_ARCHIVE_METADATA_BYTES + 1,
		)?;

		assert_eq!(
			preflight_synthetic_encoded_header(&header)?,
			Err(ArchiveError::ExpansionLimit)
		);
		Ok(())
	}

	#[test]
	fn rejects_encoded_header_packed_size_before_backend_parsing() -> TestResult {
		let mut header = vec![K_PACK_INFO];
		push_variable_u64(&mut header, 0);
		push_variable_u64(&mut header, 1);
		header.push(K_SIZE);
		push_variable_u64(&mut header, MAX_ARCHIVE_METADATA_BYTES + 1);

		assert_eq!(
			preflight_synthetic_encoded_header(&header)?,
			Err(ArchiveError::ExpansionLimit)
		);
		Ok(())
	}

	#[test]
	fn rejects_encoded_header_coder_count_before_backend_parsing() -> TestResult {
		let mut header = vec![K_PACK_INFO];
		push_variable_u64(&mut header, 0);
		push_variable_u64(&mut header, 1);
		header.push(K_SIZE);
		push_variable_u64(&mut header, 1);
		header.extend([K_END, K_UNPACK_INFO, K_FOLDER]);
		push_variable_u64(&mut header, 1);
		header.push(0);
		push_variable_u64(&mut header, u64::try_from(MAX_ARCHIVE_MEMBERS)? + 1);

		assert_eq!(
			preflight_synthetic_encoded_header(&header)?,
			Err(ArchiveError::ExpansionLimit)
		);
		Ok(())
	}

	#[test]
	fn rejects_encrypted_encoded_header_before_backend_parsing() -> TestResult {
		let header = encoded_header(SEVEN_ZIP_AES_METHOD, &[], 1)?;

		assert_eq!(
			preflight_synthetic_encoded_header(&header)?,
			Err(ArchiveError::Encrypted)
		);
		Ok(())
	}

	#[test]
	fn rejects_unsafe_compression_block_metadata() -> TestResult {
		let cancellation = CancellationToken::new();
		let started = Instant::now();

		let zero_packed = validate_block_compression_ratio(1, &[], &cancellation, started);
		assert_eq!(
			zero_packed.err().map(|error| *error.current_context()),
			Some(ArchiveError::ExpansionLimit)
		);

		let excessive =
			validate_block_compression_ratio(MAX_COMPRESSION_RATIO + 1, &[1], &cancellation, started);
		assert_eq!(
			excessive.err().map(|error| *error.current_context()),
			Some(ArchiveError::ExpansionLimit)
		);

		let packed_sum_overflow = validate_block_compression_ratio(1, &[u64::MAX, 1], &cancellation, started);
		assert_eq!(
			packed_sum_overflow.err().map(|error| *error.current_context()),
			Some(ArchiveError::ExpansionLimit)
		);

		let ratio_limit_overflow = validate_block_compression_ratio(1, &[u64::MAX], &cancellation, started);
		assert_eq!(
			ratio_limit_overflow.err().map(|error| *error.current_context()),
			Some(ArchiveError::ExpansionLimit)
		);
		Ok(())
	}
}
