use crate::error::ArchiveError;
use crate::index::ArchiveFormat;
use crate::index::ArchiveIndexCore;
use crate::index::ArchiveMember;
use crate::index::MemberKind;
use crate::index::verify_identity;
use crate::limits::COPY_BUFFER_BYTES;
use crate::limits::MAX_ARCHIVE_WORK;
use crate::limits::MAX_DICTIONARY_BYTES;
use crate::rar::map_error as map_rar_error;
use crate::rar::open as open_rar;
use crate::seven_zip::map_error as map_seven_zip_error;
use crate::seven_zip::open_controlled as open_seven_zip;
use crate::zip::map_error as map_zip_error;
use crate::zip::open as open_zip;
use rars::ArchiveReadOptions;
use rootcause::Report;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use rootcause::report;
use std::cell::Cell;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::HashMap;
use std::error::Error;
use std::fmt::Display;
use std::fmt::Formatter;
use std::fmt::Result as FmtResult;
use std::io::Error as IoError;
use std::io::Read;
use std::io::Result as IoResult;
use std::io::Write;
use std::io::sink;
use std::rc::Rc;
use std::time::Instant;
use tokio_util::sync::CancellationToken;

pub(crate) fn extract_selected<F>(
	index: &ArchiveIndexCore,
	ordinals: &[usize],
	cancellation: &CancellationToken,
	mut open_output: F,
) -> Result<(), ArchiveError>
where
	F: FnMut(&ArchiveMember) -> Result<Box<dyn Write>, ArchiveError>,
{
	let started = Instant::now();
	let selected: BTreeSet<usize> = ordinals.iter().copied().collect();
	if selected.len() != ordinals.len() {
		return Err(report!(ArchiveError::DuplicatePath));
	}
	for ordinal in &selected {
		let member = index
			.members
			.get(*ordinal)
			.ok_or_else(|| report!(ArchiveError::MissingMember))?;
		if member.kind != MemberKind::File {
			return Err(report!(ArchiveError::UnsafeEntryKind));
		}
	}

	verify_identity(index, cancellation, started)?;
	let extracted = match index.format {
		ArchiveFormat::Zip => extract_zip(index, &selected, cancellation, started, &mut open_output)?,
		ArchiveFormat::SevenZip => {
			extract_seven_zip(index, &selected, cancellation, started, &mut open_output)?
		}
		ArchiveFormat::Rar => extract_rar(index, &selected, cancellation, started, &mut open_output)?,
	};
	if extracted != selected {
		return Err(report!(ArchiveError::MissingMember));
	}
	verify_identity(index, cancellation, started)
}

pub(crate) fn read_member_bounded(
	index: &ArchiveIndexCore,
	ordinal: usize,
	limit: u64,
	cancellation: &CancellationToken,
) -> Result<Vec<u8>, ArchiveError> {
	let member = index
		.members
		.get(ordinal)
		.ok_or_else(|| report!(ArchiveError::MissingMember))?;
	if member.uncompressed_size > limit {
		return Err(report!(ArchiveError::ExpansionLimit));
	}
	let capacity = usize::try_from(member.uncompressed_size).context(ArchiveError::ExpansionLimit)?;
	let bytes = Rc::new(RefCell::new(Vec::with_capacity(capacity)));
	let output = Rc::clone(&bytes);
	extract_selected(index, &[ordinal], cancellation, move |_| {
		Ok(Box::new(SharedBuffer(Rc::clone(&output))))
	})?;
	let bytes = Rc::try_unwrap(bytes).map_err(|_| report!(ArchiveError::InvalidArchive))?;
	Ok(bytes.into_inner())
}

fn extract_zip<F>(
	index: &ArchiveIndexCore,
	selected: &BTreeSet<usize>,
	cancellation: &CancellationToken,
	started: Instant,
	open_output: &mut F,
) -> Result<BTreeSet<usize>, ArchiveError>
where
	F: FnMut(&ArchiveMember) -> Result<Box<dyn Write>, ArchiveError>,
{
	let mut archive = open_zip(index.source.duplicate()?, cancellation, started)?;
	let mut extracted = BTreeSet::new();
	for ordinal in selected {
		if cancellation.is_cancelled() {
			return Err(report!(ArchiveError::Cancelled));
		}
		if started.elapsed() > MAX_ARCHIVE_WORK {
			return Err(report!(ArchiveError::WorkLimit));
		}
		let member = &index.members[*ordinal];
		let mut input = archive.by_index(*ordinal).map_err(map_zip_error)?;
		if input.encrypted() {
			return Err(report!(ArchiveError::Encrypted));
		}
		let output = open_output(member)?;
		copy_exact(&mut input, output, member.uncompressed_size, cancellation, started)?;
		extracted.insert(*ordinal);
	}
	Ok(extracted)
}

fn extract_seven_zip<F>(
	index: &ArchiveIndexCore,
	selected: &BTreeSet<usize>,
	cancellation: &CancellationToken,
	started: Instant,
	open_output: &mut F,
) -> Result<BTreeSet<usize>, ArchiveError>
where
	F: FnMut(&ArchiveMember) -> Result<Box<dyn Write>, ArchiveError>,
{
	let mut ordinals_by_name = HashMap::with_capacity(index.members.len());
	for member in &index.members {
		if cancellation.is_cancelled() {
			return Err(report!(ArchiveError::Cancelled));
		}
		if started.elapsed() > MAX_ARCHIVE_WORK {
			return Err(report!(ArchiveError::WorkLimit));
		}
		ordinals_by_name.insert(member.path.as_str(), member.ordinal);
	}

	let mut archive = open_seven_zip(index.source.duplicate()?, cancellation, started)?;
	let mut extracted = BTreeSet::new();
	// The backend callback cannot return a Report, so retain it until the backend gives ownership back.
	let mut callback_error = None;
	let result = archive.for_each_entries(|entry, input| {
		if cancellation.is_cancelled() {
			callback_error = Some(report!(ArchiveError::Cancelled));
			return Err(IoError::other("archive callback stopped").into());
		}
		if started.elapsed() > MAX_ARCHIVE_WORK {
			callback_error = Some(report!(ArchiveError::WorkLimit));
			return Err(IoError::other("archive callback stopped").into());
		}
		let normalized_name = entry.name.trim_end_matches(['/', '\\']).replace('\\', "/");
		let Some(&ordinal) = ordinals_by_name.get(normalized_name.as_str()) else {
			callback_error = Some(report!(ArchiveError::IdentityChanged));
			return Err(IoError::other("archive callback stopped").into());
		};
		let member = &index.members[ordinal];
		let output: Box<dyn Write> = if selected.contains(&ordinal) {
			match open_output(member) {
				Ok(output) => output,
				Err(error) => {
					callback_error = Some(error);
					return Err(IoError::other("archive callback stopped").into());
				}
			}
		} else {
			Box::new(sink())
		};
		if let Err(error) = copy_exact(input, output, member.uncompressed_size, cancellation, started) {
			callback_error = Some(error);
			return Err(IoError::other("archive callback stopped").into());
		}
		if selected.contains(&ordinal) {
			extracted.insert(ordinal);
		}
		Ok(true)
	});
	if let Some(error) = callback_error {
		return Err(error);
	}
	result.map_err(map_seven_zip_error)?;
	Ok(extracted)
}

fn extract_rar<F>(
	index: &ArchiveIndexCore,
	selected: &BTreeSet<usize>,
	cancellation: &CancellationToken,
	started: Instant,
	open_output: &mut F,
) -> Result<BTreeSet<usize>, ArchiveError>
where
	F: FnMut(&ArchiveMember) -> Result<Box<dyn Write>, ArchiveError>,
{
	let archive = open_rar(&index.source, index.sha256, cancellation, started)?;
	let next_ordinal = Cell::new(0_usize);
	let written = Rc::new(RefCell::new(BTreeMap::new()));
	// The backend callback cannot return a Report, so retain it until the backend gives ownership back.
	let callback_error = Rc::new(RefCell::new(None));
	let written_for_callback = Rc::clone(&written);
	let error_for_callback = Rc::clone(&callback_error);
	let result = archive.extract_to_with_options(
		ArchiveReadOptions::new().with_rar50_buffered_decode_limit(MAX_DICTIONARY_BYTES),
		|_| {
			if cancellation.is_cancelled() {
				*error_for_callback.borrow_mut() = Some(report!(ArchiveError::Cancelled));
				return Err(IoError::other("archive callback stopped").into());
			}
			if started.elapsed() > MAX_ARCHIVE_WORK {
				*error_for_callback.borrow_mut() = Some(report!(ArchiveError::WorkLimit));
				return Err(IoError::other("archive callback stopped").into());
			}
			let ordinal = next_ordinal.get();
			next_ordinal.set(ordinal.saturating_add(1));
			let Some(member) = index.members.get(ordinal) else {
				*error_for_callback.borrow_mut() = Some(report!(ArchiveError::IdentityChanged));
				return Err(IoError::other("archive callback stopped").into());
			};
			let output: Box<dyn Write> = if selected.contains(&ordinal) {
				match open_output(member) {
					Ok(output) => output,
					Err(error) => {
						*error_for_callback.borrow_mut() = Some(error);
						return Err(IoError::other("archive callback stopped").into());
					}
				}
			} else {
				Box::new(sink())
			};
			let counter = Rc::new(Cell::new(0_u64));
			if selected.contains(&ordinal) {
				written_for_callback.borrow_mut().insert(ordinal, Rc::clone(&counter));
			}
			Ok(Box::new(BoundedWriter::with_counter(
				output,
				member.uncompressed_size,
				cancellation.clone(),
				started,
				counter,
				Some(Rc::clone(&error_for_callback)),
			)) as Box<dyn Write>)
		},
	);
	if let Some(error) = callback_error.borrow_mut().take() {
		return Err(error);
	}
	if cancellation.is_cancelled() {
		let error = match result {
			Ok(_) => report!(ArchiveError::Cancelled),
			Err(error) => report!(error).context(ArchiveError::Cancelled),
		};
		return Err(error);
	}
	if started.elapsed() > MAX_ARCHIVE_WORK {
		let error = match result {
			Ok(_) => report!(ArchiveError::WorkLimit),
			Err(error) => report!(error).context(ArchiveError::WorkLimit),
		};
		return Err(error);
	}
	result.map_err(map_rar_error)?;

	let mut extracted = BTreeSet::new();
	for ordinal in selected {
		let count = written.borrow().get(ordinal).map(|count| count.get());
		if count != Some(index.members[*ordinal].uncompressed_size) {
			return Err(report!(ArchiveError::InvalidArchive));
		}
		extracted.insert(*ordinal);
	}
	Ok(extracted)
}

// std::io::copy has no declared-size bound or cooperative cancellation checkpoint.
fn copy_exact(
	input: &mut dyn Read,
	output: Box<dyn Write>,
	expected: u64,
	cancellation: &CancellationToken,
	started: Instant,
) -> Result<(), ArchiveError> {
	let counter = Rc::new(Cell::new(0_u64));
	let mut output = BoundedWriter::with_counter(
		output,
		expected,
		cancellation.clone(),
		started,
		Rc::clone(&counter),
		None,
	);
	let mut buffer = vec![0; COPY_BUFFER_BYTES];
	loop {
		if cancellation.is_cancelled() {
			return Err(report!(ArchiveError::Cancelled));
		}
		if started.elapsed() > MAX_ARCHIVE_WORK {
			return Err(report!(ArchiveError::WorkLimit));
		}
		let count = input.read(&mut buffer).map_err(map_payload_io)?;
		if count == 0 {
			break;
		}
		output.write_all(&buffer[..count]).map_err(map_payload_io)?;
	}
	output.flush().map_err(map_payload_io)?;
	if counter.get() != expected {
		return Err(report!(ArchiveError::InvalidArchive));
	}
	Ok(())
}

struct BoundedWriter {
	inner: Box<dyn Write>,
	expected: u64,
	written: Rc<Cell<u64>>,
	cancellation: CancellationToken,
	started: Instant,
	callback_error: Option<Rc<RefCell<Option<Report<ArchiveError>>>>>,
}

impl BoundedWriter {
	fn with_counter(
		inner: Box<dyn Write>,
		expected: u64,
		cancellation: CancellationToken,
		started: Instant,
		written: Rc<Cell<u64>>,
		callback_error: Option<Rc<RefCell<Option<Report<ArchiveError>>>>>,
	) -> Self {
		Self {
			inner,
			expected,
			written,
			cancellation,
			started,
			callback_error,
		}
	}

	fn stop_callback(&self, error: IoError) -> IoError {
		let Some(callback_error) = &self.callback_error else {
			return error;
		};
		let mut callback_error = callback_error.borrow_mut();
		if callback_error.is_none() {
			*callback_error = Some(map_payload_io(error));
		}
		IoError::other(CallbackStopped)
	}
}

impl Write for BoundedWriter {
	fn write(&mut self, bytes: &[u8]) -> IoResult<usize> {
		if self.cancellation.is_cancelled() {
			let error = IoError::other(CancelledWrite);
			return Err(self.stop_callback(error));
		}
		if self.started.elapsed() > MAX_ARCHIVE_WORK {
			let error = IoError::other(WorkLimitReached);
			return Err(self.stop_callback(error));
		}
		let Some(next) = self.written.get().checked_add(bytes.len() as u64) else {
			let error = IoError::other("archive member exceeds declared size");
			return Err(self.stop_callback(error));
		};
		if next > self.expected {
			let error = IoError::other("archive member exceeds declared size");
			return Err(self.stop_callback(error));
		}
		let count = match self.inner.write(bytes) {
			Ok(count) => count,
			Err(error) => return Err(self.stop_callback(error)),
		};
		let Some(written) = self.written.get().checked_add(count as u64) else {
			let error = IoError::other("archive member exceeds declared size");
			return Err(self.stop_callback(error));
		};
		self.written.set(written);
		Ok(count)
	}

	fn flush(&mut self) -> IoResult<()> {
		if self.cancellation.is_cancelled() {
			let error = IoError::other(CancelledWrite);
			return Err(self.stop_callback(error));
		}
		if self.started.elapsed() > MAX_ARCHIVE_WORK {
			let error = IoError::other(WorkLimitReached);
			return Err(self.stop_callback(error));
		}
		if let Err(error) = self.inner.flush() {
			return Err(self.stop_callback(error));
		}
		Ok(())
	}
}

struct SharedBuffer(Rc<RefCell<Vec<u8>>>);

impl Write for SharedBuffer {
	fn write(&mut self, bytes: &[u8]) -> IoResult<usize> {
		self.0.borrow_mut().extend_from_slice(bytes);
		Ok(bytes.len())
	}

	fn flush(&mut self) -> IoResult<()> {
		Ok(())
	}
}

// Write::write_all retries Interrupted forever, so bridge control signals use distinct non-retryable errors.
#[derive(Debug)]
struct CallbackStopped;

impl Display for CallbackStopped {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> FmtResult {
		formatter.write_str("archive callback stopped")
	}
}

impl Error for CallbackStopped {}

#[derive(Debug)]
struct CancelledWrite;

impl Display for CancelledWrite {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> FmtResult {
		formatter.write_str("archive operation cancelled")
	}
}

impl Error for CancelledWrite {}

#[derive(Debug)]
struct WorkLimitReached;

impl Display for WorkLimitReached {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> FmtResult {
		formatter.write_str("archive operation exceeded the elapsed-work limit")
	}
}

impl Error for WorkLimitReached {}

fn map_payload_io(error: IoError) -> Report<ArchiveError> {
	let context = if error.get_ref().is_some_and(|source| source.is::<CancelledWrite>()) {
		ArchiveError::Cancelled
	} else if error.get_ref().is_some_and(|source| source.is::<WorkLimitReached>()) {
		ArchiveError::WorkLimit
	} else {
		ArchiveError::Io
	};
	report!(error).context(context)
}

#[cfg(test)]
mod tests {
	use super::SharedBuffer;
	use super::extract_seven_zip;
	use crate::error::ArchiveError;
	use crate::index::ArchiveIndexCore;
	use crate::index::ArchiveMember;
	use crate::index::index_archive;
	use crate::limits::MAX_ARCHIVE_MEMBERS;
	use crate::path::SafeArchivePath;
	use rootcause::Result;
	use rootcause::report;
	use sevenz_rust2::ArchiveEntry;
	use sevenz_rust2::ArchiveWriter;
	use std::cell::RefCell;
	use std::collections::BTreeMap;
	use std::collections::BTreeSet;
	use std::error::Error;
	use std::fs::File;
	use std::io::Cursor;
	use std::io::Write;
	use std::rc::Rc;
	use std::result::Result as StdResult;
	use std::time::Instant;
	use tempfile::TempDir;
	use tokio_util::sync::CancellationToken;

	type TestResult<T = ()> = StdResult<T, Box<dyn Error>>;

	fn seven_zip_index(entries: &[(&str, Option<&[u8]>)]) -> TestResult<(TempDir, ArchiveIndexCore)> {
		let temp = TempDir::new()?;
		let path = temp.path().join("fixture.7z");
		let mut writer = ArchiveWriter::new(File::create(&path)?)?;
		writer.set_encrypt_header(false);
		for (name, contents) in entries {
			if let Some(contents) = contents {
				writer.push_archive_entry(
					ArchiveEntry::new_file(name),
					Some(Cursor::new(contents.to_vec())),
				)?;
			} else {
				writer.push_archive_entry(ArchiveEntry::new_directory(name), None::<Cursor<Vec<u8>>>)?;
			}
		}
		writer.finish()?;
		let index = index_archive(&path, &CancellationToken::new()).map_err(|error| format!("{error:?}"))?;
		Ok((temp, index))
	}

	fn extract_to_buffers(
		index: &ArchiveIndexCore,
		selected: &BTreeSet<usize>,
	) -> Result<BTreeMap<String, Vec<u8>>, ArchiveError> {
		let buffers = Rc::new(RefCell::new(BTreeMap::new()));
		let output_buffers = Rc::clone(&buffers);
		extract_seven_zip(
			index,
			selected,
			&CancellationToken::new(),
			Instant::now(),
			&mut |member| {
				let buffer = Rc::new(RefCell::new(Vec::new()));
				output_buffers
					.borrow_mut()
					.insert(member.path.as_str().to_owned(), Rc::clone(&buffer));
				Ok(Box::new(SharedBuffer(buffer)))
			},
		)?;
		let mut extracted = BTreeMap::new();
		for (name, buffer) in buffers.borrow().iter() {
			extracted.insert(name.clone(), buffer.borrow().clone());
		}
		Ok(extracted)
	}

	#[test]
	fn seven_zip_extraction_matches_reordered_members_by_exact_name() -> TestResult {
		let (_temp, mut index) = seven_zip_index(&[
			("Data/first.txt", Some(b"first")),
			("Data/second.txt", Some(b"second payload")),
		])?;
		index.members.swap(0, 1);
		for (ordinal, member) in index.members.iter_mut().enumerate() {
			member.ordinal = ordinal;
		}

		let extracted =
			extract_to_buffers(&index, &BTreeSet::from([0, 1])).map_err(|error| format!("{error:?}"))?;

		assert_eq!(extracted.get("Data/first.txt").map(Vec::as_slice), Some(&b"first"[..]));
		assert_eq!(
			extracted.get("Data/second.txt").map(Vec::as_slice),
			Some(&b"second payload"[..]),
		);
		Ok(())
	}

	#[test]
	fn seven_zip_extraction_rejects_an_exact_name_mismatch() -> TestResult {
		let (_temp, mut index) = seven_zip_index(&[("Data/File.txt", Some(b"payload"))])?;
		index.members.first_mut().ok_or("indexed archive had no members")?.path =
			SafeArchivePath::new("data/File.txt").map_err(|error| format!("{error:?}"))?;

		let error = extract_to_buffers(&index, &BTreeSet::from([0]))
			.err()
			.ok_or("mismatched member name was accepted")?;

		assert_eq!(error.current_context(), &ArchiveError::IdentityChanged);
		Ok(())
	}

	#[test]
	fn seven_zip_extraction_normalizes_directory_separators_and_trailing_slashes() -> TestResult {
		let (_temp, index) =
			seven_zip_index(&[(r"Data\Folder\", None), (r"Data\Folder\file.txt", Some(b"payload"))])?;
		let file = index
			.members
			.iter()
			.find(|member| member.path.as_str() == "Data/Folder/file.txt")
			.ok_or("normalized file was not indexed")?;

		let extracted = extract_to_buffers(&index, &BTreeSet::from([file.ordinal]))
			.map_err(|error| format!("{error:?}"))?;

		assert_eq!(
			extracted.get("Data/Folder/file.txt").map(Vec::as_slice),
			Some(&b"payload"[..]),
		);
		Ok(())
	}

	#[test]
	fn cancelled_large_seven_zip_lookup_stops_before_backend_iteration() -> TestResult {
		let (_temp, mut index) = seven_zip_index(&[("Data/file.txt", Some(b"payload"))])?;
		let prototype = index.members.first().ok_or("indexed archive had no members")?.clone();
		index.members = (0..MAX_ARCHIVE_MEMBERS)
			.map(|ordinal| {
				let mut member = prototype.clone();
				member.ordinal = ordinal;
				member
			})
			.collect();
		let cancellation = CancellationToken::new();
		cancellation.cancel();
		let mut open_output = |_: &ArchiveMember| -> Result<Box<dyn Write>, ArchiveError> {
			Err(report!(ArchiveError::InvalidArchive))
		};

		let error = extract_seven_zip(
			&index,
			&BTreeSet::from([0]),
			&cancellation,
			Instant::now(),
			&mut open_output,
		)
		.err()
		.ok_or("cancelled member lookup continued")?;

		assert_eq!(error.current_context(), &ArchiveError::Cancelled);
		Ok(())
	}
}
