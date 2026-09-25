use std::path::Component;
use std::path::Path;
use std::path::PathBuf;

/// Resolve lexically without requiring the target to exist or reading the filesystem.
/// Host-native Path checks do not recognize Windows drive and UNC inputs on other hosts.
pub(crate) fn resolve_path(path: &Path, startup: &Path) -> PathBuf {
	if path.is_absolute() || is_windows_absolute(path) {
		return path.to_path_buf();
	}

	let mut resolved = startup.to_path_buf();
	for component in path.components() {
		match component {
			Component::CurDir => {}
			Component::ParentDir => {
				resolved.pop();
			}
			Component::Normal(name) => resolved.push(name),
			Component::Prefix(_) | Component::RootDir => return path.to_path_buf(),
		}
	}

	resolved
}

fn is_windows_absolute(path: &Path) -> bool {
	#[cfg(windows)]
	{
		path.is_absolute()
	}
	#[cfg(not(windows))]
	{
		let Some(value) = path.to_str() else {
			return false;
		};
		let bytes = value.as_bytes();
		(bytes.len() >= 3
			&& bytes[0].is_ascii_alphabetic()
			&& bytes[1] == b':' && matches!(bytes[2], b'\\' | b'/'))
			|| value.strip_prefix("\\\\")
				.is_some_and(|unc| unc.split(['\\', '/']).filter(|part| !part.is_empty()).count() >= 2)
	}
}
