use crate::profile::MAX_PROFILE_BYTES;
use crate::profile::decode;
use crate::profile::encode;
use crate::safe_fs::SafeDir;
use crate::safe_fs::read_bounded;
use application::ErrorMarker;
use application::execution::RetainedExecutionInis;
use domain::ProfileIniPurpose;
use domain::derive_profile_ini;
use domain::preserve_profile_ini_keys;
use domain::profile_archive_list;
use domain::profile_ini_valid;
use rootcause::Result;
use rootcause::prelude::ResultExt;
use rootcause::report;
use std::path::Path;
use std::path::PathBuf;
use tempfile::Builder;
use tempfile::TempDir;
use tokio_util::sync::CancellationToken;

const INI_FILES: [&str; 5] = [
	"Fallout.ini",
	"FalloutPrefs.ini",
	"FalloutCustom.ini",
	"GECKCustom.ini",
	"GECKPrefs.ini",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProfileIniInputs {
	pub(crate) files: Vec<(&'static str, Option<Vec<u8>>)>,
	pub(crate) archive_list: String,
	pub(crate) fallback: Option<Vec<u8>>,
}

impl ProfileIniInputs {
	pub(crate) fn read(
		profile: &SafeDir,
		game: &Path,
		cancellation: &CancellationToken,
	) -> Result<Self, ErrorMarker> {
		let mut files = Vec::new();
		let mut fallout_list = None;
		let mut custom_list = None;
		for name in INI_FILES {
			if cancellation.is_cancelled() {
				return Err(report!(ErrorMarker::operation_cancelled()));
			}
			if !profile.exists(name).context(ErrorMarker::environment_invalid(None))? {
				files.push((name, None));
				continue;
			}

			let bytes = read_bounded(
				profile,
				name,
				MAX_PROFILE_BYTES,
				ErrorMarker::environment_invalid(None),
				cancellation,
			)?;
			let (text, _) = decode(&bytes)?;
			if !profile_ini_valid(&text) {
				return Err(report!(ErrorMarker::environment_invalid(Some("profile_ini"))));
			}
			if name == "Fallout.ini" {
				fallout_list = profile_archive_list(&text).map(ToOwned::to_owned);
			}
			if name == "FalloutCustom.ini" {
				custom_list = profile_archive_list(&text).map(ToOwned::to_owned);
			}
			files.push((name, Some(bytes)));
		}

		let mut fallback = None;
		let archive_list = if let Some(list) = custom_list.or(fallout_list) {
			list
		} else {
			let game = SafeDir::open_absolute(game).context(ErrorMarker::game_install_invalid())?;
			let bytes = read_bounded(
				&game,
				"Fallout_default.ini",
				MAX_PROFILE_BYTES,
				ErrorMarker::game_install_invalid(),
				cancellation,
			)?;
			let (text, _) = decode(&bytes)?;
			if !profile_ini_valid(&text) {
				return Err(report!(ErrorMarker::game_install_invalid()));
			}
			let list = profile_archive_list(&text).unwrap_or_default().to_owned();
			fallback = Some(bytes);
			list
		};

		Ok(Self {
			files,
			archive_list,
			fallback,
		})
	}

	pub(crate) fn derive(
		&self,
		name: &str,
		bytes: &[u8],
		purpose: ProfileIniPurpose,
	) -> Result<Vec<u8>, ErrorMarker> {
		let (text, encoding) = decode(bytes)?;
		encode(&derive_profile_ini(name, &text, purpose, &self.archive_list), encoding)
	}
}

/// Retains its backing files unless preservation and known process completion
/// explicitly succeed. Dropping this owner during uncertain drain never removes INIs.
pub struct ExecutionInis {
	directory: Option<TempDir>,
	path: PathBuf,
	canonical_directory: PathBuf,
	inputs: ProfileIniInputs,
	baseline: Vec<(&'static str, Option<Vec<u8>>)>,
}

impl ExecutionInis {
	pub(crate) fn create(
		profile: &Path,
		game: &Path,
		temp: &Path,
		cancellation: &CancellationToken,
	) -> Result<Self, ErrorMarker> {
		let canonical = SafeDir::open_absolute(profile).context(ErrorMarker::environment_invalid(None))?;
		let inputs = ProfileIniInputs::read(&canonical, game, cancellation)?;

		SafeDir::open_absolute(temp).context(ErrorMarker::environment_invalid(None))?;
		let directory = Builder::new()
			.prefix("execution-inis-")
			.tempdir_in(temp)
			.context(ErrorMarker::io_failure())?;

		let mut owner = Self {
			path: directory.path().to_owned(),
			directory: Some(directory),
			canonical_directory: profile.to_owned(),
			inputs,
			baseline: Vec::new(),
		};

		let staged = (|| -> Result<(), ErrorMarker> {
			let stage = SafeDir::open_absolute(owner.path()).context(ErrorMarker::io_failure())?;
			for (name, bytes) in &owner.inputs.files {
				if cancellation.is_cancelled() {
					return Err(report!(ErrorMarker::operation_cancelled()));
				}
				let derived = bytes
					.as_ref()
					.map(|bytes| owner.inputs.derive(name, bytes, ProfileIniPurpose::Execution))
					.transpose()?;
				if let Some(bytes) = &derived {
					stage.write_new(name, bytes).context(ErrorMarker::io_failure())?;
				}
				owner.baseline.push((name, derived));
			}
			Ok(())
		})();

		staged.map_err(|mut error| {
			error.children_mut().push(report!(RetainedExecutionInis {
				path: owner.path.clone()
			})
			.into_dynamic()
			.into_cloneable());
			error
		})?;
		Ok(owner)
	}

	pub fn path(&self) -> &Path {
		&self.path
	}

	pub(crate) fn revalidate(&self, game: &Path, cancellation: &CancellationToken) -> Result<(), ErrorMarker> {
		let profile = SafeDir::open_absolute(&self.canonical_directory).context(ErrorMarker::io_failure())?;
		if ProfileIniInputs::read(&profile, game, cancellation)? != self.inputs {
			return Err(report!(ErrorMarker::environment_invalid(Some("profile_changed"))));
		}
		Ok(())
	}

	pub(crate) fn directory(&self) -> Option<&TempDir> {
		self.directory.as_ref()
	}

	/// Call only after the entire managed Job is known to be empty, regardless of
	/// its exit status. Publication is ordered, with no rollback or retry.
	///
	/// # Errors
	/// Retains temporary files on concurrent canonical edits, child deletion,
	/// malformed edits, or any publication failure.
	pub fn preserve(mut self) -> Result<(), ErrorMarker> {
		self.preserve_inner().map_err(|mut error| {
			error.children_mut().push(report!(RetainedExecutionInis {
				path: self.path.clone()
			})
			.into_dynamic()
			.into_cloneable());
			error
		})
	}

	fn preserve_inner(&mut self) -> Result<(), ErrorMarker> {
		let cancellation = CancellationToken::new();
		let canonical = SafeDir::open_absolute(&self.canonical_directory).context(ErrorMarker::io_failure())?;
		let stage = SafeDir::open_absolute(self.path()).context(ErrorMarker::io_failure())?;
		let mut updates = Vec::new();
		for ((name, original), (_, baseline)) in self.inputs.files.iter().zip(&self.baseline) {
			let current = if canonical.exists(name).context(ErrorMarker::io_failure())? {
				Some(read_bounded(
					&canonical,
					name,
					MAX_PROFILE_BYTES,
					ErrorMarker::io_failure(),
					&cancellation,
				)?)
			} else {
				None
			};
			if current != *original {
				return Err(report!(ErrorMarker::environment_invalid(Some("profile_changed"))));
			}

			let child = if stage.exists(name).context(ErrorMarker::io_failure())? {
				Some(read_bounded(
					&stage,
					name,
					MAX_PROFILE_BYTES,
					ErrorMarker::io_failure(),
					&cancellation,
				)?)
			} else {
				None
			};
			if &child == baseline {
				continue;
			}
			let Some(child) = child else {
				return Err(report!(ErrorMarker::environment_invalid(Some("profile_deleted"))));
			};
			let (text, encoding) = decode(&child)?;
			if !profile_ini_valid(&text) {
				return Err(report!(ErrorMarker::environment_invalid(Some("profile_ini"))));
			}
			let original_text = original
				.as_ref()
				.map(|bytes| decode(bytes).map(|(text, _)| text))
				.transpose()?
				.unwrap_or_default();
			let preserved = encode(&preserve_profile_ini_keys(&original_text, &text), encoding)?;
			updates.push((*name, preserved));
		}

		let publication = stage.create_dir("preserved").context(ErrorMarker::io_failure())?;
		for (name, bytes) in &updates {
			publication.write_new(name, bytes).context(ErrorMarker::io_failure())?;
		}
		for (name, _) in &updates {
			#[cfg(test)]
			tests::before_canonical_rename(name).context(ErrorMarker::io_failure())?;
			publication
				.rename_durable_to(name, &canonical, name)
				.context(ErrorMarker::io_failure())?;
		}

		drop(publication);
		drop(stage);
		if let Some(directory) = self.directory.take() {
			directory.close().context(ErrorMarker::io_failure())?;
		}
		Ok(())
	}
}

impl Drop for ExecutionInis {
	fn drop(&mut self) {
		if let Some(directory) = self.directory.take() {
			let _ = directory.keep();
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::cell::Cell;
	use std::fs;
	use std::io;

	thread_local! {
		static FAIL_RENAME: Cell<Option<&'static str>> = const { Cell::new(None) };
		static RENAME_ATTEMPTS: Cell<usize> = const { Cell::new(0) };
	}

	pub(super) fn before_canonical_rename(name: &str) -> Result<(), io::Error> {
		RENAME_ATTEMPTS.with(|attempts| attempts.set(attempts.get() + 1));
		if FAIL_RENAME.with(|failure| failure.get() == Some(name)) {
			FAIL_RENAME.with(|failure| failure.set(None));
			return Err(report!(io::Error::new(
				io::ErrorKind::PermissionDenied,
				"injected canonical rename failure"
			)));
		}
		Ok(())
	}

	#[test]
	fn later_publication_failure_keeps_earlier_edit_and_retains_remaining_inis() -> Result<(), ErrorMarker> {
		let (_temp, profile, game, staging) = fixture()?;
		let original_prefs = b"[Display]\nvalue=original\n[Archive]\nsArchiveList=Prefs.bsa\n";
		fs::write(profile.join("FalloutPrefs.ini"), original_prefs).context(ErrorMarker::io_failure())?;
		let owner = ExecutionInis::create(&profile, &game, &staging, &CancellationToken::new())?;
		let path = owner.path().to_owned();
		for name in ["Fallout.ini", "FalloutPrefs.ini"] {
			let child = fs::read_to_string(path.join(name)).context(ErrorMarker::io_failure())?;
			fs::write(path.join(name), child.replace("value=original", "value=child"))
				.context(ErrorMarker::io_failure())?;
		}

		RENAME_ATTEMPTS.with(|attempts| attempts.set(0));
		FAIL_RENAME.with(|failure| failure.set(Some("FalloutPrefs.ini")));
		let Err(error) = owner.preserve() else {
			return Err(report!(ErrorMarker::io_failure()));
		};

		assert_eq!(RENAME_ATTEMPTS.with(Cell::get), 2);
		let retained = error
			.iter_reports()
			.find_map(|cause| cause.downcast_current_context::<RetainedExecutionInis>())
			.ok_or_else(|| report!(ErrorMarker::io_failure()))?;
		assert_eq!(retained.path, path);
		assert!(path.is_dir());
		let fallout = fs::read_to_string(profile.join("Fallout.ini")).context(ErrorMarker::io_failure())?;
		assert!(fallout.contains("value=child"));
		assert!(fallout.contains("SLocalSavePath=Saves\\"));
		assert!(fallout.contains("sArchiveList=Original.bsa"));
		assert!(!fallout.contains("__mods_saves"));
		assert!(!fallout.contains("bInvalidateOlderFiles"));
		assert!(!fallout.contains("SInvalidationFile"));
		assert_eq!(
			fs::read(profile.join("FalloutPrefs.ini")).context(ErrorMarker::io_failure())?,
			original_prefs
		);
		assert!(path.join("preserved/FalloutPrefs.ini").is_file());
		assert!(!path.join("preserved/Fallout.ini").exists());
		assert!(error.iter_reports().any(|cause| cause
			.downcast_current_context::<io::Error>()
			.is_some_and(|error| error.kind() == io::ErrorKind::PermissionDenied)));
		Ok(())
	}

	fn fixture() -> Result<(TempDir, PathBuf, PathBuf, PathBuf), ErrorMarker> {
		let temp = TempDir::new().context(ErrorMarker::io_failure())?;
		let root = temp.path().canonicalize().context(ErrorMarker::io_failure())?;
		let profile = root.join("profile");
		let game = root.join("game");
		let staging = root.join("temp");
		for path in [&profile, &game, &staging] {
			fs::create_dir(path).context(ErrorMarker::io_failure())?;
		}
		fs::write(profile.join("Fallout.ini"), b"[General]\r\nbUseMyGamesDirectory=1\r\nSLocalSavePath=Saves\\\r\n[Display]\r\nvalue=original\r\n[Archive]\r\nsArchiveList=Original.bsa\r\n").context(ErrorMarker::io_failure())?;
		Ok((temp, profile, game, staging))
	}

	#[test]
	fn optional_creation_and_invalid_child_edits_follow_preservation_policy() -> Result<(), ErrorMarker> {
		let (_temp, profile, game, staging) = fixture()?;
		let owner = ExecutionInis::create(&profile, &game, &staging, &CancellationToken::new())?;
		fs::write(
			owner.path().join("FalloutCustom.ini"),
			b"[Display]\ncreated=yes\n[Archive]\nsArchiveList=injected\n",
		)
		.context(ErrorMarker::io_failure())?;
		owner.preserve()?;
		let created =
			fs::read_to_string(profile.join("FalloutCustom.ini")).context(ErrorMarker::io_failure())?;
		assert!(created.contains("created=yes"));
		assert!(!created.contains("sArchiveList"));
		let owner = ExecutionInis::create(&profile, &game, &staging, &CancellationToken::new())?;
		let retained = owner.path().to_owned();
		fs::write(retained.join("Fallout.ini"), b"[malformed").context(ErrorMarker::io_failure())?;
		assert!(owner.preserve().is_err());
		assert!(retained.exists());
		Ok(())
	}

	#[test]
	fn derivation_preserves_encoding_and_newline_style() -> Result<(), ErrorMarker> {
		let (_temp, profile, game, staging) = fixture()?;
		let text = "[General]\r\nbUseMyGamesDirectory=1\r\nSLocalSavePath=Saves\\\r\n[Archive]\r\nsArchiveList=Original.bsa\r\n";
		let mut bytes = vec![0xff, 0xfe];
		for value in text.encode_utf16() {
			bytes.extend_from_slice(&value.to_le_bytes());
		}
		fs::write(profile.join("Fallout.ini"), &bytes).context(ErrorMarker::io_failure())?;
		let owner = ExecutionInis::create(&profile, &game, &staging, &CancellationToken::new())?;
		let derived = fs::read(owner.path().join("Fallout.ini")).context(ErrorMarker::io_failure())?;
		assert!(derived.starts_with(&[0xff, 0xfe]));
		let (derived, _) = decode(&derived)?;
		assert!(!derived.replace("\r\n", "").contains('\n'));
		owner.preserve()?;
		assert_eq!(
			fs::read(profile.join("Fallout.ini")).context(ErrorMarker::io_failure())?,
			bytes
		);
		Ok(())
	}

	#[test]
	fn stopped_child_edits_preserve_keys_and_absent_optional_files() -> Result<(), ErrorMarker> {
		let (_temp, profile, game, staging) = fixture()?;
		let owner = ExecutionInis::create(&profile, &game, &staging, &CancellationToken::new())?;
		let path = owner.path().to_owned();
		let ini = path.join("Fallout.ini");
		let derived = fs::read_to_string(&ini).context(ErrorMarker::io_failure())?;
		assert!(derived.contains("__mods_saves"));
		assert!(derived.contains("Fallout - Invalidation.bsa"));
		assert!(!path.join("FalloutCustom.ini").exists());
		fs::write(&ini, derived.replace("value=original", "value=child")).context(ErrorMarker::io_failure())?;
		owner.preserve()?;
		let canonical = fs::read_to_string(profile.join("Fallout.ini")).context(ErrorMarker::io_failure())?;
		assert!(canonical.contains("value=child"));
		assert!(canonical.contains("SLocalSavePath=Saves\\"));
		assert!(!canonical.contains("Invalidation.bsa"));
		assert!(!profile.join("FalloutCustom.ini").exists());
		assert!(!path.exists());
		Ok(())
	}

	#[test]
	fn uncertain_drain_deletion_and_concurrent_edits_retain_temporary_files() -> Result<(), ErrorMarker> {
		for failure in ["uncertain", "deleted", "concurrent"] {
			let (_temp, profile, game, staging) = fixture()?;
			let owner = ExecutionInis::create(&profile, &game, &staging, &CancellationToken::new())?;
			let path = owner.path().to_owned();
			if failure == "uncertain" {
				drop(owner);
			} else {
				if failure == "deleted" {
					fs::remove_file(path.join("Fallout.ini")).context(ErrorMarker::io_failure())?;
				} else {
					fs::write(profile.join("Fallout.ini"), b"concurrent")
						.context(ErrorMarker::io_failure())?;
				}
				assert!(owner.preserve().is_err());
			}
			assert!(path.exists());
		}
		Ok(())
	}

	#[test]
	fn explicit_empty_precedence_and_required_fallback_are_revalidated() -> Result<(), ErrorMarker> {
		let (_temp, profile, game, staging) = fixture()?;
		fs::write(profile.join("FalloutCustom.ini"), b"[Archive]\nsArchiveList=\n")
			.context(ErrorMarker::io_failure())?;
		let owner = ExecutionInis::create(&profile, &game, &staging, &CancellationToken::new())?;
		assert_eq!(owner.inputs.archive_list, "");
		assert!(owner.inputs.fallback.is_none());
		owner.preserve()?;
		fs::remove_file(profile.join("FalloutCustom.ini")).context(ErrorMarker::io_failure())?;
		fs::write(
			profile.join("Fallout.ini"),
			b"[General]\nSLocalSavePath=Saves\\\nbUseMyGamesDirectory=1\n",
		)
		.context(ErrorMarker::io_failure())?;
		assert!(ExecutionInis::create(&profile, &game, &staging, &CancellationToken::new()).is_err());
		fs::write(
			game.join("Fallout_default.ini"),
			b"[Archive]\nsArchiveList=Default.bsa\n",
		)
		.context(ErrorMarker::io_failure())?;
		let owner = ExecutionInis::create(&profile, &game, &staging, &CancellationToken::new())?;
		assert_eq!(owner.inputs.archive_list, "Default.bsa");
		fs::write(
			game.join("Fallout_default.ini"),
			b"[Archive]\nsArchiveList=Changed.bsa\n",
		)
		.context(ErrorMarker::io_failure())?;
		assert!(owner.revalidate(&game, &CancellationToken::new()).is_err());
		Ok(())
	}
}
