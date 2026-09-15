use std::error::Error;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use tempfile::TempDir;

fn game_fixture(base: &Path, name: &str, build: u64) -> Result<PathBuf, Box<dyn Error>> {
	let game = base.join(name).join("steamapps/common/Fallout New Vegas");
	fs::create_dir_all(game.join("Data"))?;
	fs::write(game.join("FalloutNV.exe"), b"fixture executable")?;
	fs::write(
		game.join("Fallout_default.ini"),
		b"[Archive]\r\nsArchiveList=Fallout - Meshes.bsa\r\n",
	)?;
	fs::write(
		game.parent()
			.and_then(Path::parent)
			.ok_or("missing steamapps")?
			.join("appmanifest_22380.acf"),
		format!(
			concat!(
				"\"AppState\"\n{{\n\"appid\" \"22380\"\n",
				"\"buildid\" \"{}\"\n",
				"\"installdir\" \"Fallout New Vegas\"\n}}\n",
			),
			build,
		),
	)?;
	Ok(game)
}

fn mods() -> Command {
	let mut command = Command::new(env!("CARGO_BIN_EXE_mods"));
	command.env_remove("MODS_GAME_DIR");
	command
}

#[test]
fn explicit_root_initializes_then_lists_exact_settings() -> Result<(), Box<dyn Error>> {
	let temp = TempDir::new()?;
	let base = fs::canonicalize(temp.path())?;
	let game = game_fixture(&base, "steam", 777)?;
	let root = base.join("environment");
	let initialized = mods()
		.args(["--log-level", "off", "--environment"])
		.arg(&root)
		.args(["init", "--game-install"])
		.arg(&game)
		.output()?;
	assert!(
		initialized.status.success(),
		"{}",
		String::from_utf8_lossy(&initialized.stderr)
	);
	assert!(initialized.stdout.is_empty());
	assert!(initialized.stderr.is_empty());
	assert!(root.join("mods.toml").is_file());
	assert!(!root.join("logs").exists());
	let listed = mods()
		.args(["--log-level", "off", "--environment"])
		.arg(&root)
		.args(["config", "list"])
		.output()?;
	assert!(listed.status.success(), "{}", String::from_utf8_lossy(&listed.stderr));
	let stdout = String::from_utf8(listed.stdout)?;
	let keys = [
		"schema-version =",
		"name =",
		"steam-app-id =",
		"game-dir =",
		"observed-build-id =",
	];
	let positions = keys.map(|key| stdout.find(key).ok_or("missing setting"));
	assert!(positions
		.into_iter()
		.collect::<Result<Vec<_>, _>>()?
		.windows(2)
		.all(|pair| pair[0] < pair[1]));
	Ok(())
}

#[test]
fn default_root_and_explicit_game_override_environment_value() -> Result<(), Box<dyn Error>> {
	let temp = TempDir::new()?;
	let canonical_temp = temp.path().canonicalize()?;
	let explicit = game_fixture(&canonical_temp, "explicit", 111)?;
	let shadowed = game_fixture(&canonical_temp, "shadowed", 222)?;
	let initialized = mods()
		.current_dir(&canonical_temp)
		.env("LOCALAPPDATA", &canonical_temp)
		.env("MODS_GAME_DIR", &shadowed)
		.args(["--log-level", "off", "init", "--game-install"])
		.arg(&explicit)
		.output()?;
	assert!(
		initialized.status.success(),
		"{}",
		String::from_utf8_lossy(&initialized.stderr)
	);
	assert!(initialized.stdout.is_empty());
	assert!(initialized.stderr.is_empty());
	let manifest = fs::read_to_string(canonical_temp.join("mods/environments/default/mods.toml"))?;
	assert!(manifest.contains("observed_build_id = 111"));
	assert!(!manifest.contains("observed_build_id = 222"));
	Ok(())
}

#[test]
fn shadowed_set_writes_manifest_and_reports_invalid_effective_binding() -> Result<(), Box<dyn Error>> {
	let temp = TempDir::new()?;
	let base = fs::canonicalize(temp.path())?;
	let build_a = game_fixture(&base, "build-a", 100)?;
	let build_b = game_fixture(&base, "build-b", 200)?;
	let root = base.join("environment");
	let initialized = mods()
		.args(["--log-level", "off", "--environment"])
		.arg(&root)
		.args(["init", "--game-install"])
		.arg(&build_a)
		.output()?;
	assert!(
		initialized.status.success(),
		"{}",
		String::from_utf8_lossy(&initialized.stderr)
	);
	assert!(initialized.stdout.is_empty());
	assert!(initialized.stderr.is_empty());

	let updated = mods()
		.env("MODS_GAME_DIR", &build_a)
		.args(["--log-level", "off", "--environment"])
		.arg(&root)
		.args(["config", "set", "game-dir"])
		.arg(&build_b)
		.output()?;
	assert!(updated.status.success(), "{}", String::from_utf8_lossy(&updated.stderr));
	assert!(updated.stdout.is_empty());
	let stderr = String::from_utf8(updated.stderr)?;
	assert!(stderr.contains("warning: MODS_GAME_DIR build does not match observed-build-id"));
	let manifest = fs::read_to_string(root.join("mods.toml"))?;
	assert!(manifest.contains("observed_build_id = 200"));
	assert!(root.join("temp").is_dir());
	Ok(())
}

#[test]
fn failed_command_references_a_durable_diagnostic_session() -> Result<(), Box<dyn Error>> {
	let temp = TempDir::new()?;
	let base = fs::canonicalize(temp.path())?;
	let root = base.join("missing-environment");
	let output = mods()
		.args(["--environment"])
		.arg(&root)
		.args(["config", "list"])
		.output()?;
	assert!(!output.status.success());
	let stderr = String::from_utf8(output.stderr)?;
	assert!(stderr.contains("environment_not_initialized"));
	assert!(stderr.contains("diagnostic session:"));
	let files = fs::read_dir(root.join("logs"))?.collect::<Result<Vec<_>, _>>()?;
	assert_eq!(files.len(), 1);
	let text = fs::read_to_string(files[0].path())?;
	assert!(text.contains("session.started"));
	assert!(text.contains("session.failed"));
	Ok(())
}
