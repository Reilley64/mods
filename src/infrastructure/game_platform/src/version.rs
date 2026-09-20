use crate::GamePlatformAdapter;
use crate::bound_game::reopen_bound_game;
use crate::file_version::read_file_version;
use crate::fs_access;
use application::ErrorMarker;
use domain::GameBinding;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use rootcause::report;
use std::io::ErrorKind;
use std::path::Path;
use tokio_util::sync::CancellationToken;

const XNVSE_VERSION_FILES: [&str; 2] = ["nvse_loader.exe", "nvse_1_4.dll"];

impl GamePlatformAdapter {
	pub(crate) fn read_game_version(
		&self,
		binding: &GameBinding,
		cancellation: &CancellationToken,
	) -> Result<Vec<u32>, ErrorMarker> {
		let game = reopen_bound_game(binding, cancellation)?;
		if cancellation.is_cancelled() {
			return Err(report!(ErrorMarker::operation_cancelled()));
		}

		let executable = fs_access::open_regular(&game, Path::new("FalloutNV.exe"))
			.context(ErrorMarker::game_install_invalid())?;
		read_file_version(executable)
	}

	pub(crate) fn read_xnvse_version(
		&self,
		binding: &GameBinding,
		cancellation: &CancellationToken,
	) -> Result<Option<Vec<u32>>, ErrorMarker> {
		let game = reopen_bound_game(binding, cancellation)?;
		let mut malformed = None;
		for name in XNVSE_VERSION_FILES {
			if cancellation.is_cancelled() {
				return Err(report!(ErrorMarker::operation_cancelled()));
			}

			let file = match fs_access::open_regular(&game, Path::new(name)) {
				Ok(file) => file,
				Err(error) if error.current_context().kind() == ErrorKind::NotFound => continue,
				Err(error) => return Err(error.context(ErrorMarker::game_install_invalid())),
			};
			let Some(mut version) = read_file_version(file)
				.map_err(|error| {
					if malformed.is_none() {
						malformed = Some(error);
					}
				})
				.ok()
			else {
				continue;
			};

			// xNVSE publishes 6.x.y as Windows file version 0.6.x.y. FOMOD dependencies use the
			// public three-component version, so the resource-only prefix must not affect ordering.
			if version.len() == 4 && version[0] == 0 {
				version.remove(0);
			}
			return Ok(Some(version));
		}
		if let Some(error) = malformed {
			return Err(error);
		}
		Ok(None)
	}
}

#[cfg(test)]
mod tests {
	use super::GamePlatformAdapter;
	use crate::adapter::KnownFolderSource;
	use crate::steam;
	use rootcause::Result;
	use rootcause::report;
	use std::fs;
	use std::io::Error as IoError;
	use std::path::Path;
	use std::path::PathBuf;
	use std::sync::Arc;
	use tempfile::TempDir;
	use tokio_util::sync::CancellationToken;

	#[test]
	fn game_and_xnvse_versions_use_their_semantic_component_order() -> Result<()> {
		let (_temp, game) = fixture()?;
		fs::write(game.join("FalloutNV.exe"), pe_with_version([1, 4, 0, 525]))?;
		fs::write(game.join("nvse_loader.exe"), pe_with_version([0, 6, 4, 9]))?;
		let binding = steam::validate(&game)?;
		let adapter = adapter_without_sources();

		let game_version = adapter.read_game_version(&binding, &CancellationToken::new())?;
		let xnvse_version = adapter.read_xnvse_version(&binding, &CancellationToken::new())?;

		assert_eq!(game_version, vec![1, 4, 0, 525]);
		assert_eq!(xnvse_version, Some(vec![6, 4, 9]));
		Ok(())
	}

	#[test]
	fn xnvse_core_is_used_when_loader_is_absent() -> Result<()> {
		let (_temp, game) = fixture()?;
		fs::write(game.join("nvse_1_4.dll"), pe_with_version([0, 6, 3, 10]))?;
		let binding = steam::validate(&game)?;

		let version = adapter_without_sources().read_xnvse_version(&binding, &CancellationToken::new())?;

		assert_eq!(version, Some(vec![6, 3, 10]));
		Ok(())
	}

	#[test]
	fn missing_xnvse_returns_none() -> Result<()> {
		let (_temp, game) = fixture()?;
		let binding = steam::validate(&game)?;

		let version = adapter_without_sources().read_xnvse_version(&binding, &CancellationToken::new())?;

		assert_eq!(version, None);
		Ok(())
	}

	fn fixture() -> Result<(TempDir, PathBuf)> {
		let temp = TempDir::new()?;
		let game = fs::canonicalize(temp.path())?.join("steam/steamapps/common/Fallout New Vegas");
		fs::create_dir_all(game.join("Data"))?;
		fs::write(game.join("FalloutNV.exe"), b"exe")?;
		fs::write(game.join("Fallout_default.ini"), b"[Archive]\n")?;
		fs::write(
			game.parent()
				.and_then(Path::parent)
				.ok_or_else(|| report!(IoError::other("missing steamapps")))?
				.join("appmanifest_22380.acf"),
			concat!(
				"\"AppState\"\n{\n",
				"\"appid\" \"22380\"\n",
				"\"buildid\" \"88\"\n",
				"\"installdir\" \"Fallout New Vegas\"\n}"
			),
		)?;
		Ok((temp, game))
	}

	fn adapter_without_sources() -> GamePlatformAdapter {
		GamePlatformAdapter {
			steam_roots: Arc::new(Vec::new()),
			bethesda_hints: Arc::new(Vec::new()),
			known_folders: KnownFolderSource::System,
		}
	}

	fn pe_with_version([major, minor, patch, build]: [u16; 4]) -> Vec<u8> {
		let version_resource = version_resource([major, minor, patch, build]);
		let resource_data_offset = 88_u32;
		let resource_size = resource_data_offset + u32::try_from(version_resource.len()).unwrap_or(0);
		let mut image = vec![0_u8; 0x400];

		put_u16(&mut image, 0, 0x5a4d);
		put_u32(&mut image, 0x3c, 0x80);
		image[0x80..0x84].copy_from_slice(b"PE\0\0");
		put_u16(&mut image, 0x84, 0x14c);
		put_u16(&mut image, 0x86, 1);
		put_u16(&mut image, 0x94, 224);
		put_u16(&mut image, 0x96, 0x0102);

		let optional = 0x98;
		put_u16(&mut image, optional, 0x10b);
		put_u32(&mut image, optional + 28, 0x0040_0000);
		put_u32(&mut image, optional + 32, 0x1000);
		put_u32(&mut image, optional + 36, 0x200);
		put_u32(&mut image, optional + 56, 0x2000);
		put_u32(&mut image, optional + 60, 0x200);
		put_u16(&mut image, optional + 68, 3);
		put_u32(&mut image, optional + 92, 16);
		put_u32(&mut image, optional + 112, 0x1000);
		put_u32(&mut image, optional + 116, resource_size);

		let section = 0x178;
		image[section..section + 8].copy_from_slice(b".rsrc\0\0\0");
		put_u32(&mut image, section + 8, resource_size);
		put_u32(&mut image, section + 12, 0x1000);
		put_u32(&mut image, section + 16, 0x200);
		put_u32(&mut image, section + 20, 0x200);
		put_u32(&mut image, section + 36, 0x4000_0040);

		let resource = 0x200;
		put_u16(&mut image, resource + 14, 1);
		put_u32(&mut image, resource + 16, 16);
		put_u32(&mut image, resource + 20, 0x8000_0018);
		put_u16(&mut image, resource + 24 + 14, 1);
		put_u32(&mut image, resource + 24 + 16, 1);
		put_u32(&mut image, resource + 24 + 20, 0x8000_0030);
		put_u16(&mut image, resource + 48 + 14, 1);
		put_u32(&mut image, resource + 48 + 16, 0x0409);
		put_u32(&mut image, resource + 48 + 20, 72);
		put_u32(&mut image, resource + 72, 0x1000 + resource_data_offset);
		put_u32(
			&mut image,
			resource + 76,
			u32::try_from(version_resource.len()).unwrap_or(0),
		);
		let data = resource + usize::try_from(resource_data_offset).unwrap_or(88);
		image[data..data + version_resource.len()].copy_from_slice(&version_resource);
		image
	}

	fn version_resource([major, minor, patch, build]: [u16; 4]) -> Vec<u8> {
		let mut resource = vec![0_u8; 92];
		put_u16(&mut resource, 0, 92);
		put_u16(&mut resource, 2, 52);
		for (index, word) in "VS_VERSION_INFO\0".encode_utf16().enumerate() {
			put_u16(&mut resource, 6 + index * 2, word);
		}
		let fixed = 40;
		put_u32(&mut resource, fixed, 0xfeef_04bd);
		put_u32(&mut resource, fixed + 4, 0x0001_0000);
		put_u32(&mut resource, fixed + 8, u32::from(major) << 16 | u32::from(minor));
		put_u32(&mut resource, fixed + 12, u32::from(patch) << 16 | u32::from(build));
		put_u32(&mut resource, fixed + 16, u32::from(major) << 16 | u32::from(minor));
		put_u32(&mut resource, fixed + 20, u32::from(patch) << 16 | u32::from(build));
		resource
	}

	fn put_u16(bytes: &mut [u8], offset: usize, value: u16) {
		bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
	}

	fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
		bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
	}
}
