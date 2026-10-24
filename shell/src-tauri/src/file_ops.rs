//! Moving, copying and deleting the files a search shows: Move To…, Copy To… and Delete on the results the person
//! selected in the window. The file system work is here; the index follows through the search helper's `file_op`
//! (`SearchService`), so a moved or deleted file leaves the results right away instead of after a rescan of its folders.
//!
//! On Windows the wide APIs do the work, as Explorer does: `MoveFileExW` (never over a file), `CopyFileExW` (create-new,
//! with progress and cancel) and `SHFileOperationW` (the Recycle Bin, never a quiet permanent delete). Other systems are
//! out of the first step: the operation says so, and the window doesn't offer it there.
#![cfg_attr(not(windows), allow(dead_code))]

use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter};

#[cfg(windows)]
use std::os::windows::fs::MetadataExt;
#[cfg(windows)]
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
#[cfg(windows)]
use windows_sys::Win32::Storage::FileSystem::{
    CopyFileExW, CreateFileW, DeleteFileW, GetFileInformationByHandle, MoveFileExW, BY_HANDLE_FILE_INFORMATION,
    COPY_FILE_FAIL_IF_EXISTS, FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_OFFLINE, FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS,
    FILE_ATTRIBUTE_RECALL_ON_OPEN, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    MOVEFILE_COPY_ALLOWED, OPEN_EXISTING, PROGRESS_CANCEL, PROGRESS_CONTINUE,
};
#[cfg(windows)]
use windows_sys::Win32::UI::Shell::{
    SHFileOperationW, SHFILEOPSTRUCTW, FO_DELETE, FOF_ALLOWUNDO, FOF_NOERRORUI, FOF_SILENT, FOF_WANTNUKEWARNING,
};

/// What the window asks for: the operation, the results it had selected, and (for a move or a copy) the folder they go
/// to. `size` and `mtime` are what the search knew of each file: a file that changed since then isn't touched.
#[derive(Deserialize)]
pub struct Request {
    pub kind: String,
    #[serde(default)]
    pub id: u64,
    #[serde(default)]
    pub destination: Option<String>,
    #[serde(default)]
    pub items: Vec<Item>,
}

#[derive(Deserialize)]
pub struct Item {
    pub path: String,
    #[serde(default)]
    pub size: Option<i64>,
    #[serde(default)]
    pub mtime: Option<f64>,
}

/// What the window gets back: what happened to each file, and the counts for its report line.
#[derive(Default, serde::Serialize)]
struct Report {
    kind: String,
    id: u64,
    total: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    destination: Option<String>,
    done: usize,
    renamed: usize,
    failed: usize,
    skipped: usize,
    cancelled: bool,
    /// False when the files moved but the index couldn't follow: the shell then asks for a rescan.
    index: bool,
    items: Vec<Value>,
}

/// The operations under way, by the window's number for them: cancel sets a flag that stops one between files, and
/// stops a copy where it is.
static RUNNING: OnceLock<Mutex<HashMap<u64, Arc<AtomicBool>>>> = OnceLock::new();

fn running() -> &'static Mutex<HashMap<u64, Arc<AtomicBool>>> {
    RUNNING.get_or_init(Mutex::default)
}

/// Stops the operation the window numbers `id` (`cancel_file_operation`).
pub fn cancel(id: u64) {
    if let Some(flag) = running().lock().unwrap().get(&id) {
        flag.store(true, Ordering::SeqCst);
    }
}

/// Runs one operation: the files first, then the index for each file that changed. Blocking: the command runs it on a
/// blocking thread, since a copy can take a while.
pub fn run(app: &AppHandle, shell: &crate::Shell, request: Value) -> Result<Value, String> {
    let request: Request =
        serde_json::from_value(request).map_err(|error| format!("not a file operation: {error}"))?;
    if !["move", "copy", "delete", "delete_permanent"].contains(&request.kind.as_str()) {
        return Err(format!("not something DigUp does with files: {}", request.kind));
    }
    let destination = match request.kind.as_str() {
        "move" | "copy" => {
            let folder = PathBuf::from(request.destination.as_deref().ok_or("no folder to put them in")?);
            if !folder.is_dir() {
                return Err(format!("{} isn't a folder", folder.display()));
            }
            Some(folder)
        }
        _ => None,
    };
    #[cfg(not(windows))]
    {
        let _ = (app, shell, destination);
        return Err("moving, copying and deleting files is Windows only for now".into());
    }
    #[cfg(windows)]
    {
        let flag = Arc::new(AtomicBool::new(false));
        running().lock().unwrap().insert(request.id, flag.clone());
        let report = operate(app, shell, &request, destination.as_deref(), &flag);
        running().lock().unwrap().remove(&request.id);
        Ok(report)
    }
}

fn operate(app: &AppHandle, shell: &crate::Shell, request: &Request, destination: Option<&Path>,
           cancel: &AtomicBool) -> Value {
    let total = request.items.len();
    let mut report = Report {
        kind: request.kind.clone(),
        id: request.id,
        total,
        destination: destination.map(path_text),
        index: true,
        ..Report::default()
    };
    // The search service's vectors are reloaded once, after the files are done: reloading them for every one of
    // thousands would cost more than the moves themselves. Row paths come from the database anyway.
    let mut index_changed = false;
    emit(app, json!({"phase": "start", "id": report.id, "kind": report.kind, "total": total,
                     "destination": report.destination}));
    'files: for (position, item) in request.items.iter().enumerate() {
        if cancel.load(Ordering::SeqCst) {
            report.cancelled = true;
            break;
        }
        let from = Path::new(&item.path);
        'one: {
            if let Err(reason) = check_source(from, item) {
                report.skipped += 1;
                report.items.push(json!({"from": item.path, "ok": false, "skipped": true, "error": reason,
                                         "size": item.size, "mtime": item.mtime}));
                break 'one;
            }
            match request.kind.as_str() {
                "delete" | "delete_permanent" => {
                    let permanent = request.kind == "delete_permanent";
                    // The command is reachable from the webview: a permanent delete for a normal path would bypass the
                    // Recycle Bin if the UI were buggy. The Rust side enforces the same rule as the report button.
                    if permanent && !recycle_too_long(from) {
                        report.failed += 1;
                        report.items.push(json!({"from": item.path, "ok": false,
                                                 "error": "permanent deletion is only for paths the Recycle Bin can't take",
                                                 "size": item.size, "mtime": item.mtime}));
                        break 'one;
                    }
                    // The window offers permanent deletion only for this exact refusal. Keep the reason machine-readable:
                    // the UI must not match on a translated message.
                    if !permanent && recycle_too_long(from) {
                        report.failed += 1;
                        report.items.push(json!({"from": item.path, "ok": false, "reason": "path_too_long",
                                                 "error": "its path is too long for the Recycle Bin",
                                                 "size": item.size, "mtime": item.mtime}));
                        break 'one;
                    }
                    let result = if permanent { delete_permanently(from) } else { delete_item(from) };
                    match result {
                        Ok(()) => {
                            report.done += 1;
                            // Note it before the index follows: a batch of thousands takes longer than the watcher's
                            // quiet period, and its burst must not race the index updates. If the index fails, the
                            // command's repair sync (and the next start) settles it; the note is only for the echo.
                            crate::echo::note_file(from);
                            let mut entry = json!({"from": item.path, "ok": true});
                            if permanent {
                                entry["permanent"] = json!(true);
                            }
                            if let Err(error) = index_delete(shell, &item.path) {
                                report.index = false;
                                entry["index"] = json!(false);
                                entry["index_error"] = json!(error);
                            } else {
                                index_changed = true;
                            }
                            report.items.push(entry);
                        }
                        Err(error) => {
                            report.failed += 1;
                            report.items.push(json!({"from": item.path, "ok": false, "error": error,
                                                     "size": item.size, "mtime": item.mtime}));
                        }
                    }
                }
                "move" | "copy" => {
                    let destination = destination.expect("a destination was checked");
                    if request.kind == "move" && same_folder(from, destination) {
                        report.skipped += 1;
                        report.items.push(json!({"from": item.path, "ok": false, "skipped": true,
                                                 "error": "it's already in that folder",
                                                 "size": item.size, "mtime": item.mtime}));
                        break 'one;
                    }
                    let result = if request.kind == "move" {
                        move_to_free(from, destination)
                    } else {
                        copy_to_free(app, report.id, position, total, from, destination, cancel)
                    };
                    match result {
                        Ok((to, renamed)) => {
                            report.done += 1;
                            if renamed {
                                report.renamed += 1;
                            }
                            let mut entry = json!({"from": item.path, "to": path_text(&to), "ok": true,
                                                   "renamed": renamed});
                            if request.kind == "move" {
                                // Before the index follows: see the delete above. Both ends are noted, so a watcher
                                // burst of any size that is only this move's files is passed over.
                                crate::echo::note_move(from, &to);
                                if let Err(error) = index_move(shell, &item.path, &to) {
                                    report.index = false;
                                    entry["index"] = json!(false);
                                    entry["index_error"] = json!(error);
                                } else {
                                    index_changed = true;
                                }
                            }
                            report.items.push(entry);
                        }
                        Err(error) => {
                            // Cancelling a copy is what the person asked for, not a failure.
                            if cancel.load(Ordering::SeqCst) {
                                report.cancelled = true;
                                break 'files;
                            }
                            report.failed += 1;
                            report.items.push(json!({"from": item.path, "ok": false, "error": error,
                                                     "size": item.size, "mtime": item.mtime}));
                        }
                    }
                }
                _ => unreachable!("the kind was checked"),
            }
        }
        emit(app, json!({"phase": "progress", "id": report.id, "kind": report.kind, "done": position + 1,
                         "total": total, "path": item.path}));
    }
    if index_changed {
        if let Err(error) = reload_index(shell) {
            report.index = false;
            eprintln!("digup-shell: the search service didn't reload the index: {error}");
        }
    }
    emit(app, json!({"phase": "done", "id": report.id, "kind": report.kind, "done": report.done,
                     "renamed": report.renamed, "failed": report.failed, "skipped": report.skipped,
                     "cancelled": report.cancelled, "index": report.index}));
    serde_json::to_value(report).unwrap_or_else(|_| json!({}))
}

fn emit(app: &AppHandle, payload: Value) {
    let _ = app.emit_to(crate::WINDOW, "file-op", payload);
}

// MARK: Each file

/// The file as the search knew it, or why it isn't touched: a link (its target is the file), a folder, or a file that
/// changed since the search showed it.
fn check_source(from: &Path, item: &Item) -> Result<(), String> {
    let metadata = std::fs::symlink_metadata(from).map_err(|error| match error.kind() {
        ErrorKind::NotFound => "it isn't there any more".to_string(),
        _ => format!("it can't be looked at: {error}"),
    })?;
    if metadata.file_type().is_symlink() {
        return Err("it's a link, not a file".into());
    }
    if !metadata.is_file() {
        return Err("it isn't a file".into());
    }
    if dataless(&metadata) {
        return Err("it's only in the cloud, not downloaded".into());
    }
    if let Some(size) = item.size {
        if metadata.len() as i64 != size {
            return Err("it changed since the search (its size)".into());
        }
    }
    if let Some(mtime) = item.mtime {
        if let Some(now) = modified(&metadata) {
            if (now - mtime).abs() > 1.0 {
                return Err("it changed since the search (its date)".into());
            }
        }
    }
    Ok(())
}

/// A move or rename, never over a file: a name that's taken gets a number (`photo (2).jpg`).
fn move_to_free(from: &Path, destination: &Path) -> Result<(PathBuf, bool), String> {
    for _ in 0..20 {
        let target = free_path(destination, from);
        match move_file(from, &target) {
            Ok(()) => {
                let renamed = target.file_name() != from.file_name();
                return Ok((target, renamed));
            }
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,   // something appeared meanwhile
            Err(error) => return Err(why(&error)),
        }
    }
    Err("no free name could be found next to it".into())
}

/// A copy, never over a file, with progress and cancel. A copy that stops partway leaves a file that isn't the
/// original: it goes.
fn copy_to_free(app: &AppHandle, id: u64, position: usize, total: usize, from: &Path, destination: &Path,
                cancel: &AtomicBool) -> Result<(PathBuf, bool), String> {
    for _ in 0..20 {
        let target = free_path(destination, from);
        let progress = Progress { app: Some(app), id, position, total, from, to: &target, cancel,
                                  last: AtomicU64::new(0) };
        match progress.copy() {
            Ok(()) => {
                let renamed = target.file_name() != from.file_name();
                return Ok((target, renamed));
            }
            Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(match std::fs::remove_file(&target) {
                    Ok(()) => why(&error),
                    Err(cleanup) => format!("{}; the partial copy couldn't be removed: {cleanup}", why(&error)),
                });
            }
        }
    }
    Err("no free name could be found next to it".into())
}

fn delete_item(from: &Path) -> Result<(), String> {
    trash(from)
}

/// The one reason a delete gets a second, explicit action in the window: the Recycle Bin's API can't take the path.
#[cfg(windows)]
fn recycle_too_long(path: &Path) -> bool {
    path.to_string_lossy().encode_utf16().count() >= 260
}

#[cfg(not(windows))]
fn recycle_too_long(_: &Path) -> bool {
    false
}

/// The last resort the window offers after the Recycle Bin refused a path: delete the file itself, through the
/// extended-length form, and report it as permanent. Never called unless the person confirmed that second action.
#[cfg(windows)]
fn delete_permanently(path: &Path) -> Result<(), String> {
    let path = wide(path);
    if unsafe { DeleteFileW(path.as_ptr()) } == 0 {
        return Err(why(&std::io::Error::last_os_error()));
    }
    Ok(())
}

#[cfg(not(windows))]
fn delete_permanently(_: &Path) -> Result<(), String> {
    Err("permanent deletion is Windows only for now".into())
}

/// Whether the file is already in the folder asked for (either spelling, either case).
fn same_folder(from: &Path, destination: &Path) -> bool {
    let (Some(source), Ok(target)) = (from.parent().and_then(|path| std::fs::canonicalize(path).ok()),
                                      std::fs::canonicalize(destination)) else {
        return false;
    };
    source == target
}

/// The next name in `destination` that's free: `photo.jpg`, else `photo (2).jpg`, `photo (3).jpg`… Names compare
/// without case, as Windows does, so a name that differs only in case isn't taken for a free one.
fn free_path(destination: &Path, from: &Path) -> PathBuf {
    let name = from.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    let first = destination.join(&name);
    if !taken(&first) {
        return first;
    }
    let (stem, extension) = split_name(&name);
    for number in 2..=9999 {
        let candidate = destination.join(format!("{stem} ({number}){extension}"));
        if !taken(&candidate) {
            return candidate;
        }
    }
    destination.join(format!("{stem} (copied){extension}"))
}

/// `photo.jpg` → (`photo`, `.jpg`); a name without an extension (or a dotfile) keeps it that way.
fn split_name(name: &str) -> (String, String) {
    match name.rfind('.') {
        Some(dot) if dot > 0 => (name[..dot].to_string(), name[dot..].to_string()),
        _ => (name.to_string(), String::new()),
    }
}

fn taken(path: &Path) -> bool {
    if path.exists() {
        return true;
    }
    let (Some(folder), Some(name)) = (path.parent(), path.file_name()) else {
        return false;
    };
    let wanted = name.to_string_lossy().to_lowercase();
    std::fs::read_dir(folder)
        .map(|entries| entries.flatten().any(|entry| entry.file_name().to_string_lossy().to_lowercase() == wanted))
        .unwrap_or(false)
}

/// How many of `paths` have a name already in `destination` (they get a number), and the name the first one would
/// take (`photo (2).jpg`). The window's Move/Copy sheet says so only when there is one.
pub fn conflicts(destination: &Path, paths: &[String]) -> (usize, Option<String>) {
    // One listing for the whole batch: `taken` would read the folder again for each file.
    let existing: std::collections::HashSet<String> = std::fs::read_dir(destination)
        .map(|entries| entries.flatten().map(|entry| entry.file_name().to_string_lossy().to_lowercase()).collect())
        .unwrap_or_default();
    let mut count = 0;
    let mut example = None;
    for path in paths {
        let from = Path::new(path);
        let Some(name) = from.file_name() else { continue };
        if existing.contains(&name.to_string_lossy().to_lowercase()) {
            count += 1;
            if example.is_none() {
                example = free_path(destination, from).file_name().map(|name| name.to_string_lossy().into_owned());
            }
        }
    }
    (count, example)
}

// MARK: The index

/// The path as the index knows it: `/`, whatever Windows uses.
fn path_text(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn modified(metadata: &std::fs::Metadata) -> Option<f64> {
    metadata.modified().ok()?.duration_since(UNIX_EPOCH).ok().map(|since| since.as_secs_f64())
}

/// A cloud placeholder (OneDrive's Files On-Demand, or a File Provider): reading it would download the file, and a
/// move or copy would too. The crawl leaves these out, so one that shows up changed after the search.
#[cfg(windows)]
fn dataless(metadata: &std::fs::Metadata) -> bool {
    metadata.file_attributes()
        & (FILE_ATTRIBUTE_OFFLINE | FILE_ATTRIBUTE_RECALL_ON_OPEN | FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS)
        != 0
}

#[cfg(not(windows))]
fn dataless(_: &std::fs::Metadata) -> bool {
    false
}

/// Tells the search helper that the file is at its new path now (its vectors stay), with the identity of the new place:
/// a move to another volume gives it another device and inode. See `SearchService`'s `file_op`.
fn index_move(shell: &crate::Shell, from: &str, to: &Path) -> Result<(), String> {
    let metadata = std::fs::metadata(to).map_err(|error| format!("the new file can't be read: {error}"))?;
    let mut item = json!({"from": from, "to": path_text(to), "size": metadata.len()});
    if let Some(mtime) = modified(&metadata) {
        item["mtime"] = json!(mtime);
    }
    // Without the identity (a file held open without sharing), the row keeps the one it has: right for a move within
    // the same volume, and the next sync settles anything else.
    if let Some((inode, device)) = identity(to) {
        item["inode"] = json!(inode.to_string());
        item["device"] = json!(device.to_string());
    }
    ask(shell, json!({"op": "file_op", "kind": "move", "items": [item], "reload": false}))
}

fn index_delete(shell: &crate::Shell, from: &str) -> Result<(), String> {
    ask(shell, json!({"op": "file_op", "kind": "delete", "items": [{"from": from}], "reload": false}))
}

/// The one reload after a batch of index changes (see `index_changed`): the search service's vectors and file kinds
/// catch up, so a search right after a move sees the new paths.
fn reload_index(shell: &crate::Shell) -> Result<(), String> {
    ask(shell, json!({"op": "reload"}))
}

fn ask(shell: &crate::Shell, request: Value) -> Result<(), String> {
    let reply = tauri::async_runtime::block_on(shell.search.ask(request))
        .map_err(|error| format!("the index didn't follow: {error}"))?;
    if reply["ok"].as_bool() == Some(false) {
        return Err(reply["error"].as_str().unwrap_or("the index didn't follow").to_string());
    }
    if let Some(result) = reply["results"].as_array().and_then(|results| results.first()) {
        if result["ok"].as_bool() != Some(true) {
            return Err(result["error"].as_str().unwrap_or("the index didn't follow").to_string());
        }
    }
    Ok(())
}

/// Why a file operation failed, in words a person can act on.
fn why(error: &std::io::Error) -> String {
    match error.kind() {
        ErrorKind::NotFound => "it isn't there any more".into(),
        ErrorKind::PermissionDenied => "Windows says it's in use, or not allowed".into(),
        ErrorKind::AlreadyExists => "there's already a file with that name".into(),
        _ => error.to_string(),
    }
}

// MARK: Windows

/// A path as the wide calls take it: backslashes, the extended-length prefix (the Win32 calls here don't have the
/// long-path opt-in), and a NUL at the end. `SHFileOperationW` gets its own list and stays unprefixed: the shell doesn't
/// accept `\\?\` there.
#[cfg(windows)]
fn wide(path: &Path) -> Vec<u16> {
    let text = path.to_string_lossy().replace('/', "\\");
    let text = if text.starts_with(r"\\?\") || text.starts_with(r"\\.\") {
        text
    } else if let Some(rest) = text.strip_prefix(r"\\") {
        format!(r"\\?\UNC\{rest}")   // a share: \\server\share → \\?\UNC\server\share
    } else if text.len() >= 3 && text.as_bytes()[0].is_ascii_alphabetic() && text.as_bytes()[1] == b':'
        && text.as_bytes()[2] == b'\\' {
        format!(r"\\?\{text}")
    } else {
        text   // relative or drive-relative: no prefix means anything, and search paths are absolute
    };
    text.encode_utf16().chain([0]).collect()
}

#[cfg(windows)]
fn move_file(from: &Path, to: &Path) -> std::io::Result<()> {
    let (from, to) = (wide(from), wide(to));
    // No MOVEFILE_REPLACE_EXISTING: a file that's there already stays. Across volumes Windows copies it.
    if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), MOVEFILE_COPY_ALLOWED) } == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

/// The file's id on its volume, as the crawler knows it (`GetFileInformationByHandle`'s file index and volume serial):
/// a moved file keeps its vectors by it.
#[cfg(windows)]
fn identity(path: &Path) -> Option<(u64, u64)> {
    let path = wide(path);
    let handle = unsafe {
        CreateFileW(path.as_ptr(), FILE_READ_ATTRIBUTES, FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                    std::ptr::null(), OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, std::ptr::null_mut())
    };
    if handle == INVALID_HANDLE_VALUE {
        return None;
    }
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    let ok = unsafe { GetFileInformationByHandle(handle, &mut info) };
    unsafe { CloseHandle(handle) };
    (ok != 0).then_some((((info.nFileIndexHigh as u64) << 32) | info.nFileIndexLow as u64,
                         info.dwVolumeSerialNumber as u64))
}

#[cfg(not(windows))]
fn move_file(from: &Path, to: &Path) -> std::io::Result<()> {
    let _ = (from, to);
    Err(std::io::Error::new(ErrorKind::Unsupported, "moving files is Windows only for now"))
}

#[cfg(not(windows))]
fn identity(_: &Path) -> Option<(u64, u64)> {
    None
}

/// One copy of one file, and what the window hears of it: where it got to, and whether it should stop. It's also the
/// `lpData` of `CopyFileExW`'s progress callback, which asks it each chunk.
struct Progress<'a> {
    app: Option<&'a AppHandle>,
    id: u64,
    position: usize,
    total: usize,
    from: &'a Path,
    to: &'a Path,
    cancel: &'a AtomicBool,
    /// When the window last heard of it (milliseconds), so a copy doesn't flood it.
    last: AtomicU64,
}

impl Progress<'_> {
    #[cfg(windows)]
    fn copy(&self) -> std::io::Result<()> {
        let (from, to) = (wide(self.from), wide(self.to));
        let mut cancelled: i32 = 0;
        let ok = unsafe {
            CopyFileExW(from.as_ptr(), to.as_ptr(), Some(copy_progress),
                        self as *const Progress as *const std::ffi::c_void, &mut cancelled, COPY_FILE_FAIL_IF_EXISTS)
        };
        if ok == 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }

    #[cfg(not(windows))]
    fn copy(&self) -> std::io::Result<()> {
        Err(std::io::Error::new(ErrorKind::Unsupported, "moving and copying files is Windows only for now"))
    }

    fn say(&self, bytes: i64, size: i64) {
        if let Some(app) = self.app {
            emit(app, json!({"phase": "progress", "id": self.id, "done": self.position, "total": self.total,
                             "path": self.from.to_string_lossy(), "to": self.to.to_string_lossy(),
                             "bytes": bytes, "size": size}));
        }
    }
}

#[cfg(windows)]
unsafe extern "system" fn copy_progress(total: i64, transferred: i64, _stream: i64, _stream_transferred: i64,
                                        _number: u32, _reason: u32, _source: HANDLE, _target: HANDLE,
                                        data: *const std::ffi::c_void) -> u32 {
    let progress = unsafe { &*(data as *const Progress) };
    if progress.cancel.load(Ordering::Relaxed) {
        return PROGRESS_CANCEL;
    }
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|since| since.as_millis() as u64).unwrap_or(0);
    let last = progress.last.load(Ordering::Relaxed);
    if transferred >= total || now.saturating_sub(last) >= 100 {
        progress.last.store(now, Ordering::Relaxed);
        progress.say(transferred, total);
    }
    PROGRESS_CONTINUE
}

/// To the Recycle Bin, as Explorer's Delete does, through the shell (and its warning when a file is too big for it).
#[cfg(windows)]
fn trash(path: &Path) -> Result<(), String> {
    // SHFileOperationW is the shell's old path, and the shell refuses a path at MAX_PATH. Say so before spending a
    // shell call on it; moving and copying have their extended-length form, deleting doesn't.
    if recycle_too_long(path) {
        return Err("its path is too long for the Recycle Bin".into());
    }
    let mut from: Vec<u16> = path.to_string_lossy().replace('/', "\\").encode_utf16().collect();
    from.extend([0, 0]);   // the shell wants the list ended by two NULs
    let mut operation = SHFILEOPSTRUCTW {
        hwnd: std::ptr::null_mut(),
        wFunc: FO_DELETE,
        pFrom: from.as_ptr(),
        pTo: std::ptr::null(),
        fFlags: (FOF_ALLOWUNDO | FOF_WANTNUKEWARNING | FOF_SILENT | FOF_NOERRORUI) as u16,
        fAnyOperationsAborted: 0,
        hNameMappings: std::ptr::null_mut(),
        lpszProgressTitle: std::ptr::null(),
    };
    let result = unsafe { SHFileOperationW(&mut operation) };
    if result != 0 {
        return Err(format!("the Recycle Bin refused it (code {result:#x})"));
    }
    if operation.fAnyOperationsAborted != 0 {
        return Err("the move to the Recycle Bin was stopped".into());
    }
    Ok(())
}

#[cfg(not(windows))]
fn trash(_: &Path) -> Result<(), String> {
    Err("moving files to the Recycle Bin is Windows only for now".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(windows)]
    use windows_sys::Win32::Storage::FileSystem::{SetFileAttributesW, FILE_ATTRIBUTE_READONLY};

    fn folder(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("DigUp-file-ops-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        folder
    }

    fn put(folder: &Path, name: &str) -> PathBuf {
        let path = folder.join(name);
        std::fs::write(&path, b"picture").unwrap();
        path
    }

    #[test]
    fn a_free_name_is_left_alone() {
        let destination = folder("free");
        let source = folder("free-source");
        let from = put(&source, "photo.jpg");
        assert_eq!(free_path(&destination, &from), destination.join("photo.jpg"));
        let _ = std::fs::remove_dir_all(&destination);
        let _ = std::fs::remove_dir_all(&source);
    }

    #[test]
    fn a_taken_name_gets_a_number() {
        let destination = folder("taken");
        let source = folder("taken-source");
        put(&destination, "photo.jpg");
        put(&destination, "photo (2).jpg");
        let from = put(&source, "photo.jpg");
        assert_eq!(free_path(&destination, &from).file_name().unwrap(), "photo (3).jpg");
        let _ = std::fs::remove_dir_all(&destination);
        let _ = std::fs::remove_dir_all(&source);
    }

    #[test]
    fn names_compare_without_case() {
        let destination = folder("case");
        let source = folder("case-source");
        put(&destination, "Photo.JPG");
        let from = put(&source, "photo.jpg");
        assert_eq!(free_path(&destination, &from).file_name().unwrap(), "photo (2).jpg");
        let _ = std::fs::remove_dir_all(&destination);
        let _ = std::fs::remove_dir_all(&source);
    }

    #[test]
    fn a_name_without_an_extension_keeps_it_that_way() {
        assert_eq!(split_name("notes"), ("notes".to_string(), String::new()));
        assert_eq!(split_name(".hidden"), (".hidden".to_string(), String::new()));
        assert_eq!(split_name("archive.tar.gz"), ("archive.tar".to_string(), ".gz".to_string()));
    }

    #[test]
    fn a_file_that_changed_since_the_search_is_left_alone() {
        let source = folder("changed");
        let path = put(&source, "photo.jpg");
        let stale = Item { path: path.display().to_string(), size: Some(1), mtime: None };
        assert!(check_source(&path, &stale).is_err());
        let same = Item { path: path.display().to_string(), size: Some(7), mtime: None };
        assert!(check_source(&path, &same).is_ok());
        let _ = std::fs::remove_dir_all(&source);
    }

    #[test]
    fn a_folder_is_not_a_file() {
        let source = folder("folder-source");
        let item = Item { path: source.display().to_string(), size: None, mtime: None };
        assert!(check_source(&source, &item).is_err());
        let _ = std::fs::remove_dir_all(&source);
    }

    #[test]
    fn conflicts_are_counted_and_named() {
        let destination = folder("conflicts");
        let source = folder("conflicts-source");
        put(&destination, "photo.jpg");
        let free = put(&source, "notes.txt");
        let taken = put(&source, "photo.jpg");
        assert_eq!(conflicts(&destination, &[free.display().to_string()]), (0, None));
        let (count, example) = conflicts(&destination, &[free.display().to_string(), taken.display().to_string()]);
        assert_eq!(count, 1);
        assert_eq!(example.as_deref(), Some("photo (2).jpg"));
        for path in [destination, source] {
            let _ = std::fs::remove_dir_all(path);
        }
    }

    #[cfg(windows)]
    #[test]
    fn two_files_with_the_same_name_both_land() {
        let destination = folder("landing");
        let (one, two) = (folder("landing-one"), folder("landing-two"));
        let (first, second) = (put(&one, "photo.jpg"), put(&two, "photo.jpg"));
        let (to_first, renamed_first) = move_to_free(&first, &destination).unwrap();
        let (to_second, renamed_second) = move_to_free(&second, &destination).unwrap();
        assert!(!renamed_first);
        assert!(renamed_second);
        assert_eq!(to_first.file_name().unwrap(), "photo.jpg");
        assert_eq!(to_second.file_name().unwrap(), "photo (2).jpg");
        assert!(to_first.is_file() && to_second.is_file());
        for path in [destination, one, two] {
            let _ = std::fs::remove_dir_all(path);
        }
    }

    #[cfg(windows)]
    #[test]
    fn a_move_never_overwrites() {
        let destination = folder("move-dest");
        let source = folder("move-source");
        let existing = put(&destination, "photo.jpg");
        let from = put(&source, "photo.jpg");
        let (to, renamed) = move_to_free(&from, &destination).unwrap();
        assert!(renamed);
        assert_eq!(std::fs::read(existing).unwrap(), b"picture");
        assert!(to.is_file() && !from.exists());
        for path in [destination, source] {
            let _ = std::fs::remove_dir_all(path);
        }
    }

    #[cfg(windows)]
    #[test]
    fn a_copy_never_overwrites() {
        let destination = folder("copy-dest");
        let source = folder("copy-source");
        let existing = put(&destination, "photo.jpg");
        let from = put(&source, "photo.jpg");
        let cancel = AtomicBool::new(false);
        let target = free_path(&destination, &from);
        let progress = Progress { app: None, id: 1, position: 0, total: 1, from: &from, to: &target, cancel: &cancel,
                                  last: AtomicU64::new(0) };
        progress.copy().unwrap();
        assert_eq!(std::fs::read(existing).unwrap(), b"picture");
        assert_eq!(std::fs::read(&target).unwrap(), b"picture");
        assert_eq!(target.file_name().unwrap(), "photo (2).jpg");
        for path in [destination, source] {
            let _ = std::fs::remove_dir_all(path);
        }
    }

    #[test]
    fn the_index_can_be_waited_for_from_a_blocking_thread() {
        // `run` asks the search helper from the blocking thread the command put it on: blocking there on the async
        // runtime (which the helper's reply wakes) must work, not panic.
        let answer = tauri::async_runtime::block_on(tauri::async_runtime::spawn_blocking(|| {
            tauri::async_runtime::block_on(async { 42 })
        }))
        .unwrap();
        assert_eq!(answer, 42);
    }

    #[cfg(windows)]
    #[test]
    fn a_long_path_moves_and_copies() {
        let destination = folder("long-dest");
        let source = folder("long-source");
        let deep = std::path::PathBuf::from("a".repeat(90)).join("b".repeat(90)).join("c".repeat(60));
        let (target, origin) = (destination.join(&deep), source.join(&deep));
        std::fs::create_dir_all(&target).unwrap();
        std::fs::create_dir_all(&origin).unwrap();
        let from = put(&origin, "photo.jpg");
        assert!(from.to_string_lossy().len() > 260);
        assert!(identity(&from).is_some());
        let (moved, renamed) = move_to_free(&from, &target).unwrap();
        assert!(!renamed && moved.is_file() && !from.exists());
        let copy = free_path(&destination, &moved);
        let cancel = AtomicBool::new(false);
        let progress = Progress { app: None, id: 1, position: 0, total: 1, from: &moved, to: &copy, cancel: &cancel,
                                  last: AtomicU64::new(0) };
        progress.copy().unwrap();
        assert!(copy.is_file());
        let _ = std::fs::remove_dir_all(&destination);
        let _ = std::fs::remove_dir_all(&source);
    }

    #[cfg(windows)]
    fn clear_read_only(path: &Path) {
        unsafe { SetFileAttributesW(wide(path).as_ptr(), FILE_ATTRIBUTE_NORMAL) };
    }

    #[cfg(windows)]
    #[test]
    fn read_only_files_still_move_and_copy() {
        let destination = folder("readonly-dest");
        let source = folder("readonly-source");
        let from = put(&source, "photo.jpg");
        assert_ne!(unsafe { SetFileAttributesW(wide(&from).as_ptr(), FILE_ATTRIBUTE_READONLY) }, 0);
        let (moved, _) = move_to_free(&from, &destination).unwrap();
        assert!(moved.is_file());
        let copy = free_path(&source, &moved);
        let cancel = AtomicBool::new(false);
        let progress = Progress { app: None, id: 1, position: 0, total: 1, from: &moved, to: &copy, cancel: &cancel,
                                  last: AtomicU64::new(0) };
        progress.copy().unwrap();
        assert!(copy.is_file());
        clear_read_only(&moved);
        clear_read_only(&copy);
        for path in [destination, source] {
            let _ = std::fs::remove_dir_all(path);
        }
    }

    #[cfg(windows)]
    #[test]
    fn unicode_names_move_and_copy() {
        let destination = folder("unicode-dest");
        let source = folder("unicode-source");
        let name = "é-日-ı-ö.jpg";
        let from = put(&source, name);
        let (moved, renamed) = move_to_free(&from, &destination).unwrap();
        assert!(!renamed);
        assert_eq!(moved.file_name().unwrap(), name);
        let copy = free_path(&source, &moved);
        let cancel = AtomicBool::new(false);
        let progress = Progress { app: None, id: 1, position: 0, total: 1, from: &moved, to: &copy, cancel: &cancel,
                                  last: AtomicU64::new(0) };
        progress.copy().unwrap();
        assert!(copy.is_file());
        assert_eq!(copy.file_name().unwrap(), name);
        for path in [destination, source] {
            let _ = std::fs::remove_dir_all(path);
        }
    }

    #[cfg(windows)]
    #[test]
    fn a_busy_file_is_reported_and_stays() {
        use std::os::windows::fs::OpenOptionsExt;
        let destination = folder("busy-dest");
        let source = folder("busy-source");
        let from = put(&source, "photo.jpg");
        // No sharing: a move, a copy and a delete all meet Windows' sharing violation.
        let held = std::fs::OpenOptions::new().read(true).share_mode(0).open(&from).unwrap();
        assert!(move_to_free(&from, &destination).is_err());
        assert!(from.is_file() && !destination.join("photo.jpg").exists());
        drop(held);
        let (moved, _) = move_to_free(&from, &destination).unwrap();
        assert!(moved.is_file());
        for path in [destination, source] {
            let _ = std::fs::remove_dir_all(path);
        }
    }

    #[cfg(windows)]
    #[test]
    fn a_cloud_placeholder_is_left_alone() {
        let source = folder("cloud");
        let path = put(&source, "photo.jpg");
        assert_ne!(unsafe { SetFileAttributesW(wide(&path).as_ptr(), FILE_ATTRIBUTE_OFFLINE) }, 0);
        let metadata = std::fs::symlink_metadata(&path).unwrap();
        assert!(dataless(&metadata));
        let item = Item { path: path.display().to_string(), size: Some(7), mtime: None };
        assert_eq!(check_source(&path, &item).unwrap_err(), "it's only in the cloud, not downloaded");
        let _ = std::fs::remove_dir_all(&source);
    }

    #[cfg(windows)]
    #[test]
    fn a_long_path_can_be_deleted_permanently() {
        let source = folder("permanent-long");
        let deep = std::path::PathBuf::from("a".repeat(90)).join("b".repeat(90)).join("c".repeat(60));
        let origin = source.join(&deep);
        std::fs::create_dir_all(&origin).unwrap();
        let path = put(&origin, "photo.jpg");
        assert!(path.to_string_lossy().len() > 260);
        delete_permanently(&path).unwrap();
        assert!(!path.exists());
        let _ = std::fs::remove_dir_all(&source);
    }

    #[cfg(windows)]
    #[test]
    fn a_path_too_long_for_the_recycle_bin_is_refused() {
        let long = PathBuf::from("C:/").join("a".repeat(300)).join("photo.jpg");
        assert_eq!(trash(&long).unwrap_err(), "its path is too long for the Recycle Bin");
    }

    #[cfg(windows)]
    #[test]
    fn the_recycle_bin_leaves_a_file_its_not_given() {
        let source = folder("trash");
        let path = source.join("gone.jpg");
        assert!(trash(&path).is_err());   // nothing there: nothing to recycle, and no crash
        let _ = std::fs::remove_dir_all(&source);
    }
}
