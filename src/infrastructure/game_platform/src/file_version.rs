use application::ErrorMarker;
use cap_std::fs::File;
use pelite::PeFile;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use rootcause::report;
use std::error::Error;
use std::fmt;
use std::io::Read;

pub(super) fn read_file_version(mut file: File) -> Result<Vec<u32>, ErrorMarker> {
	let mut bytes = Vec::new();
	file.read_to_end(&mut bytes)
		.context(ErrorMarker::game_install_invalid())?;
	let image = PeFile::from_bytes(&bytes)
		.map_err(|error| report!(error).context(ErrorMarker::game_install_invalid()))?;
	let resources = image
		.resources()
		.map_err(|error| report!(error).context(ErrorMarker::game_install_invalid()))?;
	let version_info = resources
		.version_info()
		.map_err(|error| report!(error).context(ErrorMarker::game_install_invalid()))?;
	let fixed = version_info
		.fixed()
		.ok_or_else(|| report!(MalformedVersionResource).context(ErrorMarker::game_install_invalid()))?;
	Ok(vec![
		u32::from(fixed.dwFileVersion.Major),
		u32::from(fixed.dwFileVersion.Minor),
		u32::from(fixed.dwFileVersion.Patch),
		u32::from(fixed.dwFileVersion.Build),
	])
}

#[derive(Debug, Clone, Copy)]
struct MalformedVersionResource;

impl fmt::Display for MalformedVersionResource {
	fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
		formatter.write_str("executable version resource is malformed")
	}
}

impl Error for MalformedVersionResource {}

#[cfg(test)]
mod tests {
	use super::read_file_version;
	use application::ErrorCode;
	use cap_std::ambient_authority;
	use cap_std::fs::Dir;
	use rootcause::Result;
	use std::fs;
	use tempfile::TempDir;

	#[test]
	fn malformed_version_resource_is_typed() -> Result<()> {
		let temp = TempDir::new()?;
		fs::write(temp.path().join("FalloutNV.exe"), pe_with_version(None))?;
		let directory = Dir::open_ambient_dir(temp.path(), ambient_authority())?;
		let file = directory.open("FalloutNV.exe")?;

		let result = read_file_version(file);

		assert_eq!(
			result.as_ref().err().map(|error| error.current_context().code()),
			Some(ErrorCode::GameInstallInvalid),
		);
		Ok(())
	}

	fn pe_with_version(version: Option<[u16; 4]>) -> Vec<u8> {
		let version_resource = version_resource(version);
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

	fn version_resource(version: Option<[u16; 4]>) -> Vec<u8> {
		let length = if version.is_some() { 92 } else { 40 };
		let mut resource = vec![0_u8; length];
		put_u16(&mut resource, 0, u16::try_from(length).unwrap_or(0));
		put_u16(&mut resource, 2, if version.is_some() { 52 } else { 0 });
		for (index, word) in "VS_VERSION_INFO\0".encode_utf16().enumerate() {
			put_u16(&mut resource, 6 + index * 2, word);
		}
		if let Some([major, minor, patch, build]) = version {
			let fixed = 40;
			put_u32(&mut resource, fixed, 0xfeef_04bd);
			put_u32(&mut resource, fixed + 4, 0x0001_0000);
			put_u32(&mut resource, fixed + 8, u32::from(major) << 16 | u32::from(minor));
			put_u32(&mut resource, fixed + 12, u32::from(patch) << 16 | u32::from(build));
			put_u32(&mut resource, fixed + 16, u32::from(major) << 16 | u32::from(minor));
			put_u32(&mut resource, fixed + 20, u32::from(patch) << 16 | u32::from(build));
		}
		resource
	}

	fn put_u16(bytes: &mut [u8], offset: usize, value: u16) {
		bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
	}

	fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
		bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
	}
}
