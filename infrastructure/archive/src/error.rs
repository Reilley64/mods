use std::error::Error;
use std::fmt::Display;
use std::fmt::Formatter;
use std::fmt::Result as FmtResult;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ArchiveError {
	Cancelled,
	DictionaryLimit,
	DuplicatePath,
	Encrypted,
	ExpansionLimit,
	IdentityChanged,
	InvalidArchive,
	InvalidXml,
	Io,
	MissingMember,
	NonLosslessName,
	SplitArchive,
	UnsupportedFormat,
	UnsupportedInstaller,
	UnsafeEntryKind,
	UnsafePath,
	WorkLimit,
}

impl Display for ArchiveError {
	fn fmt(&self, formatter: &mut Formatter<'_>) -> FmtResult {
		let message = match self {
			Self::Cancelled => "archive operation was cancelled",
			Self::DictionaryLimit => "archive dictionary exceeds the safety limit",
			Self::DuplicatePath => "archive contains a non-portable duplicate path",
			Self::Encrypted => "encrypted archives are unsupported",
			Self::ExpansionLimit => "archive exceeds an expansion safety limit",
			Self::IdentityChanged => "archive identity changed",
			Self::InvalidArchive => "archive is invalid",
			Self::InvalidXml => "FOMOD configuration is invalid",
			Self::Io => "archive I/O failed",
			Self::MissingMember => "archive member is missing",
			Self::NonLosslessName => "archive member name cannot be decoded without loss",
			Self::SplitArchive => "split or multivolume archives are unsupported",
			Self::UnsupportedFormat => "archive format is unsupported",
			Self::UnsupportedInstaller => "installer behavior is unsupported",
			Self::UnsafeEntryKind => "archive contains a link, redirection, device, or other unsafe entry",
			Self::UnsafePath => "archive contains an unsafe path",
			Self::WorkLimit => "archive operation exceeded the elapsed-work limit",
		};
		formatter.write_str(message)
	}
}

impl Error for ArchiveError {}
