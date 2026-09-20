use std::path::PathBuf;
#[cfg(windows)]
use winreg::RegKey;
#[cfg(windows)]
use winreg::enums::HKEY_CURRENT_USER;
#[cfg(windows)]
use winreg::enums::HKEY_LOCAL_MACHINE;
#[cfg(windows)]
use winreg::enums::KEY_READ;
#[cfg(windows)]
use winreg::enums::KEY_WOW64_32KEY;
#[cfg(windows)]
use winreg::enums::KEY_WOW64_64KEY;

#[cfg(windows)]
pub(crate) fn steam_roots() -> Vec<PathBuf> {
	let mut roots = Vec::new();
	for (hive, flags, name) in [
		(HKEY_CURRENT_USER, KEY_READ, "SteamPath"),
		(HKEY_LOCAL_MACHINE, KEY_READ | KEY_WOW64_32KEY, "InstallPath"),
		(HKEY_LOCAL_MACHINE, KEY_READ | KEY_WOW64_64KEY, "InstallPath"),
	] {
		if let Ok(key) = RegKey::predef(hive).open_subkey_with_flags("Software\\Valve\\Steam", flags)
			&& let Ok(value) = key.get_value::<String, _>(name)
		{
			let path = PathBuf::from(value);
			if !roots.contains(&path) {
				roots.push(path);
			}
		}
	}
	roots
}

#[cfg(not(windows))]
pub(crate) fn steam_roots() -> Vec<PathBuf> {
	Vec::new()
}

#[cfg(windows)]
pub(crate) fn bethesda_hints() -> Vec<PathBuf> {
	[KEY_WOW64_32KEY, KEY_WOW64_64KEY]
		.into_iter()
		.filter_map(|view| {
			RegKey::predef(HKEY_LOCAL_MACHINE)
				.open_subkey_with_flags("Software\\Bethesda Softworks\\FalloutNV", KEY_READ | view)
				.ok()?
				.get_value::<String, _>("Installed Path")
				.ok()
				.map(PathBuf::from)
		})
		.collect()
}

#[cfg(not(windows))]
pub(crate) fn bethesda_hints() -> Vec<PathBuf> {
	Vec::new()
}
