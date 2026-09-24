use crate::entry::validate_entry_kind;
use crate::error::ArchiveError;
use crate::index::ArchiveMember;
use crate::index::ArchiveMemberCollector;
use crate::index::MemberKind;
use crate::limits::COPY_BUFFER_BYTES;
use crate::limits::MAX_ARCHIVE_BYTES;
use crate::limits::MAX_ARCHIVE_MEMBERS;
use crate::limits::MAX_ARCHIVE_METADATA_BYTES;
use crate::limits::MAX_ARCHIVE_WORK;
use crate::limits::MAX_DICTIONARY_BYTES;
use crate::path::SafeArchivePath;
use crate::source::SourceFile;
use rars::Archive;
use rars::ArchiveReadOptions;
use rars::ArchiveReader;
use rars::Error as RarError;
use rootcause::Report;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use rootcause::report;
use sha2::Digest;
use sha2::Sha256;
use std::fs::File;
use std::fs::OpenOptions;
#[cfg(unix)]
use std::fs::Permissions;
#[cfg(windows)]
use std::fs::remove_dir_all;
#[cfg(unix)]
use std::fs::set_permissions;
#[cfg(windows)]
use std::io::Error as IoError;
#[cfg(windows)]
use std::io::ErrorKind;
use std::io::Read;
#[cfg(windows)]
use std::io::Result as IoResult;
use std::io::Seek;
use std::io::SeekFrom;
use std::io::Write;
#[cfg(windows)]
use std::mem::size_of;
use std::ops::Deref;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;
#[cfg(windows)]
use std::os::windows::fs::OpenOptionsExt;
#[cfg(windows)]
use std::os::windows::io::AsRawHandle;
#[cfg(windows)]
use std::os::windows::io::FromRawHandle;
use std::path::Path;
#[cfg(windows)]
use std::path::PathBuf;
#[cfg(windows)]
use std::ptr::from_ref;
#[cfg(windows)]
use std::ptr::null_mut;
#[cfg(windows)]
use std::slice::from_raw_parts;
use std::str::from_utf8;
use std::time::Instant;
use tempfile::Builder;
#[cfg(unix)]
use tempfile::TempDir;
use tokio_util::sync::CancellationToken;
#[cfg(windows)]
use windows::Win32::Foundation::ERROR_SUCCESS;
#[cfg(windows)]
use windows::Win32::Foundation::HANDLE;
#[cfg(windows)]
use windows::Win32::Foundation::HLOCAL;
#[cfg(windows)]
use windows::Win32::Foundation::LocalFree;
#[cfg(windows)]
use windows::Win32::Foundation::WIN32_ERROR;
#[cfg(windows)]
use windows::Win32::Security::ACL;
#[cfg(windows)]
use windows::Win32::Security::Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW;
#[cfg(windows)]
use windows::Win32::Security::Authorization::GetSecurityInfo;
#[cfg(windows)]
use windows::Win32::Security::Authorization::SDDL_REVISION_1;
#[cfg(windows)]
use windows::Win32::Security::Authorization::SE_FILE_OBJECT;
#[cfg(windows)]
use windows::Win32::Security::DACL_SECURITY_INFORMATION;
#[cfg(windows)]
use windows::Win32::Security::GetSecurityDescriptorControl;
#[cfg(windows)]
use windows::Win32::Security::GetSecurityDescriptorDacl;
#[cfg(windows)]
use windows::Win32::Security::PSECURITY_DESCRIPTOR;
#[cfg(windows)]
use windows::Win32::Security::SE_DACL_PROTECTED;
#[cfg(windows)]
use windows::Win32::Security::SECURITY_ATTRIBUTES;
#[cfg(windows)]
use windows::Win32::Storage::FileSystem::CREATE_NEW;
#[cfg(windows)]
use windows::Win32::Storage::FileSystem::CreateDirectoryW;
#[cfg(windows)]
use windows::Win32::Storage::FileSystem::CreateFileW;
#[cfg(windows)]
use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_NORMAL;
#[cfg(windows)]
use windows::Win32::Storage::FileSystem::FILE_CREATION_DISPOSITION;
#[cfg(windows)]
use windows::Win32::Storage::FileSystem::FILE_FLAG_BACKUP_SEMANTICS;
#[cfg(windows)]
use windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES;
#[cfg(windows)]
use windows::Win32::Storage::FileSystem::FILE_GENERIC_READ;
#[cfg(windows)]
use windows::Win32::Storage::FileSystem::FILE_GENERIC_WRITE;
#[cfg(windows)]
use windows::Win32::Storage::FileSystem::FILE_SHARE_MODE;
#[cfg(windows)]
use windows::Win32::Storage::FileSystem::FILE_SHARE_READ;
#[cfg(windows)]
use windows::Win32::Storage::FileSystem::OPEN_EXISTING;
#[cfg(windows)]
use windows::Win32::Storage::FileSystem::READ_CONTROL;
#[cfg(windows)]
use windows::core::BOOL;
#[cfg(windows)]
use windows::core::Error as WindowsError;
#[cfg(windows)]
use windows::core::PCWSTR;
#[cfg(windows)]
use windows::core::w;

const HOST_OS_UNIX: u64 = 3;

pub(crate) fn index(
	source: &SourceFile,
	sha256: [u8; 32],
	cancellation: &CancellationToken,
	started: Instant,
) -> Result<Vec<ArchiveMember>, ArchiveError> {
	let archive = open(source, sha256, cancellation, started)?;
	let mut members = ArchiveMemberCollector::with_declared_count(archive.members().count())?;
	validate_archive(&archive, cancellation, started)?;
	for (ordinal, member) in archive.members().enumerate() {
		if cancellation.is_cancelled() {
			return Err(report!(ArchiveError::Cancelled));
		}
		if started.elapsed() > MAX_ARCHIVE_WORK {
			return Err(report!(ArchiveError::WorkLimit));
		}
		let metadata = member.meta;
		if metadata.is_encrypted {
			return Err(report!(ArchiveError::Encrypted));
		}
		if metadata.is_split_before || metadata.is_split_after {
			return Err(report!(ArchiveError::SplitArchive));
		}
		let unix_mode = (metadata.host_os == Some(HOST_OS_UNIX)).then_some(metadata.file_attr);
		let windows_attributes = (metadata.host_os != Some(HOST_OS_UNIX)).then_some(metadata.file_attr);
		validate_entry_kind(metadata.is_directory, windows_attributes, unix_mode)?;
		let name = from_utf8(&metadata.name).context(ArchiveError::NonLosslessName)?;
		let normalized_name = if metadata.is_directory {
			name.trim_end_matches(['/', '\\'])
		} else {
			name
		};
		members.push(
			ArchiveMember {
				ordinal,
				path: SafeArchivePath::new(normalized_name)?,
				kind: if metadata.is_directory {
					MemberKind::Directory
				} else {
					MemberKind::File
				},
				uncompressed_size: metadata.unpacked_size,
				compressed_size: metadata.packed_size,
			},
			cancellation,
			started,
		)?;
	}
	Ok(members.into_members())
}

pub(crate) struct RarArchive {
	archive: Archive,
	// rars retains this path and reopens it while extracting, so the private directory and file handle
	// must outlive every archive method call.
	_snapshot: RarSnapshot,
}

impl Deref for RarArchive {
	type Target = Archive;

	fn deref(&self) -> &Self::Target {
		&self.archive
	}
}

struct RarSnapshot {
	file: File,
	directory: RarTempDir,
}

#[cfg(unix)]
type RarTempDir = TempDir;

#[cfg(windows)]
struct RarTempDir {
	path: PathBuf,
	handle: Option<File>,
}

#[cfg(windows)]
impl RarTempDir {
	fn path(&self) -> &Path {
		&self.path
	}
}

#[cfg(windows)]
impl Drop for RarTempDir {
	fn drop(&mut self) {
		drop(self.handle.take());
		let _ = remove_dir_all(&self.path);
	}
}

pub(crate) fn open(
	source: &SourceFile,
	sha256: [u8; 32],
	cancellation: &CancellationToken,
	started: Instant,
) -> Result<RarArchive, ArchiveError> {
	// rars 0.9.4 has no File or Read + Seek constructor. A private bounded disk snapshot avoids both
	// its untrusted-path reopen and the only native alternative, retaining the whole archive in memory.
	let mut snapshot = create_snapshot(source, sha256, cancellation, started)?;
	preflight_snapshot(&mut snapshot.file, cancellation, started)?;
	let archive = ArchiveReader::read_path_with_options(
		snapshot.directory.path().join("archive.rar"),
		ArchiveReadOptions::new().with_rar50_buffered_decode_limit(MAX_DICTIONARY_BYTES),
	)
	.map_err(map_error)?;
	Ok(RarArchive {
		archive,
		_snapshot: snapshot,
	})
}

const RAR13_SIGNATURE: &[u8] = b"RE~^";
const RAR15_SIGNATURE: &[u8] = b"Rar!\x1a\x07\x00";
const RAR50_SIGNATURE: &[u8] = b"Rar!\x1a\x07\x01\x00";

fn preflight_snapshot(file: &mut File, cancellation: &CancellationToken, started: Instant) -> Result<(), ArchiveError> {
	let file_len = file.metadata().context(ArchiveError::Io)?.len();
	let prefix_len = file_len.min(RAR50_SIGNATURE.len() as u64) as usize;
	let mut prefix = [0; RAR50_SIGNATURE.len()];
	if cancellation.is_cancelled() {
		return Err(report!(ArchiveError::Cancelled));
	}
	if started.elapsed() > MAX_ARCHIVE_WORK {
		return Err(report!(ArchiveError::WorkLimit));
	}
	file.seek(SeekFrom::Start(0)).context(ArchiveError::Io)?;
	file.read_exact(&mut prefix[..prefix_len]).context(ArchiveError::Io)?;

	if prefix.starts_with(RAR50_SIGNATURE) {
		return preflight_rar50(file, file_len, cancellation, started);
	}
	if prefix.starts_with(RAR15_SIGNATURE) {
		return preflight_rar15(file, file_len, cancellation, started);
	}
	// RAR 1.3 has a different header model and rars retains its metadata too. It is obsolete and uncommon,
	// so fail closed instead of expanding this safety walker into a third parser.
	if prefix.starts_with(RAR13_SIGNATURE) {
		return Err(report!(ArchiveError::UnsupportedFormat));
	}
	Err(report!(ArchiveError::InvalidArchive))
}

fn preflight_rar15(
	file: &mut File,
	file_len: u64,
	cancellation: &CancellationToken,
	started: Instant,
) -> Result<(), ArchiveError> {
	const MAIN_HEAD: u8 = 0x73;
	const FILE_HEAD: u8 = 0x74;
	const NEWSUB_HEAD: u8 = 0x7a;
	const ENDARC_HEAD: u8 = 0x7b;
	const LONG_BLOCK: u16 = 0x8000;
	const MHD_VOLUME: u16 = 0x0001;
	const MHD_PASSWORD: u16 = 0x0080;
	const MHD_ENCRYPTVER: u16 = 0x0200;
	const FHD_SPLIT: u16 = 0x0003;
	const FHD_PASSWORD: u16 = 0x0004;
	const FHD_LARGE: u16 = 0x0100;
	const FHD_UNICODE: u16 = 0x0200;
	const FHD_SALT: u16 = 0x0400;

	let mut position = RAR15_SIGNATURE.len() as u64;
	let mut metadata_bytes = RAR15_SIGNATURE.len() as u64;
	let mut header_count = 0_usize;
	let mut member_count = 0_usize;
	let mut saw_main = false;

	while position < file_len {
		if cancellation.is_cancelled() {
			return Err(report!(ArchiveError::Cancelled));
		}
		if started.elapsed() > MAX_ARCHIVE_WORK {
			return Err(report!(ArchiveError::WorkLimit));
		}
		let base_end = position
			.checked_add(7)
			.ok_or_else(|| report!(ArchiveError::InvalidArchive))?;
		if base_end > file_len {
			return Err(report!(ArchiveError::InvalidArchive));
		}

		let mut prefix = [0; 40];
		file.seek(SeekFrom::Start(position)).context(ArchiveError::Io)?;
		file.read_exact(&mut prefix[..7]).context(ArchiveError::Io)?;
		let head_type = prefix[2];
		let flags = u16::from_le_bytes([prefix[3], prefix[4]]);
		let head_size = u64::from(u16::from_le_bytes([prefix[5], prefix[6]]));
		if head_size < 7 {
			return Err(report!(ArchiveError::InvalidArchive));
		}
		let header_end = position
			.checked_add(head_size)
			.ok_or_else(|| report!(ArchiveError::InvalidArchive))?;
		if header_end > file_len {
			return Err(report!(ArchiveError::InvalidArchive));
		}
		metadata_bytes = metadata_bytes
			.checked_add(head_size)
			.ok_or_else(|| report!(ArchiveError::ExpansionLimit))?;
		if metadata_bytes > MAX_ARCHIVE_METADATA_BYTES {
			return Err(report!(ArchiveError::ExpansionLimit));
		}
		header_count = header_count
			.checked_add(1)
			.ok_or_else(|| report!(ArchiveError::ExpansionLimit))?;
		if header_count > MAX_ARCHIVE_MEMBERS.saturating_mul(2) {
			return Err(report!(ArchiveError::ExpansionLimit));
		}

		let prefix_len = usize::try_from(head_size.min(prefix.len() as u64))
			.map_err(|_| report!(ArchiveError::InvalidArchive))?;
		if prefix_len > 7 {
			if cancellation.is_cancelled() {
				return Err(report!(ArchiveError::Cancelled));
			}
			if started.elapsed() > MAX_ARCHIVE_WORK {
				return Err(report!(ArchiveError::WorkLimit));
			}
			file.read_exact(&mut prefix[7..prefix_len]).context(ArchiveError::Io)?;
		}

		if !saw_main {
			if head_type != MAIN_HEAD
				|| flags & LONG_BLOCK != 0 || head_size < 13
				|| (flags & MHD_ENCRYPTVER != 0 && head_size < 14)
			{
				return Err(report!(ArchiveError::InvalidArchive));
			}
			if flags & MHD_VOLUME != 0 {
				return Err(report!(ArchiveError::SplitArchive));
			}
			if flags & MHD_PASSWORD != 0 {
				return Err(report!(ArchiveError::Encrypted));
			}
			saw_main = true;
		} else if head_type == FILE_HEAD || head_type == NEWSUB_HEAD {
			let fixed_size = if flags & FHD_LARGE != 0 { 40_u64 } else { 32_u64 };
			if flags & LONG_BLOCK == 0 || head_size < fixed_size {
				return Err(report!(ArchiveError::InvalidArchive));
			}
			if flags & FHD_PASSWORD != 0 {
				return Err(report!(ArchiveError::Encrypted));
			}
			if flags & FHD_SPLIT != 0 {
				return Err(report!(ArchiveError::SplitArchive));
			}
			// rars 0.9.4 cannot cap the legacy decoder's dictionary. RAR 2.9 PPMd can replace the
			// header dictionary with a payload-controlled value up to 256 MiB, so the header bits do
			// not prove the project's 128 MiB limit. Reject compressed legacy payloads before decode.
			if prefix[25] != 0x30 {
				return Err(report!(ArchiveError::UnsupportedFormat));
			}

			let name_size = u64::from(u16::from_le_bytes([prefix[26], prefix[27]]));
			let required_size = fixed_size
				.checked_add(name_size)
				.and_then(|size| size.checked_add(if flags & FHD_SALT != 0 { 8 } else { 0 }))
				.ok_or_else(|| report!(ArchiveError::InvalidArchive))?;
			if required_size > head_size {
				return Err(report!(ArchiveError::InvalidArchive));
			}
			let decoded_name_bytes = if flags & FHD_UNICODE != 0 {
				if cancellation.is_cancelled() {
					return Err(report!(ArchiveError::Cancelled));
				}
				if started.elapsed() > MAX_ARCHIVE_WORK {
					return Err(report!(ArchiveError::WorkLimit));
				}
				let name_start = position
					.checked_add(fixed_size)
					.ok_or_else(|| report!(ArchiveError::InvalidArchive))?;
				file.seek(SeekFrom::Start(name_start)).context(ArchiveError::Io)?;
				let mut name = vec![0; name_size as usize];
				file.read_exact(&mut name).context(ArchiveError::Io)?;
				rar15_decoded_name_bytes_upper_bound(&name)?
			} else {
				name_size
			};
			metadata_bytes = metadata_bytes
				.checked_add(decoded_name_bytes)
				.ok_or_else(|| report!(ArchiveError::ExpansionLimit))?;
			if metadata_bytes > MAX_ARCHIVE_METADATA_BYTES {
				return Err(report!(ArchiveError::ExpansionLimit));
			}
			if head_type == FILE_HEAD {
				member_count = member_count
					.checked_add(1)
					.ok_or_else(|| report!(ArchiveError::ExpansionLimit))?;
				if member_count > MAX_ARCHIVE_MEMBERS {
					return Err(report!(ArchiveError::ExpansionLimit));
				}
			}
		} else if head_type == ENDARC_HEAD {
			if flags & 0x0001 != 0 {
				return Err(report!(ArchiveError::SplitArchive));
			}
			break;
		}

		let low_data_size = if flags & LONG_BLOCK != 0 {
			if head_size < 11 {
				return Err(report!(ArchiveError::InvalidArchive));
			}
			u64::from(u32::from_le_bytes([prefix[7], prefix[8], prefix[9], prefix[10]]))
		} else {
			0
		};
		let data_size = if (head_type == FILE_HEAD || head_type == NEWSUB_HEAD) && flags & FHD_LARGE != 0 {
			let high = u64::from(u32::from_le_bytes([prefix[32], prefix[33], prefix[34], prefix[35]]));
			(high << 32) | low_data_size
		} else {
			low_data_size
		};
		position = header_end
			.checked_add(data_size)
			.ok_or_else(|| report!(ArchiveError::InvalidArchive))?;
		if position > file_len {
			return Err(report!(ArchiveError::InvalidArchive));
		}
	}

	if !saw_main {
		return Err(report!(ArchiveError::InvalidArchive));
	}
	Ok(())
}

fn rar15_decoded_name_bytes_upper_bound(raw: &[u8]) -> Result<u64, ArchiveError> {
	let Some(zero_position) = raw.iter().position(|byte| *byte == 0) else {
		return Ok(raw.len() as u64);
	};
	if zero_position + 1 >= raw.len() {
		return Ok(zero_position as u64);
	}

	let encoded = &raw[zero_position + 2..];
	let mut position = 0_usize;
	let mut flag_byte = 0_u8;
	let mut flag_bits = 0_u8;
	let mut decoded_units = 0_u64;
	while position < encoded.len() {
		if flag_bits == 0 {
			let Some(&next_flags) = encoded.get(position) else {
				return Ok(raw.len() as u64);
			};
			flag_byte = next_flags;
			flag_bits = 8;
			position += 1;
		}
		let mode = flag_byte >> 6;
		flag_byte <<= 2;
		flag_bits -= 2;

		let added_units = if mode == 2 {
			if encoded.get(position..position + 2).is_none() {
				return Ok(raw.len() as u64);
			}
			position += 2;
			1
		} else {
			let Some(&byte) = encoded.get(position) else {
				return Ok(raw.len() as u64);
			};
			position += 1;
			if mode != 3 {
				1
			} else if byte & 0x80 != 0 {
				if encoded.get(position).is_none() {
					return Ok(raw.len() as u64);
				}
				position += 1;
				u64::from(byte & 0x7f) + 2
			} else {
				u64::from(byte) + 2
			}
		};
		decoded_units = decoded_units
			.checked_add(added_units)
			.ok_or_else(|| report!(ArchiveError::ExpansionLimit))?;
	}

	decoded_units
		.checked_mul(3)
		.ok_or_else(|| report!(ArchiveError::ExpansionLimit))
}

struct Rar50HeaderReader<'a> {
	file: &'a mut File,
	position: u64,
	end: u64,
	cancellation: &'a CancellationToken,
	started: Instant,
}

impl Rar50HeaderReader<'_> {
	fn read_exact<const N: usize>(&mut self) -> Result<[u8; N], ArchiveError> {
		if self.cancellation.is_cancelled() {
			return Err(report!(ArchiveError::Cancelled));
		}
		if self.started.elapsed() > MAX_ARCHIVE_WORK {
			return Err(report!(ArchiveError::WorkLimit));
		}
		let next = self
			.position
			.checked_add(N as u64)
			.ok_or_else(|| report!(ArchiveError::InvalidArchive))?;
		if next > self.end {
			return Err(report!(ArchiveError::InvalidArchive));
		}
		let mut bytes = [0; N];
		self.file.read_exact(&mut bytes).context(ArchiveError::Io)?;
		self.position = next;
		Ok(bytes)
	}

	fn read_vint(&mut self) -> Result<u64, ArchiveError> {
		let mut value = 0_u64;
		let mut shift = 0_u32;
		for _ in 0..10 {
			let byte = self.read_exact::<1>()?[0];
			if shift == 63 && byte & 0x7e != 0 {
				return Err(report!(ArchiveError::InvalidArchive));
			}
			value = value
				.checked_add(u64::from(byte & 0x7f) << shift)
				.ok_or_else(|| report!(ArchiveError::InvalidArchive))?;
			if byte & 0x80 == 0 {
				return Ok(value);
			}
			shift += 7;
		}
		Err(report!(ArchiveError::InvalidArchive))
	}

	fn skip_to(&mut self, position: u64) -> Result<(), ArchiveError> {
		if self.cancellation.is_cancelled() {
			return Err(report!(ArchiveError::Cancelled));
		}
		if self.started.elapsed() > MAX_ARCHIVE_WORK {
			return Err(report!(ArchiveError::WorkLimit));
		}
		if position < self.position || position > self.end {
			return Err(report!(ArchiveError::InvalidArchive));
		}
		self.file.seek(SeekFrom::Start(position)).context(ArchiveError::Io)?;
		self.position = position;
		Ok(())
	}
}

fn preflight_rar50(
	file: &mut File,
	file_len: u64,
	cancellation: &CancellationToken,
	started: Instant,
) -> Result<(), ArchiveError> {
	const HEAD_MAIN: u64 = 1;
	const HEAD_FILE: u64 = 2;
	const HEAD_SERVICE: u64 = 3;
	const HEAD_CRYPT: u64 = 4;
	const HEAD_END: u64 = 5;
	const HFL_EXTRA: u64 = 0x0001;
	const HFL_DATA: u64 = 0x0002;
	const HFL_SPLIT: u64 = 0x0018;
	const MHFL_VOLUME: u64 = 0x0001;
	const FHFL_MTIME: u64 = 0x0002;
	const FHFL_CRC32: u64 = 0x0004;

	let mut position = RAR50_SIGNATURE.len() as u64;
	let mut metadata_bytes = RAR50_SIGNATURE.len() as u64;
	let mut header_count = 0_usize;
	let mut member_count = 0_usize;
	let mut saw_main = false;

	while position < file_len {
		if cancellation.is_cancelled() {
			return Err(report!(ArchiveError::Cancelled));
		}
		if started.elapsed() > MAX_ARCHIVE_WORK {
			return Err(report!(ArchiveError::WorkLimit));
		}
		file.seek(SeekFrom::Start(position)).context(ArchiveError::Io)?;
		let mut reader = Rar50HeaderReader {
			file,
			position,
			end: file_len,
			cancellation,
			started,
		};
		reader.read_exact::<4>()?;
		let size_start = reader.position;
		let header_size = reader.read_vint()?;
		let header_size_length = reader.position - size_start;
		if header_size_length > 3 {
			return Err(report!(ArchiveError::InvalidArchive));
		}
		let header_end = reader
			.position
			.checked_add(header_size)
			.ok_or_else(|| report!(ArchiveError::InvalidArchive))?;
		if header_end > file_len {
			return Err(report!(ArchiveError::InvalidArchive));
		}
		let disk_header_size = 4_u64
			.checked_add(header_size_length)
			.and_then(|size| size.checked_add(header_size))
			.ok_or_else(|| report!(ArchiveError::InvalidArchive))?;
		metadata_bytes = metadata_bytes
			.checked_add(disk_header_size)
			.ok_or_else(|| report!(ArchiveError::ExpansionLimit))?;
		if metadata_bytes > MAX_ARCHIVE_METADATA_BYTES {
			return Err(report!(ArchiveError::ExpansionLimit));
		}
		header_count = header_count
			.checked_add(1)
			.ok_or_else(|| report!(ArchiveError::ExpansionLimit))?;
		if header_count > MAX_ARCHIVE_MEMBERS.saturating_mul(2) {
			return Err(report!(ArchiveError::ExpansionLimit));
		}

		reader.end = header_end;
		let header_type = reader.read_vint()?;
		let flags = reader.read_vint()?;
		let is_split = flags & HFL_SPLIT != 0;
		let extra_size = if flags & HFL_EXTRA != 0 { reader.read_vint()? } else { 0 };
		let data_size = if flags & HFL_DATA != 0 { reader.read_vint()? } else { 0 };
		let extra_start = header_end
			.checked_sub(extra_size)
			.ok_or_else(|| report!(ArchiveError::InvalidArchive))?;
		if reader.position > extra_start {
			return Err(report!(ArchiveError::InvalidArchive));
		}
		reader.end = extra_start;

		if !saw_main {
			if header_type == HEAD_CRYPT {
				return Err(report!(ArchiveError::Encrypted));
			}
			if header_type != HEAD_MAIN {
				return Err(report!(ArchiveError::InvalidArchive));
			}
			let archive_flags = reader.read_vint()?;
			if archive_flags & MHFL_VOLUME != 0 {
				return Err(report!(ArchiveError::SplitArchive));
			}
			saw_main = true;
		} else if header_type == HEAD_CRYPT {
			return Err(report!(ArchiveError::Encrypted));
		} else if header_type == HEAD_FILE || header_type == HEAD_SERVICE {
			let file_flags = reader.read_vint()?;
			reader.read_vint()?;
			reader.read_vint()?;
			if file_flags & FHFL_MTIME != 0 {
				reader.read_exact::<4>()?;
			}
			if file_flags & FHFL_CRC32 != 0 {
				reader.read_exact::<4>()?;
			}
			reader.read_vint()?;
			reader.read_vint()?;
			let name_size = reader.read_vint()?;
			let name_end = reader
				.position
				.checked_add(name_size)
				.ok_or_else(|| report!(ArchiveError::InvalidArchive))?;
			if name_end > extra_start {
				return Err(report!(ArchiveError::InvalidArchive));
			}
			metadata_bytes = metadata_bytes
				.checked_add(name_size)
				.ok_or_else(|| report!(ArchiveError::ExpansionLimit))?;
			if metadata_bytes > MAX_ARCHIVE_METADATA_BYTES {
				return Err(report!(ArchiveError::ExpansionLimit));
			}
			reader.skip_to(name_end)?;
			if header_type == HEAD_FILE {
				member_count = member_count
					.checked_add(1)
					.ok_or_else(|| report!(ArchiveError::ExpansionLimit))?;
				if member_count > MAX_ARCHIVE_MEMBERS {
					return Err(report!(ArchiveError::ExpansionLimit));
				}
			}
		} else if header_type == HEAD_END && reader.position < extra_start && reader.read_vint()? & 0x0001 != 0
		{
			return Err(report!(ArchiveError::SplitArchive));
		}

		if (header_type == HEAD_FILE || header_type == HEAD_SERVICE) && extra_size != 0 {
			reader.end = header_end;
			reader.skip_to(extra_start)?;
			while reader.position < header_end {
				let record_size = reader.read_vint()?;
				let record_end = reader
					.position
					.checked_add(record_size)
					.ok_or_else(|| report!(ArchiveError::InvalidArchive))?;
				if record_end <= reader.position || record_end > header_end {
					return Err(report!(ArchiveError::InvalidArchive));
				}
				let record_type = reader.read_vint()?;
				if reader.position > record_end {
					return Err(report!(ArchiveError::InvalidArchive));
				}
				if record_type == 1 {
					return Err(report!(ArchiveError::Encrypted));
				}
				// WinRAR 5.21 and earlier wrote a service SUBDATA record one byte shorter than its
				// payload. rars folds the single dangling final byte into that record, so mirror only
				// that exact compatibility case while rejecting other malformed record ranges.
				if header_type == HEAD_SERVICE && record_type == 7 && header_end - record_end == 1 {
					reader.skip_to(header_end)?;
					continue;
				}
				reader.skip_to(record_end)?;
			}
		}
		if is_split {
			return Err(report!(ArchiveError::SplitArchive));
		}

		position = header_end
			.checked_add(data_size)
			.ok_or_else(|| report!(ArchiveError::InvalidArchive))?;
		if position > file_len {
			return Err(report!(ArchiveError::InvalidArchive));
		}
		if header_type == HEAD_END {
			break;
		}
	}

	if !saw_main {
		return Err(report!(ArchiveError::InvalidArchive));
	}
	Ok(())
}

fn create_snapshot(
	source: &SourceFile,
	sha256: [u8; 32],
	cancellation: &CancellationToken,
	started: Instant,
) -> Result<RarSnapshot, ArchiveError> {
	let directory = create_private_directory()?;

	let path = directory.path().join("archive.rar");
	let mut file = create_private_file(&path)?;
	let source_length = source.len()?;
	let mut input = source.duplicate()?;
	copy_source_to_snapshot(&mut input, &mut file, source_length, sha256, cancellation, started)?;

	// The snapshot is disposable. Hashing while copying proves source identity without a durability sync
	// or a second read. The private directory closes the Windows reopen gap before this writer is released.
	#[cfg(windows)]
	let file = {
		drop(file);
		OpenOptions::new()
			.read(true)
			.share_mode(FILE_SHARE_READ.0)
			.open(&path)
			.context(ArchiveError::Io)?
	};

	Ok(RarSnapshot { file, directory })
}

fn copy_source_to_snapshot(
	input: &mut dyn Read,
	output: &mut dyn Write,
	expected_length: u64,
	expected_sha256: [u8; 32],
	cancellation: &CancellationToken,
	started: Instant,
) -> Result<(), ArchiveError> {
	let mut hasher = Sha256::new();
	let mut buffer = vec![0; COPY_BUFFER_BYTES];
	let mut total = 0_u64;
	loop {
		if cancellation.is_cancelled() {
			return Err(report!(ArchiveError::Cancelled));
		}
		if started.elapsed() > MAX_ARCHIVE_WORK {
			return Err(report!(ArchiveError::WorkLimit));
		}
		let count = input.read(&mut buffer).context(ArchiveError::Io)?;
		if count == 0 {
			break;
		}
		total = total
			.checked_add(count as u64)
			.ok_or_else(|| report!(ArchiveError::ExpansionLimit))?;
		if total > expected_length || total > MAX_ARCHIVE_BYTES {
			return Err(report!(ArchiveError::IdentityChanged));
		}
		hasher.update(&buffer[..count]);
		output.write_all(&buffer[..count]).context(ArchiveError::Io)?;
	}
	if total != expected_length || <[u8; 32]>::from(hasher.finalize()) != expected_sha256 {
		return Err(report!(ArchiveError::IdentityChanged));
	}
	Ok(())
}

#[cfg(unix)]
fn create_private_directory() -> Result<RarTempDir, ArchiveError> {
	let directory = Builder::new().prefix("mods-rar-").tempdir().context(ArchiveError::Io)?;
	set_permissions(directory.path(), Permissions::from_mode(0o700)).context(ArchiveError::Io)?;
	Ok(directory)
}

#[cfg(windows)]
fn create_private_directory() -> Result<RarTempDir, ArchiveError> {
	let security = LocalSecurityDescriptor::private(true).context(ArchiveError::Io)?;
	let temporary = Builder::new()
		.prefix("mods-rar-")
		.make(|path| create_windows_directory(path, &security))
		.context(ArchiveError::Io)?;
	let (handle, mut path) = temporary.into_parts();
	path.disable_cleanup(true);
	let path = path.to_path_buf();

	Ok(RarTempDir {
		path,
		handle: Some(handle),
	})
}

#[cfg(windows)]
fn create_windows_directory(path: &Path, security: &LocalSecurityDescriptor) -> IoResult<File> {
	let wide_path = windows_path(path);
	let attributes = security.attributes();
	// SAFETY: `wide_path` is a NUL-terminated UTF-16 path that remains alive for the call. `attributes`
	// points to a live descriptor, which Windows copies while atomically creating a new directory.
	unsafe { CreateDirectoryW(PCWSTR(wide_path.as_ptr()), Some(&attributes)) }.map_err(windows_error)?;

	let result = open_windows_path(
		path,
		READ_CONTROL.0,
		FILE_SHARE_READ,
		OPEN_EXISTING,
		FILE_FLAG_BACKUP_SEMANTICS,
		None,
	)
	.and_then(|handle| {
		verify_private_dacl(HANDLE(handle.as_raw_handle()), security)?;
		Ok(handle)
	});
	if result.is_err() {
		let _ = remove_dir_all(path);
	}
	result
}

#[cfg(unix)]
fn create_private_file(path: &Path) -> Result<File, ArchiveError> {
	OpenOptions::new()
		.read(true)
		.write(true)
		.create_new(true)
		.mode(0o600)
		.open(path)
		.context(ArchiveError::Io)
}

#[cfg(windows)]
fn create_private_file(path: &Path) -> Result<File, ArchiveError> {
	let security = LocalSecurityDescriptor::private(false).context(ArchiveError::Io)?;
	let attributes = security.attributes();
	let file = open_windows_path(
		path,
		(FILE_GENERIC_READ | FILE_GENERIC_WRITE | READ_CONTROL).0,
		FILE_SHARE_READ,
		CREATE_NEW,
		FILE_ATTRIBUTE_NORMAL,
		Some(&attributes),
	)
	.context(ArchiveError::Io)?;
	verify_private_dacl(HANDLE(file.as_raw_handle()), &security).context(ArchiveError::Io)?;
	Ok(file)
}

#[cfg(windows)]
fn open_windows_path(
	path: &Path,
	desired_access: u32,
	share_mode: FILE_SHARE_MODE,
	creation_disposition: FILE_CREATION_DISPOSITION,
	flags: FILE_FLAGS_AND_ATTRIBUTES,
	security_attributes: Option<&SECURITY_ATTRIBUTES>,
) -> IoResult<File> {
	let wide_path = windows_path(path);
	// SAFETY: `wide_path` is a NUL-terminated UTF-16 path that remains alive for the call. Optional security
	// attributes remain live for the call, the other arguments are values, and success returns one owned handle.
	let handle = unsafe {
		CreateFileW(
			PCWSTR(wide_path.as_ptr()),
			desired_access,
			share_mode,
			security_attributes.map(from_ref),
			creation_disposition,
			flags,
			None,
		)
	}
	.map_err(windows_error)?;
	// SAFETY: `CreateFileW` returned a valid owned handle. Ownership moves to `File`, which closes it once.
	Ok(unsafe { File::from_raw_handle(handle.0) })
}

#[cfg(windows)]
fn windows_path(path: &Path) -> Vec<u16> {
	path.as_os_str().encode_wide().chain([0]).collect()
}

#[cfg(windows)]
fn verify_private_dacl(handle: HANDLE, expected: &LocalSecurityDescriptor) -> IoResult<()> {
	let mut actual = PSECURITY_DESCRIPTOR::default();
	// SAFETY: `handle` is valid for the call and `actual` is aligned writable storage. On success the API
	// returns a descriptor allocated with `LocalAlloc`; `LocalSecurityDescriptor` assumes and releases it.
	let status = unsafe {
		GetSecurityInfo(
			handle,
			SE_FILE_OBJECT,
			DACL_SECURITY_INFORMATION,
			None,
			None,
			None,
			None,
			Some(&mut actual),
		)
	};
	check_win32(status)?;
	let actual = LocalSecurityDescriptor(actual);

	if !actual.dacl_is_protected()? || actual.dacl_bytes()? != expected.dacl_bytes()? {
		return Err(IoError::new(
			ErrorKind::PermissionDenied,
			"Windows did not apply the protected private DACL",
		));
	}
	Ok(())
}

#[cfg(windows)]
struct LocalSecurityDescriptor(PSECURITY_DESCRIPTOR);

#[cfg(windows)]
impl LocalSecurityDescriptor {
	fn private(directory: bool) -> IoResult<Self> {
		// `OW` is the Windows Owner Rights SID. The protected DACL grants full access only to the object owner
		// and LocalSystem; inheritable directory entries give newly created snapshot files the same privacy.
		let sddl = if directory {
			w!("D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;OW)")
		} else {
			w!("D:P(A;;FA;;;SY)(A;;FA;;;OW)")
		};
		let mut descriptor = PSECURITY_DESCRIPTOR::default();
		// SAFETY: `sddl` is a static NUL-terminated string and `descriptor` is aligned writable storage.
		// The returned descriptor is owned by the caller and released by this type's `Drop` implementation.
		unsafe {
			ConvertStringSecurityDescriptorToSecurityDescriptorW(
				sddl,
				SDDL_REVISION_1,
				&mut descriptor,
				None,
			)
		}
		.map_err(windows_error)?;
		Ok(Self(descriptor))
	}

	fn attributes(&self) -> SECURITY_ATTRIBUTES {
		SECURITY_ATTRIBUTES {
			nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
			lpSecurityDescriptor: self.0.0,
			bInheritHandle: false.into(),
		}
	}

	fn dacl(&self) -> IoResult<*mut ACL> {
		let mut present = BOOL::default();
		let mut defaulted = BOOL::default();
		let mut dacl = null_mut();
		// SAFETY: `self.0` is a valid descriptor for this type's lifetime. All output pointers refer to aligned
		// writable storage and the returned DACL remains borrowed from `self`.
		unsafe { GetSecurityDescriptorDacl(self.0, &mut present, &mut dacl, &mut defaulted) }
			.map_err(windows_error)?;
		if !present.as_bool() || defaulted.as_bool() || dacl.is_null() {
			return Err(IoError::new(
				ErrorKind::PermissionDenied,
				"Windows private security descriptor has no explicit DACL",
			));
		}
		Ok(dacl)
	}

	fn dacl_bytes(&self) -> IoResult<&[u8]> {
		let dacl = self.dacl()?;
		// SAFETY: `dacl` points into this live descriptor and Windows validated it while parsing or querying.
		// `AclSize` is the allocation's full ACL byte length.
		let size = unsafe { (*dacl).AclSize as usize };
		// SAFETY: the DACL begins at `dacl`, spans `AclSize` initialized bytes, and cannot outlive `self`.
		Ok(unsafe { from_raw_parts(dacl.cast(), size) })
	}

	fn dacl_is_protected(&self) -> IoResult<bool> {
		let mut control = Default::default();
		let mut revision = 0;
		// SAFETY: `self.0` is a valid descriptor for this type's lifetime, and both output pointers refer to
		// aligned writable storage of the API's required types.
		unsafe { GetSecurityDescriptorControl(self.0, &mut control, &mut revision) }.map_err(windows_error)?;
		Ok(control & SE_DACL_PROTECTED.0 != 0)
	}
}

#[cfg(windows)]
impl Drop for LocalSecurityDescriptor {
	fn drop(&mut self) {
		// SAFETY: this type exclusively owns the descriptor returned by an API documented to require
		// `LocalFree`; the pointer is released exactly once here and is not used afterward.
		let _ = unsafe { LocalFree(Some(HLOCAL(self.0.0))) };
	}
}

#[cfg(windows)]
fn check_win32(status: WIN32_ERROR) -> IoResult<()> {
	if status == ERROR_SUCCESS {
		return Ok(());
	}
	Err(IoError::from_raw_os_error(status.0 as i32))
}

#[cfg(windows)]
fn windows_error(error: WindowsError) -> IoError {
	let code = error.code().0 as u32;
	if code & 0xffff_0000 == 0x8007_0000 {
		return IoError::from_raw_os_error((code & 0xffff) as i32);
	}
	IoError::other(error)
}

fn validate_archive(archive: &Archive, cancellation: &CancellationToken, started: Instant) -> Result<(), ArchiveError> {
	if archive.sfx_offset() != 0 {
		return Err(report!(ArchiveError::UnsupportedFormat));
	}
	match archive {
		Archive::Rar13(archive) => {
			if archive.main.is_volume() {
				return Err(report!(ArchiveError::SplitArchive));
			}
		}
		Archive::Rar15To40(archive) => {
			if archive.main.is_volume() {
				return Err(report!(ArchiveError::SplitArchive));
			}
			if archive.main.has_encrypted_headers() {
				return Err(report!(ArchiveError::Encrypted));
			}
		}
		Archive::Rar50Plus(archive) => {
			if archive.main.is_volume() {
				return Err(report!(ArchiveError::SplitArchive));
			}
			for file in archive.files() {
				if cancellation.is_cancelled() {
					return Err(report!(ArchiveError::Cancelled));
				}
				if started.elapsed() > MAX_ARCHIVE_WORK {
					return Err(report!(ArchiveError::WorkLimit));
				}
				if file.redirection.is_some() {
					return Err(report!(ArchiveError::UnsafeEntryKind));
				}
				if !file.is_stored()
					&& file.decoded_compression_info()
						.context(ArchiveError::InvalidArchive)?
						.dictionary_size > MAX_DICTIONARY_BYTES
				{
					return Err(report!(ArchiveError::DictionaryLimit));
				}
			}
		}
		_ => return Err(report!(ArchiveError::UnsupportedFormat)),
	}
	Ok(())
}

pub(crate) fn map_error(error: RarError) -> Report<ArchiveError> {
	let detail = error.to_string().to_ascii_lowercase();
	let context = if detail.contains("password") || detail.contains("encrypt") {
		ArchiveError::Encrypted
	} else if detail.contains("volume") || detail.contains("split") {
		ArchiveError::SplitArchive
	} else if detail.contains("memory") || detail.contains("dictionary") {
		ArchiveError::DictionaryLimit
	} else {
		ArchiveError::InvalidArchive
	};
	report!(error).context(context)
}

#[cfg(test)]
mod tests {
	use super::copy_source_to_snapshot;
	#[cfg(windows)]
	use super::create_snapshot;
	use super::index;
	use super::open;
	use super::preflight_snapshot;
	use crate::error::ArchiveError;
	use crate::limits::COPY_BUFFER_BYTES;
	use crate::limits::MAX_ARCHIVE_MEMBERS;
	use crate::limits::MAX_ARCHIVE_METADATA_BYTES;
	use crate::source::SourceFile;
	use rars::Archive;
	use rars::ArchiveReader;
	use rars::ArchiveVersion;
	use rars::EntrySource;
	use rars::FeatureSet;
	use rars::rar15_40::FileEntry;
	use rars::rar15_40::Rar29Method;
	use rars::rar15_40::StoredEntry;
	use rars::rar15_40::WriterOptions;
	use rars::rar15_40::write_compressed_archive;
	use rars::rar15_40::write_stored_archive;
	use rars::rar15_40::write_stored_archive_with_comment;
	use rars::rar50::ArchiveEntry;
	use rars::rar50::Rar50Writer;
	use rars::rar50::WriterOptions as Rar50WriterOptions;
	use rootcause::Result;
	use rootcause::prelude::ResultExt;
	use sha2::Digest;
	use sha2::Sha256;
	use std::error::Error;
	#[cfg(windows)]
	use std::fs::File;
	#[cfg(windows)]
	use std::fs::OpenOptions;
	use std::fs::rename;
	use std::fs::write;
	use std::io::Cursor;
	#[cfg(windows)]
	use std::io::Read;
	use std::io::Result as IoResult;
	use std::io::Seek;
	use std::io::SeekFrom;
	use std::io::Write;
	use std::result::Result as StdResult;
	use std::sync::Arc;
	use std::time::Instant;
	use tempfile::TempDir;
	use tempfile::tempfile;
	use tokio_util::sync::CancellationToken;

	type TestResult<T = ()> = StdResult<T, Box<dyn Error + Send + Sync>>;

	fn stored_rar40(name: &'static [u8], data: &'static [u8]) -> TestResult<Vec<u8>> {
		Ok(write_stored_archive(
			&[StoredEntry {
				name,
				data,
				file_time: 0,
				file_attr: 0x20,
				host_os: 3,
				password: None,
				file_comment: None,
			}],
			WriterOptions::new(ArchiveVersion::Rar40, FeatureSet::store_only()),
		)?)
	}

	fn stored_rar50(name: &[u8], data: &[u8]) -> TestResult<Vec<u8>> {
		let entry = ArchiveEntry::new(name.to_vec(), EntrySource::from_bytes(Arc::<[u8]>::from(data.to_vec())))
			.with_attributes(0x20)
			.with_host_os(3);
		Ok(
			Rar50Writer::new(Rar50WriterOptions::new(ArchiveVersion::Rar50, FeatureSet::store_only()))
				.entry(entry)
				.finish()?,
		)
	}

	fn preflight_bytes(bytes: &[u8]) -> Result<(), ArchiveError> {
		let mut file = tempfile().context(ArchiveError::Io)?;
		file.write_all(bytes).context(ArchiveError::Io)?;
		preflight_snapshot(&mut file, &CancellationToken::new(), Instant::now())
	}

	fn push_vint(bytes: &mut Vec<u8>, mut value: u64) {
		loop {
			let mut byte = (value & 0x7f) as u8;
			value >>= 7;
			if value != 0 {
				byte |= 0x80;
			}
			bytes.push(byte);
			if value == 0 {
				break;
			}
		}
	}

	fn push_rar50_header(bytes: &mut Vec<u8>, body: &[u8]) {
		bytes.extend_from_slice(&0_u32.to_le_bytes());
		push_vint(bytes, body.len() as u64);
		bytes.extend_from_slice(body);
	}

	fn rar_crc32(bytes: &[u8]) -> u32 {
		let mut crc = u32::MAX;
		for byte in bytes {
			crc ^= u32::from(*byte);
			for _ in 0..8 {
				crc = (crc >> 1) ^ (0xedb8_8320 & 0_u32.wrapping_sub(crc & 1));
			}
		}
		!crc
	}

	#[test]
	fn normal_stored_rar40_and_rar50_pass_metadata_preflight() -> TestResult {
		preflight_bytes(&stored_rar40(b"Data/rar40.txt", b"rar40")?)
			.map_err(|error| format!("RAR 4 preflight failed: {error:?}"))?;
		preflight_bytes(&stored_rar50(b"Data/rar50.txt", b"rar50")?)
			.map_err(|error| format!("RAR 5 preflight failed: {error:?}"))?;
		Ok(())
	}

	#[test]
	fn compressed_rar29_ppmd_member_is_rejected_before_archive_parser_construction() -> TestResult {
		let data = "PPMd must not allocate from a payload-controlled reset. ".repeat(256);
		let bytes = write_compressed_archive(
			&[FileEntry {
				name: b"Data/unsafe.txt",
				data: data.as_bytes(),
				file_time: 0,
				file_attr: 0x20,
				host_os: 3,
				password: None,
				file_comment: None,
			}],
			WriterOptions::new(ArchiveVersion::Rar29, FeatureSet::store_only())
				.with_compression_level(5)
				.with_method(Rar29Method::Ppmd),
		)?;

		let Err(error) = preflight_bytes(&bytes) else {
			return Err("compressed RAR 2.9 PPMd member passed metadata preflight".into());
		};
		assert_eq!(error.current_context(), &ArchiveError::UnsupportedFormat);
		Ok(())
	}

	#[test]
	fn compressed_rar40_comment_service_is_rejected_before_archive_parser_construction() -> TestResult {
		let bytes = write_stored_archive_with_comment(
			&[StoredEntry {
				name: b"Data/stored.txt",
				data: b"stored",
				file_time: 0,
				file_attr: 0x20,
				host_os: 3,
				password: None,
				file_comment: None,
			}],
			WriterOptions::new(ArchiveVersion::Rar40, FeatureSet::store_only()),
			Some(b"compressed NEWSUB comment"),
		)?;

		let Err(error) = preflight_bytes(&bytes) else {
			return Err("compressed RAR 4 NEWSUB comment passed metadata preflight".into());
		};
		assert_eq!(error.current_context(), &ArchiveError::UnsupportedFormat);
		Ok(())
	}

	#[test]
	fn rar50_service_subdata_with_old_winrar_short_size_is_accepted() -> TestResult {
		let mut bytes = Vec::from(*b"Rar!\x1a\x07\x01\x00");
		push_rar50_header(&mut bytes, &[1, 0, 0]);
		push_rar50_header(&mut bytes, &[3, 1, 3, 0, 0, 0, 0, 0, 2, b'R', b'R', 1, 7, 0x0a]);
		push_rar50_header(&mut bytes, &[5, 0, 0]);

		preflight_bytes(&bytes)
			.map_err(|error| format!("RAR 5 service SUBDATA preflight failed: {error:?}"))?;
		Ok(())
	}

	#[test]
	fn rar50_file_encryption_extra_and_encrypted_headers_remain_rejected() -> TestResult {
		let mut encrypted_file = Vec::from(*b"Rar!\x1a\x07\x01\x00");
		push_rar50_header(&mut encrypted_file, &[1, 0, 0]);
		push_rar50_header(
			&mut encrypted_file,
			&[2, 1, 3, 0, 0, 0, 0, 0, 4, b'f', b'i', b'l', b'e', 2, 1, 0],
		);
		let Err(error) = preflight_bytes(&encrypted_file) else {
			return Err("RAR 5 file encryption extra passed metadata preflight".into());
		};
		assert_eq!(error.current_context(), &ArchiveError::Encrypted);

		let mut encrypted_service = Vec::from(*b"Rar!\x1a\x07\x01\x00");
		push_rar50_header(&mut encrypted_service, &[1, 0, 0]);
		push_rar50_header(
			&mut encrypted_service,
			&[3, 1, 3, 0, 0, 0, 0, 0, 3, b'C', b'M', b'T', 2, 1, 0],
		);
		let Err(error) = preflight_bytes(&encrypted_service) else {
			return Err("RAR 5 service encryption extra passed metadata preflight".into());
		};
		assert_eq!(error.current_context(), &ArchiveError::Encrypted);

		let mut encrypted_headers = Vec::from(*b"Rar!\x1a\x07\x01\x00");
		push_rar50_header(&mut encrypted_headers, &[4, 0]);
		let Err(error) = preflight_bytes(&encrypted_headers) else {
			return Err("RAR 5 encrypted headers passed metadata preflight".into());
		};
		assert_eq!(error.current_context(), &ArchiveError::Encrypted);
		Ok(())
	}

	#[test]
	fn stored_rar50_ignores_invalid_compression_field_shape() -> TestResult {
		let name = b"Data/stored.txt";
		let mut bytes = stored_rar50(name, b"stored")?;
		let Some(name_start) = bytes.windows(name.len()).position(|window| window == name) else {
			return Err("RAR 5 fixture name was not found".into());
		};
		let compression_info = name_start
			.checked_sub(3)
			.ok_or("RAR 5 fixture compression field was not found")?;
		assert_eq!(&bytes[compression_info..name_start], &[0, 3, name.len() as u8]);
		bytes[compression_info] = 2;

		let file_header_start = 16_usize;
		assert_eq!(bytes[file_header_start + 4] & 0x80, 0);
		let file_header_end = file_header_start + 5 + usize::from(bytes[file_header_start + 4]);
		let header_crc = rar_crc32(&bytes[file_header_start + 4..file_header_end]);
		bytes[file_header_start..file_header_start + 4].copy_from_slice(&header_crc.to_le_bytes());

		let Archive::Rar50Plus(parsed) = ArchiveReader::read(&bytes)? else {
			return Err("RAR 5 fixture parsed as another archive family".into());
		};
		let Some(file) = parsed.files().next() else {
			return Err("RAR 5 fixture has no file".into());
		};
		assert!(file.is_stored());
		assert!(file.decoded_compression_info().is_err());

		let temp = TempDir::new()?;
		let path = temp.path().join("stored-invalid-compression-info.rar");
		write(&path, &bytes)?;
		let source = SourceFile::open(&path)?;
		let sha256 = <[u8; 32]>::from(Sha256::digest(&bytes));
		let members = index(&source, sha256, &CancellationToken::new(), Instant::now())?;
		assert_eq!(members.len(), 1);
		Ok(())
	}

	#[test]
	fn rar50_member_flood_is_rejected_before_archive_parser_construction() -> TestResult {
		let mut bytes = Vec::from(*b"Rar!\x1a\x07\x01\x00");
		push_rar50_header(&mut bytes, &[1, 0, 0]);
		let empty_file = [2, 0, 0, 0, 0, 0, 0, 0];
		for _ in 0..=MAX_ARCHIVE_MEMBERS {
			push_rar50_header(&mut bytes, &empty_file);
		}

		let Err(error) = preflight_bytes(&bytes) else {
			return Err("RAR 5 member flood passed metadata preflight".into());
		};
		assert_eq!(error.current_context(), &ArchiveError::ExpansionLimit);
		Ok(())
	}

	#[test]
	fn rar15_advertised_metadata_flood_is_rejected_from_sparse_file() -> TestResult {
		let mut file = tempfile()?;
		file.write_all(b"Rar!\x1a\x07\x00")?;
		let mut main = [0_u8; 13];
		main[2] = 0x73;
		main[5..7].copy_from_slice(&13_u16.to_le_bytes());
		file.write_all(&main)?;

		let block_size = u64::from(u16::MAX);
		let blocks = (MAX_ARCHIVE_METADATA_BYTES - 20) / block_size + 1;
		let mut position = 20_u64;
		for _ in 0..blocks {
			file.seek(SeekFrom::Start(position))?;
			let mut header = [0_u8; 7];
			header[2] = 0x77;
			header[5..7].copy_from_slice(&u16::MAX.to_le_bytes());
			file.write_all(&header)?;
			position += block_size;
		}
		file.set_len(position)?;

		let Err(error) = preflight_snapshot(&mut file, &CancellationToken::new(), Instant::now()) else {
			return Err("RAR 1.5 metadata flood passed metadata preflight".into());
		};
		assert_eq!(error.current_context(), &ArchiveError::ExpansionLimit);
		Ok(())
	}

	#[test]
	fn rar50_out_of_file_data_range_is_rejected_before_archive_parser_construction() -> TestResult {
		let mut bytes = Vec::from(*b"Rar!\x1a\x07\x01\x00");
		push_rar50_header(&mut bytes, &[1, 0, 0]);
		let mut body = vec![2, 2];
		push_vint(&mut body, u64::MAX);
		body.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
		push_rar50_header(&mut bytes, &body);

		let Err(error) = preflight_bytes(&bytes) else {
			return Err("RAR 5 out-of-file data range passed metadata preflight".into());
		};
		assert_eq!(error.current_context(), &ArchiveError::InvalidArchive);
		Ok(())
	}

	#[test]
	fn rar_index_and_extraction_use_retained_source_bytes_after_path_swap() -> TestResult {
		let temp = TempDir::new()?;
		let path = temp.path().join("source.rar");
		let original = stored_rar40(b"Data/original.txt", b"original")?;
		let replacement = stored_rar40(b"Data/replacement.txt", b"replacement")?;
		write(&path, &original)?;
		let source = SourceFile::open(&path)?;
		let sha256 = <[u8; 32]>::from(Sha256::digest(&original));

		rename(&path, temp.path().join("moved.rar"))?;
		write(&path, replacement)?;

		let cancellation = CancellationToken::new();
		let members = index(&source, sha256, &cancellation, Instant::now())?;
		assert_eq!(members[0].path.as_str(), "Data/original.txt");

		let archive = open(&source, sha256, &cancellation, Instant::now())?;
		let Some(contents) = archive.read_member_at(0, None)? else {
			return Err("snapshot member was missing".into());
		};
		assert_eq!(contents, b"original");
		Ok(())
	}

	#[test]
	fn cancellation_stops_snapshot_copy_between_chunks() -> TestResult {
		struct CancellingWriter {
			cancellation: CancellationToken,
			writes: usize,
		}

		impl Write for CancellingWriter {
			fn write(&mut self, bytes: &[u8]) -> IoResult<usize> {
				self.writes += 1;
				self.cancellation.cancel();
				Ok(bytes.len())
			}

			fn flush(&mut self) -> IoResult<()> {
				Ok(())
			}
		}

		let bytes = vec![0x5a; COPY_BUFFER_BYTES * 2];
		let sha256 = <[u8; 32]>::from(Sha256::digest(&bytes));
		let cancellation = CancellationToken::new();
		let mut input = Cursor::new(bytes.clone());
		let mut output = CancellingWriter {
			cancellation: cancellation.clone(),
			writes: 0,
		};
		let Err(error) = copy_source_to_snapshot(
			&mut input,
			&mut output,
			bytes.len() as u64,
			sha256,
			&cancellation,
			Instant::now(),
		) else {
			return Err("cancelled snapshot copy succeeded".into());
		};
		assert_eq!(error.current_context(), &ArchiveError::Cancelled);
		assert_eq!(output.writes, 1);
		Ok(())
	}
	#[cfg(windows)]
	#[test]
	fn private_snapshot_blocks_writes_and_path_replacement() -> TestResult {
		let temp = TempDir::new()?;
		let source_path = temp.path().join("source.rar");
		let bytes = stored_rar40(b"Data/file.txt", b"contents")?;
		write(&source_path, &bytes)?;
		let source = SourceFile::open(&source_path)?;
		let sha256 = <[u8; 32]>::from(Sha256::digest(&bytes));

		let snapshot = create_snapshot(&source, sha256, &CancellationToken::new(), Instant::now())?;
		let snapshot_path = snapshot.directory.path().join("archive.rar");
		assert!(OpenOptions::new().write(true).open(&snapshot_path).is_err());
		assert!(rename(&snapshot_path, snapshot.directory.path().join("replacement.rar")).is_err());
		assert!(rename(snapshot.directory.path(), temp.path().join("replacement-directory")).is_err());

		let mut reopened = File::open(snapshot_path)?;
		let mut contents = Vec::new();
		reopened.read_to_end(&mut contents)?;
		assert_eq!(contents, bytes);
		Ok(())
	}
}
