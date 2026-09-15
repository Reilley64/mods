use super::open::directory_options;
use super::open::open_dir;
use super::open::require_directory;
use super::same_dir;
use cap_std::ambient_authority;
use cap_std::fs::Dir;
use cap_std::fs::File;
use rootcause::Result;
use rootcause::report;
use std::ffi::OsString;
use std::fs;
use std::io::Error as IoError;
use std::io::ErrorKind;
use std::path::Component;
use std::path::MAIN_SEPARATOR_STR;
use std::path::Path;
use std::path::PathBuf;

pub(crate) fn open_ambient_dir(path: &Path) -> Result<(Dir, PathBuf), IoError> {
	let (mut directory, components) = absolute_anchor(path)?;
	for name in components {
		directory = open_dir(&directory, Path::new(&name))?;
	}
	let canonical = fs::canonicalize(path)?;
	let named = File::open_ambient_with(&canonical, &directory_options(), ambient_authority())?;
	let named = Dir::from_std_file(named.into_std());
	if !same_dir(&directory, &named)? {
		return Err(report!(IoError::other("directory path changed while it was opened")));
	}
	Ok((directory, canonical))
}

pub(crate) fn open_existing_ancestor(path: &Path) -> Result<(Dir, bool), IoError> {
	let (mut directory, components) = absolute_anchor(path)?;
	for name in components {
		match open_dir(&directory, Path::new(&name)) {
			Ok(child) => directory = child,
			Err(error) if error.current_context().kind() == ErrorKind::NotFound => {
				return Ok((directory, false));
			}
			Err(error) => return Err(error),
		}
	}
	Ok((directory, true))
}

fn absolute_anchor(path: &Path) -> Result<(Dir, Vec<OsString>), IoError> {
	let mut components = path.components();
	let mut root = PathBuf::new();
	match components.next() {
		#[cfg(windows)]
		Some(Component::Prefix(prefix)) => {
			root.push(prefix.as_os_str());
			if components.next() != Some(Component::RootDir) {
				return Err(report!(IoError::other("path is not absolute")));
			}
			root.push(Path::new("\\"));
		}
		Some(Component::RootDir) => root.push(Path::new(MAIN_SEPARATOR_STR)),
		_ => return Err(report!(IoError::other("path is not absolute"))),
	}
	let mut names = Vec::new();
	for component in components {
		match component {
			Component::Normal(name) => names.push(name.to_owned()),
			_ => return Err(report!(IoError::other("path contains an unsafe component"))),
		}
	}
	let file = File::open_ambient_with(&root, &directory_options(), ambient_authority())?;
	require_directory(&file.metadata()?)?;
	Ok((Dir::from_std_file(file.into_std()), names))
}
