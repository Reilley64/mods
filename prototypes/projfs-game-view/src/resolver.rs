use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Component, Path, PathBuf};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Resolved {
    Physical(PathBuf),
    SyntheticData,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Tombstone {
    pub path: PathBuf,
    pub directory: bool,
}

#[derive(Clone, Debug)]
pub struct Resolver {
    pub base: PathBuf,
    pub mods: Vec<PathBuf>,
    pub overwrite: PathBuf,
}

pub fn normalize_relative(path: &Path) -> Result<PathBuf, String> {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => out.push(part),
            _ => {
                return Err(format!(
                    "path is not a plain relative path: {}",
                    path.display()
                ));
            }
        }
    }
    Ok(out)
}

pub fn names_equal(left: &OsStr, right: &OsStr) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows::Win32::Storage::ProjectedFileSystem::PrjFileNameCompare;
        use windows::core::PCWSTR;
        let left: Vec<_> = left.encode_wide().chain(Some(0)).collect();
        let right: Vec<_> = right.encode_wide().chain(Some(0)).collect();
        unsafe { PrjFileNameCompare(PCWSTR(left.as_ptr()), PCWSTR(right.as_ptr())) == 0 }
    }
    #[cfg(not(windows))]
    {
        left.to_string_lossy()
            .eq_ignore_ascii_case(&right.to_string_lossy())
    }
}

pub fn paths_equal(left: &Path, right: &Path) -> bool {
    let left: Vec<_> = left.components().collect();
    let right: Vec<_> = right.components().collect();
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|(left, right)| match (left, right) {
                (Component::Normal(left), Component::Normal(right)) => names_equal(left, right),
                _ => false,
            })
}

fn path_is_descendant(path: &Path, ancestor: &Path) -> bool {
    let path: Vec<_> = path.components().collect();
    let ancestor: Vec<_> = ancestor.components().collect();
    path.len() > ancestor.len()
        && ancestor
            .iter()
            .zip(path)
            .all(|(ancestor, path)| match (ancestor, path) {
                (Component::Normal(ancestor), Component::Normal(path)) => {
                    names_equal(ancestor, path)
                }
                _ => false,
            })
}

#[cfg_attr(not(windows), allow(dead_code))]
pub fn fold_path(path: &Path) -> String {
    path.components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part.to_string_lossy().to_lowercase()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\\")
}

pub fn is_beneath_data(path: &Path) -> bool {
    let parts: Vec<_> = path.components().collect();
    parts.len() >= 2
        && matches!(parts.first(), Some(Component::Normal(first)) if first.to_string_lossy().eq_ignore_ascii_case("Data"))
}

pub fn is_data_root(path: &Path) -> bool {
    let parts: Vec<_> = path.components().collect();
    parts.len() == 1
        && matches!(parts.first(), Some(Component::Normal(first)) if first.to_string_lossy().eq_ignore_ascii_case("Data"))
}

pub fn hidden_by_tombstone(path: &Path, tombstones: &BTreeMap<String, Tombstone>) -> bool {
    tombstones.values().any(|tombstone| {
        paths_equal(path, &tombstone.path)
            || (tombstone.directory && path_is_descendant(path, &tombstone.path))
    })
}

pub fn find_case_insensitive(root: &Path, relative: &Path) -> Option<PathBuf> {
    let mut current = root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(wanted) = component else {
            return None;
        };
        let entries = fs::read_dir(&current).ok()?;
        let matched = entries
            .filter_map(Result::ok)
            .find(|entry| names_equal(&entry.file_name(), wanted))?;
        current = matched.path();
    }
    current
        .try_exists()
        .ok()
        .filter(|exists| *exists)
        .map(|_| current)
}

impl Resolver {
    fn data_layers_high_to_low(&self) -> Vec<PathBuf> {
        let mut roots = vec![self.overwrite.clone()];
        roots.extend(self.mods.iter().rev().cloned());
        roots.push(self.base.join("Data"));
        roots
    }

    fn data_subpath(path: &Path) -> Option<PathBuf> {
        let mut components = path.components();
        match components.next() {
            Some(Component::Normal(first))
                if first.to_string_lossy().eq_ignore_ascii_case("Data") => {}
            _ => return None,
        }
        Some(components.collect())
    }

    fn winner_at_data_subpath(&self, subpath: &Path) -> Option<PathBuf> {
        self.data_layers_high_to_low()
            .into_iter()
            .find_map(|root| find_case_insensitive(&root, subpath))
    }

    pub fn resolve(
        &self,
        relative: &Path,
        tombstones: &BTreeMap<String, Tombstone>,
    ) -> Option<Resolved> {
        let relative = normalize_relative(relative).ok()?;
        if relative.as_os_str().is_empty() {
            return Some(Resolved::Physical(self.base.clone()));
        }
        if is_data_root(&relative) {
            return Some(Resolved::SyntheticData);
        }
        if let Some(subpath) = Self::data_subpath(&relative) {
            if hidden_by_tombstone(&relative, tombstones) {
                return None;
            }
            let mut prefix = PathBuf::new();
            let components: Vec<_> = subpath.components().collect();
            for (index, component) in components.iter().enumerate() {
                prefix.push(component.as_os_str());
                if hidden_by_tombstone(&Path::new("Data").join(&prefix), tombstones) {
                    return None;
                }
                let winner = self.winner_at_data_subpath(&prefix)?;
                if index + 1 != components.len() && !winner.is_dir() {
                    return None;
                }
                if index + 1 == components.len() {
                    return Some(Resolved::Physical(winner));
                }
            }
            return Some(Resolved::SyntheticData);
        }
        find_case_insensitive(&self.base, &relative).map(Resolved::Physical)
    }

    pub fn view_relative(
        &self,
        relative: &Path,
        tombstones: &BTreeMap<String, Tombstone>,
    ) -> Option<PathBuf> {
        let relative = normalize_relative(relative).ok()?;
        if relative.as_os_str().is_empty() {
            return Some(PathBuf::new());
        }
        if is_data_root(&relative) {
            return Some(PathBuf::from("Data"));
        }
        if let Some(subpath) = Self::data_subpath(&relative) {
            let mut prefix = PathBuf::new();
            let mut result = PathBuf::from("Data");
            for component in subpath.components() {
                let Component::Normal(component) = component else {
                    return None;
                };
                prefix.push(component);
                let view_prefix = Path::new("Data").join(&prefix);
                if hidden_by_tombstone(&view_prefix, tombstones) {
                    return None;
                }
                let winner = self.winner_at_data_subpath(&prefix)?;
                result.push(winner.file_name()?);
                if prefix != subpath && !winner.is_dir() {
                    return None;
                }
            }
            return Some(result);
        }
        let physical = find_case_insensitive(&self.base, &relative)?;
        physical
            .strip_prefix(&self.base)
            .ok()
            .map(Path::to_path_buf)
    }

    pub fn enumerate(
        &self,
        relative: &Path,
        tombstones: &BTreeMap<String, Tombstone>,
    ) -> Result<Vec<(OsString, Resolved)>, String> {
        let relative = normalize_relative(relative)?;
        let resolved = self
            .resolve(&relative, tombstones)
            .ok_or_else(|| format!("not found: {}", relative.display()))?;
        if matches!(&resolved, Resolved::Physical(path) if !path.is_dir()) {
            return Err(format!("not a directory: {}", relative.display()));
        }

        if relative.as_os_str().is_empty() {
            let mut names = Vec::<OsString>::new();
            for entry in fs::read_dir(&self.base).map_err(|error| error.to_string())? {
                names.push(entry.map_err(|error| error.to_string())?.file_name());
            }
            if !names
                .iter()
                .any(|name| names_equal(name, OsStr::new("Data")))
            {
                names.push(OsString::from("Data"));
            }
            return Ok(names
                .into_iter()
                .filter_map(|name| {
                    let child = PathBuf::from(&name);
                    self.resolve(&child, tombstones).map(|item| (name, item))
                })
                .collect());
        }

        if Self::data_subpath(&relative).is_some() {
            let subdir = Self::data_subpath(&relative).expect("checked above");
            let mut candidates = Vec::<OsString>::new();
            for root in self.data_layers_high_to_low() {
                let Some(directory) = find_case_insensitive(&root, &subdir) else {
                    continue;
                };
                if !directory.is_dir() {
                    continue;
                }
                for entry in fs::read_dir(directory).map_err(|error| error.to_string())? {
                    let name = entry.map_err(|error| error.to_string())?.file_name();
                    if !candidates
                        .iter()
                        .any(|existing| names_equal(existing, &name))
                    {
                        candidates.push(name);
                    }
                }
            }
            return Ok(candidates
                .into_iter()
                .filter_map(|candidate| {
                    let child = relative.join(&candidate);
                    match self.resolve(&child, tombstones) {
                        Some(Resolved::Physical(path)) => {
                            let name = path.file_name()?.to_os_string();
                            Some((name, Resolved::Physical(path)))
                        }
                        Some(Resolved::SyntheticData) => Some((candidate, Resolved::SyntheticData)),
                        None => None,
                    }
                })
                .collect());
        }

        let Resolved::Physical(directory) = resolved else {
            return Err("synthetic path was not Data".into());
        };
        fs::read_dir(directory)
            .map_err(|error| error.to_string())?
            .map(|entry| {
                let entry = entry.map_err(|error| error.to_string())?;
                let name = entry.file_name();
                Ok((name, Resolved::Physical(entry.path())))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let root = std::env::temp_dir()
                .join(format!("projfs-resolver-{}-{nonce}", std::process::id()));
            fs::create_dir_all(&root).unwrap();
            Self(root)
        }
        fn write(&self, relative: &str, value: &str) {
            let path = self.0.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, value).unwrap();
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn priority_case_insensitivity_nested_merge_and_collisions() {
        let f = Fixture::new();
        f.write("base/Data/common.txt", "base");
        f.write("base/Data/high-order.txt", "base high order");
        f.write("base/Data/low-order.txt", "base low order");
        f.write("base/Data/nested/base.txt", "base nested");
        f.write("low/common.txt", "low");
        f.write("low/high-order.txt", "low high order");
        f.write("low/low-order.txt", "low wins");
        f.write("low/nested/low.txt", "low nested");
        f.write("high/COMMON.TXT", "high");
        f.write("high/HIGH-ORDER.TXT", "high wins");
        f.write("high/nested/high.txt", "high nested");
        f.write("overwrite/common.txt", "overwrite");
        f.write("base/Data/collision/hidden.txt", "hidden");
        f.write("high/collision", "file wins");
        let r = Resolver {
            base: f.0.join("base"),
            mods: vec![f.0.join("low"), f.0.join("high")],
            overwrite: f.0.join("overwrite"),
        };
        let tombstones = BTreeMap::new();

        let Resolved::Physical(winner) = r
            .resolve(Path::new("dAtA/COMMON.txt"), &tombstones)
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(fs::read_to_string(winner).unwrap(), "overwrite");
        assert_eq!(
            fs::read_to_string(
                match r
                    .resolve(Path::new("Data/high-order.txt"), &tombstones)
                    .unwrap()
                {
                    Resolved::Physical(path) => path,
                    Resolved::SyntheticData => panic!(),
                }
            )
            .unwrap(),
            "high wins"
        );
        assert_eq!(
            fs::read_to_string(
                match r
                    .resolve(Path::new("Data/low-order.txt"), &tombstones)
                    .unwrap()
                {
                    Resolved::Physical(path) => path,
                    Resolved::SyntheticData => panic!(),
                }
            )
            .unwrap(),
            "low wins"
        );
        assert_eq!(
            r.view_relative(Path::new("dAtA/COMMON.txt"), &tombstones),
            Some(PathBuf::from("Data/common.txt"))
        );
        let nested: BTreeSet<_> = r
            .enumerate(Path::new("Data/nested"), &tombstones)
            .unwrap()
            .into_iter()
            .map(|(n, _)| n.to_string_lossy().to_lowercase())
            .collect();
        assert_eq!(
            nested,
            BTreeSet::from(["base.txt".into(), "high.txt".into(), "low.txt".into()])
        );
        let Resolved::Physical(collision) =
            r.resolve(Path::new("Data/collision"), &tombstones).unwrap()
        else {
            panic!()
        };
        assert!(collision.is_file());
        assert!(
            r.resolve(Path::new("Data/collision/hidden.txt"), &tombstones)
                .is_none()
        );
    }

    #[test]
    fn tombstones_hide_exact_paths_and_directory_descendants() {
        let f = Fixture::new();
        f.write("base/Data/file.txt", "file");
        f.write("base/Data/gone/child.txt", "child");
        let r = Resolver {
            base: f.0.join("base"),
            mods: vec![],
            overwrite: f.0.join("overwrite"),
        };
        let tombstones = BTreeMap::from([
            (
                "data\\file.txt".into(),
                Tombstone {
                    path: PathBuf::from("Data/file.txt"),
                    directory: false,
                },
            ),
            (
                "data\\gone".into(),
                Tombstone {
                    path: PathBuf::from("Data/gone"),
                    directory: true,
                },
            ),
        ]);
        assert!(r.resolve(Path::new("DATA/File.TXT"), &tombstones).is_none());
        assert!(
            r.resolve(Path::new("Data/gone/child.txt"), &tombstones)
                .is_none()
        );
        assert!(
            r.enumerate(Path::new("Data"), &tombstones)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn rejects_non_relative_mutation_paths() {
        assert!(!is_beneath_data(Path::new("Data")));
        assert!(!is_beneath_data(Path::new("other/file")));
        assert!(normalize_relative(Path::new("Data/../escape")).is_err());
        assert!(is_beneath_data(Path::new("DATA/file")));
    }
}
