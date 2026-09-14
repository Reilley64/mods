use crate::ServeArgs;
use crate::resolver::{
    Resolved, Resolver, Tombstone, find_case_insensitive, fold_path, is_beneath_data,
    normalize_relative, paths_equal,
};
use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap};
use std::ffi::{OsStr, OsString, c_void};
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom, Write};
use std::mem::size_of;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::MetadataExt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;
use windows::Win32::Foundation::{E_FAIL, E_INVALIDARG, ERROR_FILE_NOT_FOUND, S_OK};
use windows::Win32::Storage::FileSystem::{FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT};
use windows::Win32::Storage::ProjectedFileSystem::*;
use windows::core::{GUID, HRESULT, PCWSTR};

type HrResult<T> = Result<T, HRESULT>;

#[derive(Clone)]
struct DirectoryEntry {
    name: OsString,
    info: PRJ_FILE_BASIC_INFO,
}

struct EnumerationSession {
    relative: PathBuf,
    entries: Vec<DirectoryEntry>,
    index: usize,
    search_expression: Option<OsString>,
    page: usize,
}

struct MutableState {
    tombstones: BTreeMap<String, Tombstone>,
    pending_new: Vec<PathBuf>,
}

struct Provider {
    resolver: Resolver,
    view: PathBuf,
    state_path: PathBuf,
    state: Mutex<MutableState>,
    mutation_gate: Mutex<()>,
    enumerations: Mutex<HashMap<GUID, EnumerationSession>>,
}

fn wide(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(Some(0)).collect()
}

fn hr_from_io(error: &std::io::Error) -> HRESULT {
    match error.raw_os_error() {
        Some(code) => HRESULT::from_win32(code as u32),
        None => E_FAIL,
    }
}

fn callback_log(arguments: std::fmt::Arguments<'_>) {
    let _ = writeln!(std::io::stderr().lock(), "{arguments}");
}

fn callback_result(name: &str, action: impl FnOnce() -> HrResult<()>) -> HRESULT {
    match catch_unwind(AssertUnwindSafe(action)) {
        Ok(Ok(())) => S_OK,
        Ok(Err(error)) => {
            callback_log(format_args!("callback error name={name} hresult={error:?}"));
            error
        }
        Err(_) => {
            callback_log(format_args!("callback panic name={name}"));
            E_FAIL
        }
    }
}

unsafe fn callback_provider<'a>(data: *const PRJ_CALLBACK_DATA) -> HrResult<&'a Provider> {
    if data.is_null() {
        return Err(E_INVALIDARG);
    }
    let data = unsafe { &*data };
    if data.Size < size_of::<PRJ_CALLBACK_DATA>() as u32 {
        return Err(E_INVALIDARG);
    }
    if data.InstanceContext.is_null() {
        return Err(E_FAIL);
    }
    Ok(unsafe { &*(data.InstanceContext.cast::<Provider>()) })
}

unsafe fn callback_relative(data: *const PRJ_CALLBACK_DATA) -> HrResult<PathBuf> {
    if data.is_null() {
        return Err(E_INVALIDARG);
    }
    let value = unsafe { (*data).FilePathName.to_string() }.map_err(|_| E_INVALIDARG)?;
    normalize_relative(Path::new(&value)).map_err(|_| E_INVALIDARG)
}

fn metadata_info(path: &Path) -> HrResult<PRJ_FILE_BASIC_INFO> {
    let metadata = fs::metadata(path).map_err(|error| hr_from_io(&error))?;
    Ok(PRJ_FILE_BASIC_INFO {
        IsDirectory: metadata.is_dir(),
        FileSize: if metadata.is_file() {
            metadata.file_size() as i64
        } else {
            0
        },
        CreationTime: metadata.creation_time() as i64,
        LastAccessTime: metadata.last_access_time() as i64,
        LastWriteTime: metadata.last_write_time() as i64,
        ChangeTime: 0,
        FileAttributes: metadata.file_attributes(),
    })
}

fn synthetic_data_info() -> PRJ_FILE_BASIC_INFO {
    PRJ_FILE_BASIC_INFO {
        IsDirectory: true,
        FileAttributes: FILE_ATTRIBUTE_DIRECTORY.0,
        ..Default::default()
    }
}

fn resolved_info(resolved: &Resolved) -> HrResult<PRJ_FILE_BASIC_INFO> {
    match resolved {
        Resolved::Physical(path) => metadata_info(path),
        Resolved::SyntheticData => Ok(synthetic_data_info()),
    }
}

fn load_tombstones(path: &Path) -> Result<BTreeMap<String, Tombstone>, String> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(error) => return Err(format!("cannot read {}: {error}", path.display())),
    };
    let mut result = BTreeMap::new();
    for (index, line) in text.lines().enumerate() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (kind, raw_path) = line
            .split_once('\t')
            .ok_or_else(|| format!("{}:{}: expected KIND<TAB>PATH", path.display(), index + 1))?;
        let relative = normalize_relative(Path::new(raw_path))?;
        if !is_beneath_data(&relative) {
            return Err(format!(
                "{}:{}: tombstone is not beneath Data: {}",
                path.display(),
                index + 1,
                relative.display()
            ));
        }
        let directory = match kind {
            "D" => true,
            "F" => false,
            _ => {
                return Err(format!(
                    "{}:{}: kind must be D or F",
                    path.display(),
                    index + 1
                ));
            }
        };
        result.insert(
            fold_path(&relative),
            Tombstone {
                path: relative,
                directory,
            },
        );
    }
    Ok(result)
}

impl Provider {
    fn persist_locked(&self, state: &MutableState) -> HrResult<()> {
        if let Some(parent) = self
            .state_path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent).map_err(|error| hr_from_io(&error))?;
        }
        let mut text = String::from(
            "# projfs-game-view tombstones v1\n# D=directory, F=file; paths are relative to the view root\n",
        );
        for tombstone in state.tombstones.values() {
            let kind = if tombstone.directory { 'D' } else { 'F' };
            text.push(kind);
            text.push('\t');
            text.push_str(&tombstone.path.to_string_lossy().replace('/', "\\"));
            text.push('\n');
        }
        let temp = self.state_path.with_extension("tmp");
        fs::write(&temp, text).map_err(|error| hr_from_io(&error))?;
        if self.state_path.exists() {
            fs::remove_file(&self.state_path).map_err(|error| hr_from_io(&error))?;
        }
        fs::rename(temp, &self.state_path).map_err(|error| hr_from_io(&error))
    }

    fn tombstone_snapshot(&self) -> HrResult<BTreeMap<String, Tombstone>> {
        self.state
            .lock()
            .map(|state| state.tombstones.clone())
            .map_err(|_| E_FAIL)
    }

    fn validate_data_mutation(&self, relative: &Path) -> HrResult<PathBuf> {
        let relative = normalize_relative(relative).map_err(|_| E_INVALIDARG)?;
        if !is_beneath_data(&relative) {
            return Err(E_INVALIDARG);
        }
        Ok(relative)
    }

    fn overwrite_relative(relative: &Path) -> PathBuf {
        relative.components().skip(1).collect()
    }

    fn remove_overwrite_item(&self, relative: &Path) -> HrResult<()> {
        let subpath = Self::overwrite_relative(relative);
        let Some(actual) = find_case_insensitive(&self.resolver.overwrite, &subpath) else {
            return Ok(());
        };
        let metadata = fs::symlink_metadata(&actual).map_err(|error| hr_from_io(&error))?;
        if metadata.is_dir() {
            fs::remove_dir_all(actual).map_err(|error| hr_from_io(&error))
        } else {
            fs::remove_file(actual).map_err(|error| hr_from_io(&error))
        }
    }

    fn copy_view_item_to_overwrite_locked(&self, relative: &Path) -> HrResult<()> {
        let relative = self.validate_data_mutation(relative)?;
        let source = self.view.join(&relative);
        let metadata = fs::metadata(&source).map_err(|error| hr_from_io(&error))?;
        let subpath = Self::overwrite_relative(&relative);
        let requested_destination = self.resolver.overwrite.join(&subpath);
        let existing = find_case_insensitive(&self.resolver.overwrite, &subpath);

        if metadata.is_dir() {
            if let Some(existing) = existing {
                if !existing.is_dir() {
                    fs::remove_file(existing).map_err(|error| hr_from_io(&error))?;
                }
            }
            fs::create_dir_all(&requested_destination).map_err(|error| hr_from_io(&error))?;
        } else {
            if let Some(parent) = requested_destination.parent() {
                fs::create_dir_all(parent).map_err(|error| hr_from_io(&error))?;
            }
            let destination = if let Some(existing) = existing {
                if existing.is_dir() {
                    fs::remove_dir_all(&existing).map_err(|error| hr_from_io(&error))?;
                    requested_destination
                } else {
                    existing
                }
            } else {
                requested_destination
            };
            fs::copy(source, destination).map_err(|error| hr_from_io(&error))?;
        }

        let mut state = self.state.lock().map_err(|_| E_FAIL)?;
        if let Some(key) = state.tombstones.iter().find_map(|(key, tombstone)| {
            paths_equal(&tombstone.path, &relative).then(|| key.clone())
        }) {
            state.tombstones.remove(&key);
        }
        state
            .pending_new
            .retain(|path| !paths_equal(path, &relative));
        self.persist_locked(&state)
    }

    fn copy_view_item_to_overwrite(&self, relative: &Path) -> HrResult<()> {
        let _mutation = self.mutation_gate.lock().map_err(|_| E_FAIL)?;
        self.copy_view_item_to_overwrite_locked(relative)
    }

    fn mark_pending_or_clear_tombstone(&self, relative: &Path) -> HrResult<()> {
        let _mutation = self.mutation_gate.lock().map_err(|_| E_FAIL)?;
        let relative = self.validate_data_mutation(relative)?;
        let mut state = self.state.lock().map_err(|_| E_FAIL)?;
        if !state
            .pending_new
            .iter()
            .any(|path| paths_equal(path, &relative))
        {
            state.pending_new.push(relative.clone());
        }
        let tombstone_key = state.tombstones.iter().find_map(|(key, tombstone)| {
            paths_equal(&tombstone.path, &relative).then(|| key.clone())
        });
        if let Some(key) = tombstone_key {
            state.tombstones.remove(&key);
            self.persist_locked(&state)?;
        }
        Ok(())
    }

    fn finish_pending_if_needed(&self, relative: &Path) -> HrResult<()> {
        let relative = self.validate_data_mutation(relative)?;
        let _mutation = self.mutation_gate.lock().map_err(|_| E_FAIL)?;
        let pending = self
            .state
            .lock()
            .map_err(|_| E_FAIL)?
            .pending_new
            .iter()
            .any(|path| paths_equal(path, &relative));
        if pending {
            self.copy_view_item_to_overwrite_locked(&relative)?;
        }
        Ok(())
    }

    fn record_deletion(&self, relative: &Path, directory: bool) -> HrResult<()> {
        let relative = self.validate_data_mutation(relative)?;
        let _mutation = self.mutation_gate.lock().map_err(|_| E_FAIL)?;
        let key = fold_path(&relative);
        {
            let mut state = self.state.lock().map_err(|_| E_FAIL)?;
            state
                .pending_new
                .retain(|path| !paths_equal(path, &relative));
            if let Some(old_key) = state.tombstones.iter().find_map(|(key, tombstone)| {
                paths_equal(&tombstone.path, &relative).then(|| key.clone())
            }) {
                state.tombstones.remove(&old_key);
            }
            state.tombstones.insert(
                key,
                Tombstone {
                    path: relative.clone(),
                    directory,
                },
            );
            self.persist_locked(&state)?;
        }
        self.remove_overwrite_item(&relative)
    }

    fn make_enumeration(&self, relative: &Path) -> HrResult<EnumerationSession> {
        let tombstones = self.tombstone_snapshot()?;
        let mut entries: Vec<_> = self
            .resolver
            .enumerate(relative, &tombstones)
            .map_err(|_| HRESULT::from_win32(ERROR_FILE_NOT_FOUND.0))?
            .into_iter()
            .map(|(name, resolved)| {
                Ok(DirectoryEntry {
                    name,
                    info: resolved_info(&resolved)?,
                })
            })
            .collect::<HrResult<_>>()?;
        entries.sort_by(|left, right| {
            let left_wide = wide(&left.name);
            let right_wide = wide(&right.name);
            match unsafe {
                PrjFileNameCompare(PCWSTR(left_wide.as_ptr()), PCWSTR(right_wide.as_ptr()))
            } {
                value if value < 0 => Ordering::Less,
                0 => Ordering::Equal,
                _ => Ordering::Greater,
            }
        });
        Ok(EnumerationSession {
            relative: relative.to_path_buf(),
            entries,
            index: 0,
            search_expression: None,
            page: 0,
        })
    }
}

unsafe extern "system" fn start_directory_enumeration(
    callback_data: *const PRJ_CALLBACK_DATA,
    enumeration_id: *const GUID,
) -> HRESULT {
    callback_result("start-directory-enumeration", || {
        if enumeration_id.is_null() {
            return Err(E_INVALIDARG);
        }
        let provider = unsafe { callback_provider(callback_data)? };
        let relative = unsafe { callback_relative(callback_data)? };
        let session = provider.make_enumeration(&relative)?;
        provider
            .enumerations
            .lock()
            .map_err(|_| E_FAIL)?
            .insert(unsafe { *enumeration_id }, session);
        Ok(())
    })
}

unsafe extern "system" fn end_directory_enumeration(
    callback_data: *const PRJ_CALLBACK_DATA,
    enumeration_id: *const GUID,
) -> HRESULT {
    callback_result("end-directory-enumeration", || {
        if enumeration_id.is_null() {
            return Err(E_INVALIDARG);
        }
        let provider = unsafe { callback_provider(callback_data)? };
        provider
            .enumerations
            .lock()
            .map_err(|_| E_FAIL)?
            .remove(unsafe { &*enumeration_id });
        Ok(())
    })
}

unsafe extern "system" fn get_directory_enumeration(
    callback_data: *const PRJ_CALLBACK_DATA,
    enumeration_id: *const GUID,
    search_expression: PCWSTR,
    buffer: PRJ_DIR_ENTRY_BUFFER_HANDLE,
) -> HRESULT {
    callback_result("get-directory-enumeration", || {
        if enumeration_id.is_null() {
            return Err(E_INVALIDARG);
        }
        let provider = unsafe { callback_provider(callback_data)? };
        let callback_data_ref = unsafe { &*callback_data };
        let mut sessions = provider.enumerations.lock().map_err(|_| E_FAIL)?;
        let session = sessions
            .get_mut(unsafe { &*enumeration_id })
            .ok_or(E_INVALIDARG)?;
        let restart = (callback_data_ref.Flags.0 & PRJ_CB_DATA_FLAG_ENUM_RESTART_SCAN.0) != 0;
        if restart {
            session.index = 0;
            session.search_expression = None;
            session.page = 0;
        }
        session.page += 1;
        if session.search_expression.is_none() {
            let expression = if search_expression.is_null() {
                OsString::from("*")
            } else {
                OsString::from(unsafe { search_expression.to_string() }.map_err(|_| E_INVALIDARG)?)
            };
            session.search_expression = Some(expression);
        }
        let pattern_storage = wide(
            session
                .search_expression
                .as_deref()
                .unwrap_or_else(|| OsStr::new("*")),
        );
        let pattern = PCWSTR(pattern_storage.as_ptr());
        let mut entries_written = 0usize;
        let mut buffer_full = false;
        while session.index < session.entries.len() {
            let entry = &session.entries[session.index];
            let name = wide(&entry.name);
            if !unsafe { PrjFileNameMatch(PCWSTR(name.as_ptr()), pattern) } {
                session.index += 1;
                continue;
            }
            match unsafe { PrjFillDirEntryBuffer(PCWSTR(name.as_ptr()), Some(&entry.info), buffer) }
            {
                Ok(()) => {
                    session.index += 1;
                    entries_written += 1;
                }
                Err(error)
                    if error.code()
                        == HRESULT::from_win32(
                            windows::Win32::Foundation::ERROR_INSUFFICIENT_BUFFER.0,
                        ) =>
                {
                    buffer_full = true;
                    if entries_written == 0 {
                        callback_log(format_args!(
                            "enum page id={:?} path={} page={} returned=0 next={} total={} buffer_full=true",
                            unsafe { &*enumeration_id },
                            session.relative.display(),
                            session.page,
                            session.index,
                            session.entries.len()
                        ));
                        return Err(error.code());
                    }
                    break;
                }
                Err(error) => return Err(error.code()),
            }
            if (callback_data_ref.Flags.0 & PRJ_CB_DATA_FLAG_ENUM_RETURN_SINGLE_ENTRY.0) != 0 {
                break;
            }
        }
        callback_log(format_args!(
            "enum page id={:?} path={} page={} returned={} next={} total={} buffer_full={buffer_full}",
            unsafe { &*enumeration_id },
            session.relative.display(),
            session.page,
            entries_written,
            session.index,
            session.entries.len()
        ));
        Ok(())
    })
}

unsafe extern "system" fn get_placeholder_info(callback_data: *const PRJ_CALLBACK_DATA) -> HRESULT {
    callback_result("get-placeholder-info", || {
        let provider = unsafe { callback_provider(callback_data)? };
        let relative = unsafe { callback_relative(callback_data)? };
        let tombstones = provider.tombstone_snapshot()?;
        let resolved = provider
            .resolver
            .resolve(&relative, &tombstones)
            .ok_or_else(|| HRESULT::from_win32(ERROR_FILE_NOT_FOUND.0))?;
        let destination = provider
            .resolver
            .view_relative(&relative, &tombstones)
            .ok_or_else(|| HRESULT::from_win32(ERROR_FILE_NOT_FOUND.0))?;
        let destination_wide = wide(destination.as_os_str());
        let mut placeholder = PRJ_PLACEHOLDER_INFO {
            FileBasicInfo: resolved_info(&resolved)?,
            ..Default::default()
        };
        let callback_data_ref = unsafe { &*callback_data };
        unsafe {
            PrjWritePlaceholderInfo(
                callback_data_ref.NamespaceVirtualizationContext,
                PCWSTR(destination_wide.as_ptr()),
                &mut placeholder,
                size_of::<PRJ_PLACEHOLDER_INFO>() as u32,
            )
        }
        .map_err(|error| error.code())
    })
}

struct AlignedBuffer(*mut c_void);
impl Drop for AlignedBuffer {
    fn drop(&mut self) {
        unsafe { PrjFreeAlignedBuffer(self.0) };
    }
}

unsafe extern "system" fn get_file_data(
    callback_data: *const PRJ_CALLBACK_DATA,
    byte_offset: u64,
    length: u32,
) -> HRESULT {
    callback_result("get-file-data", || {
        let provider = unsafe { callback_provider(callback_data)? };
        let relative = unsafe { callback_relative(callback_data)? };
        let tombstones = provider.tombstone_snapshot()?;
        let Resolved::Physical(source) = provider
            .resolver
            .resolve(&relative, &tombstones)
            .ok_or_else(|| HRESULT::from_win32(ERROR_FILE_NOT_FOUND.0))?
        else {
            return Err(E_INVALIDARG);
        };
        let mut file = File::open(source).map_err(|error| hr_from_io(&error))?;
        file.seek(SeekFrom::Start(byte_offset))
            .map_err(|error| hr_from_io(&error))?;
        let callback_data_ref = unsafe { &*callback_data };
        let allocation = unsafe {
            PrjAllocateAlignedBuffer(
                callback_data_ref.NamespaceVirtualizationContext,
                length as usize,
            )
        };
        if allocation.is_null() {
            return Err(E_FAIL);
        }
        let allocation = AlignedBuffer(allocation);
        let bytes =
            unsafe { std::slice::from_raw_parts_mut(allocation.0.cast::<u8>(), length as usize) };
        file.read_exact(bytes).map_err(|error| hr_from_io(&error))?;
        unsafe {
            PrjWriteFileData(
                callback_data_ref.NamespaceVirtualizationContext,
                &callback_data_ref.DataStreamId,
                allocation.0,
                byte_offset,
                length,
            )
        }
        .map_err(|error| error.code())
    })
}

unsafe extern "system" fn notification(
    callback_data: *const PRJ_CALLBACK_DATA,
    is_directory: bool,
    notification: PRJ_NOTIFICATION,
    _destination_file_name: PCWSTR,
    operation_parameters: *mut PRJ_NOTIFICATION_PARAMETERS,
) -> HRESULT {
    callback_result("notification", || {
        let provider = unsafe { callback_provider(callback_data)? };
        let callback_data_ref = unsafe { &*callback_data };
        if callback_data_ref.TriggeringProcessId == std::process::id() {
            return Ok(());
        }
        let relative = unsafe { callback_relative(callback_data)? };
        if !is_beneath_data(&relative) {
            return Ok(());
        }
        let (action, result) = if notification == PRJ_NOTIFICATION_NEW_FILE_CREATED
            || notification == PRJ_NOTIFICATION_FILE_OVERWRITTEN
        {
            if !operation_parameters.is_null() {
                unsafe {
                    (*operation_parameters).PostCreate.NotificationMask =
                        PRJ_NOTIFY_USE_EXISTING_MASK;
                }
            }
            (
                "created-or-overwritten",
                provider.mark_pending_or_clear_tombstone(&relative),
            )
        } else if notification == PRJ_NOTIFICATION_FILE_HANDLE_CLOSED_FILE_MODIFIED {
            (
                "closed-modified",
                provider.copy_view_item_to_overwrite(&relative),
            )
        } else if notification == PRJ_NOTIFICATION_FILE_HANDLE_CLOSED_NO_MODIFICATION {
            (
                "closed-unmodified",
                provider.finish_pending_if_needed(&relative),
            )
        } else if notification == PRJ_NOTIFICATION_FILE_HANDLE_CLOSED_FILE_DELETED {
            (
                "closed-deleted",
                provider.record_deletion(&relative, is_directory),
            )
        } else {
            return Ok(());
        };
        if result.is_ok() {
            callback_log(format_args!(
                "notification action={action} path={} directory={is_directory} process={}",
                relative.display(),
                callback_data_ref.TriggeringProcessId
            ));
        }
        result
    })
}

fn canonical_file_path(path: &Path) -> Result<PathBuf, String> {
    if path.exists() {
        return fs::canonicalize(path).map_err(|error| error.to_string());
    }
    let file_name = path
        .file_name()
        .ok_or_else(|| format!("path must name a file: {}", path.display()))?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    Ok(fs::canonicalize(parent)
        .map_err(|error| error.to_string())?
        .join(file_name))
}

fn prepare_args(mut args: ServeArgs) -> Result<ServeArgs, String> {
    if !args.base.is_dir() {
        return Err(format!("base is not a directory: {}", args.base.display()));
    }
    for path in &args.mods {
        if !path.is_dir() {
            return Err(format!("mod is not a directory: {}", path.display()));
        }
    }
    args.view_was_reparse_point = args.view.exists()
        && fs::symlink_metadata(&args.view)
            .map_err(|error| format!("cannot inspect view {}: {error}", args.view.display()))?
            .file_attributes()
            & FILE_ATTRIBUTE_REPARSE_POINT.0
            != 0;
    fs::create_dir_all(&args.view)
        .map_err(|error| format!("cannot create view {}: {error}", args.view.display()))?;
    fs::create_dir_all(&args.overwrite).map_err(|error| {
        format!(
            "cannot create overwrite {}: {error}",
            args.overwrite.display()
        )
    })?;
    if args.state.is_dir() {
        return Err("--state must name a file".into());
    }

    args.base = fs::canonicalize(&args.base).map_err(|error| error.to_string())?;
    args.view = fs::canonicalize(&args.view).map_err(|error| error.to_string())?;
    args.overwrite = fs::canonicalize(&args.overwrite).map_err(|error| error.to_string())?;
    args.mods = args
        .mods
        .iter()
        .map(fs::canonicalize)
        .collect::<Result<_, _>>()
        .map_err(|error| error.to_string())?;
    args.state = canonical_file_path(&args.state)?;
    args.ready_file = canonical_file_path(&args.ready_file)?;
    args.stop_file = canonical_file_path(&args.stop_file)?;

    let overlaps = |left: &Path, right: &Path| left.starts_with(right) || right.starts_with(left);
    if overlaps(&args.view, &args.base)
        || overlaps(&args.view, &args.overwrite)
        || args.mods.iter().any(|path| overlaps(&args.view, path))
    {
        return Err("--view must not contain, or be contained by, base, mods, or overwrite".into());
    }
    if overlaps(&args.overwrite, &args.base)
        || args.mods.iter().any(|path| overlaps(&args.overwrite, path))
    {
        return Err("--overwrite must be independent from base and mods".into());
    }
    if args.state.starts_with(&args.overwrite) {
        return Err("--state must be outside --overwrite".into());
    }
    Ok(args)
}

struct VirtualizationGuard(Option<PRJ_NAMESPACE_VIRTUALIZATION_CONTEXT>);

impl VirtualizationGuard {
    fn new(context: PRJ_NAMESPACE_VIRTUALIZATION_CONTEXT) -> Self {
        Self(Some(context))
    }

    fn stop(mut self) {
        if let Some(context) = self.0.take() {
            unsafe { PrjStopVirtualizing(context) };
        }
    }
}

impl Drop for VirtualizationGuard {
    fn drop(&mut self) {
        if let Some(context) = self.0.take() {
            unsafe { PrjStopVirtualizing(context) };
        }
    }
}

pub fn serve(args: ServeArgs) -> Result<(), String> {
    let args = prepare_args(args)?;
    if args.stop_file.exists() {
        fs::remove_file(&args.stop_file)
            .map_err(|error| format!("cannot remove stale stop file: {error}"))?;
    }
    if args.ready_file.exists() {
        fs::remove_file(&args.ready_file)
            .map_err(|error| format!("cannot remove stale ready file: {error}"))?;
    }
    let tombstones = load_tombstones(&args.state)?;
    let provider = Box::new(Provider {
        resolver: Resolver {
            base: args.base.clone(),
            mods: args.mods.clone(),
            overwrite: args.overwrite.clone(),
        },
        view: args.view.clone(),
        state_path: args.state.clone(),
        state: Mutex::new(MutableState {
            tombstones,
            pending_new: Vec::new(),
        }),
        mutation_gate: Mutex::new(()),
        enumerations: Mutex::new(HashMap::new()),
    });

    let view_wide = wide(args.view.as_os_str());
    if !args.view_was_reparse_point {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|error| error.to_string())?
            .as_nanos();
        let address = (&args as *const ServeArgs as usize) as u128;
        let instance_id =
            GUID::from_u128(timestamp ^ address ^ ((std::process::id() as u128) << 96));
        unsafe {
            PrjMarkDirectoryAsPlaceholder(
                PCWSTR(view_wide.as_ptr()),
                PCWSTR::null(),
                None,
                &instance_id,
            )
        }
        .map_err(|error| format!("PrjMarkDirectoryAsPlaceholder failed: {error}"))?;
    }

    let callbacks = PRJ_CALLBACKS {
        StartDirectoryEnumerationCallback: Some(start_directory_enumeration),
        EndDirectoryEnumerationCallback: Some(end_directory_enumeration),
        GetDirectoryEnumerationCallback: Some(get_directory_enumeration),
        GetPlaceholderInfoCallback: Some(get_placeholder_info),
        GetFileDataCallback: Some(get_file_data),
        NotificationCallback: Some(notification),
        ..Default::default()
    };
    let notification_root = [0u16];
    let mask = PRJ_NOTIFY_NEW_FILE_CREATED
        | PRJ_NOTIFY_FILE_OVERWRITTEN
        | PRJ_NOTIFY_FILE_HANDLE_CLOSED_NO_MODIFICATION
        | PRJ_NOTIFY_FILE_HANDLE_CLOSED_FILE_MODIFIED
        | PRJ_NOTIFY_FILE_HANDLE_CLOSED_FILE_DELETED;
    let mut mapping = PRJ_NOTIFICATION_MAPPING {
        NotificationBitMask: mask,
        NotificationRoot: PCWSTR(notification_root.as_ptr()),
    };
    let options = PRJ_STARTVIRTUALIZING_OPTIONS {
        NotificationMappings: &mut mapping,
        NotificationMappingsCount: 1,
        ..Default::default()
    };
    let context = unsafe {
        PrjStartVirtualizing(
            PCWSTR(view_wide.as_ptr()),
            &callbacks,
            Some((&*provider as *const Provider).cast::<c_void>()),
            Some(&options),
        )
    }
    .map_err(|error| {
        if args.view_was_reparse_point {
            format!("existing reparse --view is not a usable ProjFS virtualization root: {error}")
        } else {
            format!("PrjStartVirtualizing failed: {error}")
        }
    })?;
    let virtualization = VirtualizationGuard::new(context);

    let ready_result = (|| -> Result<(), String> {
        if let Some(parent) = args
            .ready_file
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent)
                .map_err(|error| format!("cannot create ready-file parent: {error}"))?;
        }
        fs::write(&args.ready_file, format!("pid={}\n", std::process::id()))
            .map_err(|error| format!("cannot write ready file: {error}"))?;
        println!(
            "provider config base={} view={} overwrite={} state={} mods={:?}",
            args.base.display(),
            args.view.display(),
            args.overwrite.display(),
            args.state.display(),
            args.mods
        );
        println!("provider ready: {}", args.view.display());
        while !args.stop_file.exists() {
            thread::sleep(Duration::from_millis(100));
        }
        Ok(())
    })();

    virtualization.stop();
    let _ = fs::remove_file(&args.ready_file);
    drop(provider);
    ready_result?;
    println!("provider stopped");
    Ok(())
}
