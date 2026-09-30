use crate::DataRelativePath;
use crate::case_fold_key;
use std::cmp::Reverse;
use std::time::Duration;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

/// 2000-01-01T00:00:00Z, the time of the first load-order position.
const FIRST_POSITION: Duration = Duration::from_secs(946_684_800);
const POSITION_STEP: u64 = 60;

/// One Data winner with its current modification time. `file` is the caller's
/// handle for the file that gets the time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadOrderCandidate<T> {
	pub file: T,
	pub path: DataRelativePath,
	pub modified: SystemTime,
}

/// Sort key for plugins. Field order is the sort order: listed plugins first,
/// by `loadorder.txt` position, then unlisted ones by current time and name.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct PluginRank {
	unlisted: bool,
	position: Option<usize>,
	modified: SystemTime,
	key: String,
}

struct Plugin<T> {
	rank: PluginRank,
	stem: String,
	file: T,
}

/// Whether a Data path can get a load-order time: a plugin (`.esm`, `.esp`) or an
/// archive (`.bsa`) at the Data root, matched case-insensitively.
pub fn takes_load_order_time(path: &DataRelativePath) -> bool {
	path.components().count() == 1
		&& path.comparison_key()
			.rsplit_once('.')
			.is_some_and(|(_, extension)| matches!(extension, "esm" | "esp" | "bsa"))
}

/// Gives Data-root plugins and archives modification times that make the game
/// load them in profile order, and returns them in time order.
///
/// Positions start at 2000-01-01T00:00:00Z and add one minute each:
///
/// 1. Archives named in `archive_list`, in list order.
/// 2. Other archives that no plugin loads, in current-time order.
/// 3. Plugins (`.esm`, `.esp`) in `loadorder.txt` order, then plugins missing
///    from it in current-time order. `loadorder.txt` lists active and inactive
///    plugins, so every present plugin gets a time.
///
/// An archive whose stem starts with a plugin stem loads with that plugin and
/// gets its time; the longest matching stem wins. Names match case-insensitively.
/// Files outside the Data root and other file types get no time. A UTF-8 byte
/// order mark at the start of `load_order` is ignored.
pub fn load_order_times<T>(
	candidates: Vec<LoadOrderCandidate<T>>,
	load_order: &str,
	archive_list: &[&str],
) -> Vec<(T, SystemTime)> {
	let listed: Vec<_> = load_order
		.strip_prefix('\u{feff}')
		.unwrap_or(load_order)
		.lines()
		.filter(|line| !line.is_empty() && !line.starts_with('#'))
		.map(case_fold_key)
		.collect();
	let archive_list: Vec<_> = archive_list.iter().map(|name| case_fold_key(name)).collect();

	let mut plugins = Vec::new();
	let mut archives = Vec::new();
	for candidate in candidates
		.into_iter()
		.filter(|candidate| takes_load_order_time(&candidate.path))
	{
		let key = candidate.path.comparison_key().to_owned();
		match key.rsplit_once('.') {
			Some((stem, "esm" | "esp")) => {
				let position = listed.iter().position(|name| *name == key);
				plugins.push(Plugin {
					rank: PluginRank {
						unlisted: position.is_none(),
						position,
						modified: candidate.modified,
						key: key.clone(),
					},
					stem: stem.to_owned(),
					file: candidate.file,
				});
			}
			Some((stem, "bsa")) => archives.push((stem.to_owned(), key.clone(), candidate)),
			_ => {}
		}
	}

	plugins.sort_by(|left, right| left.rank.cmp(&right.rank));

	let mut named = Vec::new();
	let mut unloaded = Vec::new();
	let mut loaded = Vec::new();
	for (stem, key, candidate) in archives {
		let plugin = plugins
			.iter()
			.enumerate()
			.filter(|(_, plugin)| stem.starts_with(&plugin.stem))
			.min_by_key(|(_, plugin)| Reverse(plugin.stem.len()))
			.map(|(index, _)| index);

		match (archive_list.iter().position(|name| *name == key), plugin) {
			(Some(index), _) => named.push((index, candidate.file)),
			(None, Some(plugin)) => loaded.push((plugin, key, candidate.file)),
			(None, None) => unloaded.push(((candidate.modified, key), candidate.file)),
		}
	}

	named.sort_by_key(|(index, _)| *index);
	unloaded.sort_by(|left, right| left.0.cmp(&right.0));
	loaded.sort_by(|left, right| (left.0, &left.1).cmp(&(right.0, &right.1)));

	let first_plugin = named.len() + unloaded.len();
	let mut timed: Vec<_> = named
		.into_iter()
		.map(|(_, file)| file)
		.chain(unloaded.into_iter().map(|(_, file)| file))
		.chain(plugins.into_iter().map(|plugin| plugin.file))
		.enumerate()
		.map(|(position, file)| (file, position_time(position)))
		.collect();

	timed.extend(loaded
		.into_iter()
		.map(|(plugin, _, file)| (file, position_time(first_plugin + plugin))));
	// A stable sort keeps each plugin before the archives that share its time.
	timed.sort_by_key(|(_, time)| *time);
	timed
}

fn position_time(position: usize) -> SystemTime {
	UNIX_EPOCH + FIRST_POSITION + Duration::from_secs(POSITION_STEP * position as u64)
}

#[cfg(test)]
mod tests {
	use super::LoadOrderCandidate;
	use super::load_order_times;
	use crate::DataRelativePath;
	use crate::InvalidDataRelativePath;
	use rootcause::Result;
	use std::time::Duration;
	use std::time::SystemTime;
	use std::time::UNIX_EPOCH;

	/// Runs the ordering on files named with their current time in seconds, and
	/// returns each timed file with its position in minutes after the first one.
	fn positions(
		files: &[(&str, u64)],
		load_order: &str,
		archive_list: &[&str],
	) -> Result<Vec<(String, u64)>, InvalidDataRelativePath> {
		let candidates = files
			.iter()
			.map(|(path, current)| {
				Ok(LoadOrderCandidate {
					file: (*path).to_owned(),
					path: DataRelativePath::new((*path).to_owned())?,
					modified: UNIX_EPOCH + Duration::from_secs(*current),
				})
			})
			.collect::<Result<Vec<_>, InvalidDataRelativePath>>()?;

		let first = UNIX_EPOCH + Duration::from_secs(946_684_800);

		Ok(load_order_times(candidates, load_order, archive_list)
			.into_iter()
			.map(|(file, time): (String, SystemTime)| {
				let minutes = time
					.duration_since(first)
					.map_or(u64::MAX, |elapsed| elapsed.as_secs() / 60);
				(file, minutes)
			})
			.collect())
	}

	fn expected(files: &[(&str, u64)]) -> Vec<(String, u64)> {
		files.iter()
			.map(|(file, minutes)| ((*file).to_owned(), *minutes))
			.collect()
	}

	#[test]
	fn archive_list_archives_come_first_then_archives_without_a_plugin_by_current_time()
	-> Result<(), InvalidDataRelativePath> {
		let timed = positions(
			&[
				("Mod.esp", 1),
				("Late.bsa", 30),
				("Fallout - Misc.bsa", 5),
				("Early.bsa", 20),
				("Fallout - Textures.bsa", 9),
			],
			"Mod.esp\r\n",
			&[
				"Fallout - Invalidation.bsa",
				"fallout - textures.BSA",
				"Fallout - Misc.bsa",
			],
		)?;

		assert_eq!(
			timed,
			expected(&[
				("Fallout - Textures.bsa", 0),
				("Fallout - Misc.bsa", 1),
				("Early.bsa", 2),
				("Late.bsa", 3),
				("Mod.esp", 4),
			])
		);
		Ok(())
	}

	#[test]
	fn plugins_follow_load_order_then_unlisted_plugins_by_current_time() -> Result<(), InvalidDataRelativePath> {
		let timed = positions(
			&[
				("A.esp", 1),
				("B.esp", 2),
				("FalloutNV.esm", 3),
				("Late.esp", 20),
				("Early.esp", 10),
			],
			"# comment\r\n\r\nFalloutNV.esm\r\nb.ESP\r\nMissing.esp\r\nA.esp\r\n",
			&[],
		)?;

		assert_eq!(
			timed,
			expected(&[
				("FalloutNV.esm", 0),
				("B.esp", 1),
				("A.esp", 2),
				("Early.esp", 3),
				("Late.esp", 4),
			])
		);
		Ok(())
	}

	#[test]
	fn archives_take_the_time_of_the_plugin_with_the_longest_matching_stem() -> Result<(), InvalidDataRelativePath>
	{
		let timed = positions(
			&[
				("MOD EXTRA - Textures.bsa", 1),
				("mod - main.BSA", 2),
				("Mod Extra.esp", 3),
				("Mod.esp", 4),
				("Mod - Listed.bsa", 5),
			],
			"Mod.esp\r\nMod Extra.esp\r\n",
			&["mod - listed.bsa"],
		)?;

		assert_eq!(
			timed,
			expected(&[
				("Mod - Listed.bsa", 0),
				("Mod.esp", 1),
				("mod - main.BSA", 1),
				("Mod Extra.esp", 2),
				("MOD EXTRA - Textures.bsa", 2),
			])
		);
		Ok(())
	}

	#[test]
	fn only_data_root_plugins_and_archives_get_a_time() -> Result<(), InvalidDataRelativePath> {
		let timed = positions(
			&[
				("textures/Mod.bsa", 1),
				("Sub/Plugin.esp", 2),
				("Mod.ini", 3),
				("Mod.esp", 4),
			],
			"Mod.esp\r\nPlugin.esp\r\n",
			&["Mod.bsa"],
		)?;

		assert_eq!(timed, expected(&[("Mod.esp", 0)]));
		Ok(())
	}

	#[test]
	fn a_byte_order_mark_does_not_hide_the_first_listed_plugin() -> Result<(), InvalidDataRelativePath> {
		let timed = positions(
			&[("Mod.esp", 1), ("FalloutNV.esm", 2)],
			"\u{feff}FalloutNV.esm\r\nMod.esp\r\n",
			&[],
		)?;

		assert_eq!(timed, expected(&[("FalloutNV.esm", 0), ("Mod.esp", 1)]));
		Ok(())
	}
}
