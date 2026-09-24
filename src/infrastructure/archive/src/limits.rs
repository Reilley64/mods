use std::time::Duration;

pub(crate) const COPY_BUFFER_BYTES: usize = 64 * 1024;
pub(crate) const MAX_ARCHIVE_BYTES: u64 = 64 * 1024 * 1024 * 1024;
pub(crate) const MAX_ARCHIVE_MEMBERS: usize = 100_000;
pub(crate) const MAX_ARCHIVE_METADATA_BYTES: u64 = 128 * 1024 * 1024;
pub(crate) const MAX_ARCHIVE_PATH_BYTES: usize = 1_024;
pub(crate) const MAX_ARCHIVE_PATH_COMPONENTS: usize = 64;
pub(crate) const MAX_ARCHIVE_PATH_COMPONENT_UTF16: usize = 240;
pub(crate) const MAX_COMPRESSION_RATIO: u64 = 1_000;
pub(crate) const MAX_DICTIONARY_BYTES: u64 = 128 * 1024 * 1024;
/// Derived work counts each FOMOD descriptor, source-root probe, and file yielded by a folder descriptor.
pub(crate) const MAX_FOMOD_DERIVED_WORK: usize = 10_000;
/// Derived candidates count concrete source-member/destination pairs produced from FOMOD descriptors.
pub(crate) const MAX_FOMOD_DERIVED_CANDIDATES: usize = 50_000;
/// A source member can feed this many destinations while one synchronous backend stream is active.
pub(crate) const MAX_SOURCE_DESTINATION_FAN_OUT: usize = 64;
// Winning destinations may duplicate source bytes, so bound staged output independently of archive expansion.
pub(crate) const MAX_STAGED_OUTPUT_BYTES: u64 = 32 * 1024 * 1024 * 1024;
pub(crate) const MAX_MEMBER_BYTES: u64 = 8 * 1024 * 1024 * 1024;
pub(crate) const MAX_TOTAL_UNCOMPRESSED_BYTES: u64 = 32 * 1024 * 1024 * 1024;
pub(crate) const MAX_XML_ATTRIBUTES_PER_ELEMENT: usize = 32;
pub(crate) const MAX_XML_BYTES: u64 = 4 * 1024 * 1024;
pub(crate) const MAX_XML_DEPTH: usize = 64;
pub(crate) const MAX_XML_NODES: usize = 100_000;
pub(crate) const MAX_XML_TEXT_BYTES: usize = 1024 * 1024;
pub(crate) const MAX_ARCHIVE_WORK: Duration = Duration::from_secs(30 * 60);
