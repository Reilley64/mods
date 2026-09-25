use sha2::Digest;
use sha2::Sha256;
use std::env;
use std::error::Error;
use std::fs;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn Error>> {
	println!("cargo:rerun-if-env-changed=MODS_USVFS_ARTIFACTS");
	if env::var("CARGO_CFG_TARGET_OS")? != "windows" {
		return Ok(());
	}
	let target = env::var("TARGET")?;
	if target != "x86_64-pc-windows-msvc" && target != "i686-pc-windows-msvc" {
		return Err("usvfs supports only x86/x64 MSVC targets".into());
	}
	let output = PathBuf::from(env::var("OUT_DIR")?);

	// Hash the exact clean-build bundle used by packaging, not a mutable version string.
	let artifacts = PathBuf::from(env::var("MODS_USVFS_ARTIFACTS")?);
	println!("cargo:rustc-env=MODS_USVFS_ARTIFACTS={}", artifacts.display());
	println!(
		"cargo:rerun-if-changed={}",
		artifacts.join("source-revision.txt").display()
	);
	if fs::read_to_string(artifacts.join("source-revision.txt"))?.trim()
		!= "57f1ea5e6ad13f7435a7af184748e6c1312c5637"
	{
		return Err("artifact source revision does not match pinned headers".into());
	}
	let mut hashes = String::from("pub const ARTIFACTS: &[(&str, [u8; 32])] = &[\n");
	for name in [
		"usvfs_x86.dll",
		"usvfs_x64.dll",
		"usvfs_proxy_x86.exe",
		"usvfs_proxy_x64.exe",
	] {
		let path = artifacts.join(name);
		println!("cargo:rerun-if-changed={}", path.display());
		let digest: [u8; 32] = Sha256::digest(fs::read(path)?).into();
		hashes.push_str(&format!("({name:?}, {digest:?}),\n"));
	}
	hashes.push_str("];\n");
	fs::write(output.join("artifacts.rs"), hashes)?;
	Ok(())
}
