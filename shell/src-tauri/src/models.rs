//! The model's folder: where it is, whether it's there, and the few file operations around it. The names and sizes are
//! the helper's contract (`ModelFiles` in the engine); the hashes and download addresses come from `model-manifest`, so
//! they aren't written down twice here.
//!
//! One folder holds both files. `--models` wins in dev runs, then the person's choice, then the folder an older DigUp
//! used (when at least one file is there), then a folder of its own under the app's data folder.

use crate::helper::Launch;
use crate::options::Options;
use crate::settings::Settings;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::Child;
use std::sync::Mutex;
use std::time::Duration;

pub const MODEL_FILES: [&str; 2] = ["embeddinggemma-2-Q8_0.gguf", "mmproj-embeddinggemma-2-Q8_0.gguf"];
pub const MODEL_BYTES: [u64; 2] = [309_855_456, 554_821_024];
pub const TOTAL_BYTES: u64 = MODEL_BYTES[0] + MODEL_BYTES[1];
pub const PROFILE_ID: &str = "embeddinggemma-2";
/// Room to download next to files that are still there, and for the drive not to fill to the last byte.
const SPACE_MARGIN: u64 = 200 << 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderState {
    Available,
    /// The parent is there; the folder isn't.
    Missing,
    /// A disk on the way doesn't exist (an unplugged drive, a letter that went away).
    Unavailable,
    /// The disk isn't the one the folder was chosen on.
    WrongVolume,
    NotFolder,
}

impl FolderState {
    pub fn as_str(self) -> &'static str {
        match self {
            FolderState::Available => "available",
            FolderState::Missing => "missing",
            FolderState::Unavailable => "unavailable",
            FolderState::WrongVolume => "wrong-volume",
            FolderState::NotFolder => "not-folder",
        }
    }
}

/// One model file as it is in the folder now (size only: the checksums are the helper's `model-verify`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileStatus {
    pub name: &'static str,
    pub bytes: u64,
    pub present: bool,
    pub size: u64,
}

impl FileStatus {
    pub fn ok(&self) -> bool {
        self.present && self.size == self.bytes
    }
}

/// The app's data folder, as the engine spells it on each platform (`AppPaths`).
pub fn data_folder() -> PathBuf {
    #[cfg(windows)]
    {
        if let Some(local) = std::env::var_os("LOCALAPPDATA").filter(|value| !value.is_empty()) {
            return PathBuf::from(local).join("DigUpPlus");
        }
        if let Some(profile) = std::env::var_os("USERPROFILE").filter(|value| !value.is_empty()) {
            return PathBuf::from(profile).join("AppData").join("Local").join("DigUpPlus");
        }
        PathBuf::from("DigUpPlus")
    }
    #[cfg(target_os = "macos")]
    {
        std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default()
            .join("Library").join("Application Support").join("DigUp")
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::env::var_os("XDG_DATA_HOME").filter(|value| !value.is_empty()).map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
            .unwrap_or_default().join("digup")
    }
}

/// Where DigUp kept the model before folders were a choice (and where the Mac app keeps it).
pub fn legacy_models_folder() -> PathBuf {
    data_folder().join("Models")
}

/// The folder DigUp uses when there is no choice and no older install.
pub fn default_models_folder() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        legacy_models_folder()   // the Mac shell is only tried out; the Mac app's own folder stays
    }
    #[cfg(not(target_os = "macos"))]
    {
        legacy_models_folder().join(PROFILE_ID)
    }
}

/// `--models` exactly; else the person's folder; else an older install's folder when either file is there (both must
/// stay together: the helper mustn't look for the second one somewhere else); else the default.
pub fn resolve_models_folder(options: &Options, settings: &Settings) -> PathBuf {
    resolve(options.models.as_deref(), settings.models_folder.as_deref(), &legacy_models_folder(),
            &default_models_folder())
}

pub fn resolve(models: Option<&Path>, chosen: Option<&str>, legacy: &Path, fresh: &Path) -> PathBuf {
    if let Some(models) = models {
        return models.to_path_buf();
    }
    if let Some(chosen) = chosen.map(str::trim).filter(|chosen| !chosen.is_empty()) {
        return PathBuf::from(chosen);
    }
    if MODEL_FILES.iter().any(|name| legacy.join(name).is_file()) {
        return legacy.to_path_buf();
    }
    fresh.to_path_buf()
}

/// The folder a person meant: one with a model file is used as it is (an older install, a folder copied by hand);
/// anything else is a container, and the model gets its own folder in it (`Models/embeddinggemma-2`).
pub fn chosen_folder(target: &Path) -> PathBuf {
    let has_model_file = MODEL_FILES.iter().any(|name| target.join(name).is_file());
    let is_profile = target.file_name().and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case(PROFILE_ID));
    if has_model_file || is_profile {
        target.to_path_buf()
    } else {
        target.join(PROFILE_ID)
    }
}

/// Where the model is, without checksums: present or not, and its size.
pub fn files(folder: &Path) -> Vec<FileStatus> {
    MODEL_FILES.iter().zip(MODEL_BYTES).map(|(name, bytes)| {
        let folder = folder.join(name);
        let present = folder.exists();
        let size = std::fs::metadata(&folder).map(|file| file.len()).unwrap_or(0);
        FileStatus { name, bytes, present, size }
    }).collect()
}

pub fn complete(folder: &Path) -> bool {
    files(folder).iter().all(FileStatus::ok)
}

pub fn missing_bytes(folder: &Path) -> u64 {
    files(folder).iter().filter(|file| !file.ok()).map(|file| file.bytes).sum()
}

/// What deleting the model files would free (the files themselves, and any partial download).
pub fn used_bytes(folder: &Path) -> u64 {
    let mut total = 0;
    for name in MODEL_FILES {
        for path in [folder.join(name), folder.join(format!("{name}.partial"))] {
            total += std::fs::metadata(path).map(|file| file.len()).unwrap_or(0);
        }
    }
    total
}

/// The nearest folder on the way that exists (a chosen folder may not be made yet).
fn nearest_existing(folder: &Path) -> Option<PathBuf> {
    let mut current = if folder.is_absolute() {
        folder.to_path_buf()
    } else {
        std::env::current_dir().ok()?.join(folder)
    };
    loop {
        if current.exists() {
            return Some(current);
        }
        match current.parent() {
            Some(parent) if parent != current => current = parent.to_path_buf(),
            _ => return None,
        }
    }
}

/// Whether the folder is there, and on the volume it was chosen on (`expected_volume`: what was written then).
pub fn folder_state(folder: &Path, expected_volume: Option<u64>) -> FolderState {
    let Some(existing) = nearest_existing(folder) else { return FolderState::Unavailable };
    if let Some(expected) = expected_volume {
        match volume_id(&existing) {
            Some(volume) if volume != expected => return FolderState::WrongVolume,
            None => return FolderState::Unavailable,
            _ => {}
        }
    }
    match std::fs::metadata(folder) {
        Ok(metadata) if metadata.is_dir() => FolderState::Available,
        Ok(_) => FolderState::NotFolder,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => FolderState::Missing,
        Err(_) => FolderState::Unavailable,
    }
}

/// The volume's serial number (Windows) or device id (Unix) for the nearest existing folder.
#[cfg(windows)]
pub fn volume_id(folder: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{GetVolumeInformationW, GetVolumePathNameW};
    let existing = nearest_existing(folder)?;
    let wide: Vec<u16> = existing.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut root = vec![0u16; 261];
    if unsafe { GetVolumePathNameW(wide.as_ptr(), root.as_mut_ptr(), root.len() as u32) } == 0 {
        return None;
    }
    let mut serial: u32 = 0;
    let ok = unsafe {
        GetVolumeInformationW(root.as_ptr(), std::ptr::null_mut(), 0, &mut serial, std::ptr::null_mut(),
                              std::ptr::null_mut(), std::ptr::null_mut(), 0)
    };
    (ok != 0).then_some(serial as u64)
}

#[cfg(unix)]
pub fn volume_id(folder: &Path) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    let existing = nearest_existing(folder)?;
    std::fs::metadata(existing).ok().map(|metadata| metadata.dev())
}

#[cfg(windows)]
pub fn free_space(folder: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let existing = nearest_existing(folder)?;
    let wide: Vec<u16> = existing.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut free: u64 = 0;
    let ok = unsafe {
        GetDiskFreeSpaceExW(wide.as_ptr(), &mut free, std::ptr::null_mut(), std::ptr::null_mut())
    };
    (ok != 0).then_some(free)
}

#[cfg(unix)]
pub fn free_space(folder: &Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    let existing = nearest_existing(folder)?;
    let path = std::ffi::CString::new(existing.as_os_str().as_bytes()).ok()?;
    let mut stats: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(path.as_ptr(), &mut stats) } != 0 {
        return None;
    }
    let block = if stats.f_frsize > 0 { stats.f_frsize } else { stats.f_bsize };
    Some(stats.f_bavail as u64 * block as u64)
}

/// A path spelled one way on Windows (case and separators) or Unix, for comparing a choice with the current folder.
fn path_key(path: &Path) -> String {
    let text = path.to_string_lossy();
    let separator = if cfg!(windows) { '\\' } else { '/' };
    let text = if cfg!(windows) { text.replace('/', "\\") } else { text.to_string() };
    let trimmed = text.trim_end_matches(separator);
    let text = if trimmed.is_empty() || (cfg!(windows) && trimmed.len() == 2 && trimmed.ends_with(':')) {
        text
    } else {
        trimmed.to_string()
    };
    if cfg!(windows) { text.to_lowercase() } else { text }
}

pub fn same_path(one: &Path, other: &Path) -> bool {
    path_key(one) == path_key(other)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Use,
    Download,
    Move,
    Invalid,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Use => "use",
            Kind::Download => "download",
            Kind::Move => "move",
            Kind::Invalid => "invalid",
        }
    }
}

/// What choosing `target` would mean. `current_complete`: the model files in the active folder are all there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub kind: Kind,
    pub missing: u64,
    pub free: Option<u64>,
    pub problem: Option<String>,
    /// A `move` can also be done as a download, leaving the old copy in place.
    pub can_download: bool,
}

pub fn plan(target: &Path, current: &Path, current_complete: bool, expected_volume: Option<u64>,
            free: Option<u64>) -> Plan {
    let invalid = |problem: String| Plan {
        kind: Kind::Invalid, missing: 0, free, problem: Some(problem), can_download: false,
    };
    if !target.is_absolute() {
        return invalid("Choose a full path.".into());
    }
    let expected = if same_path(target, current) { expected_volume } else { None };
    match folder_state(target, expected) {
        FolderState::Unavailable => {
            return invalid("That disk isn't available.".into());
        }
        FolderState::WrongVolume => {
            return invalid("This folder is on a different disk than when it was chosen.".into());
        }
        FolderState::NotFolder => {
            return invalid(format!("{} isn't a folder.", target.display()));
        }
        FolderState::Available | FolderState::Missing => {}
    }
    if complete(target) {
        return Plan { kind: Kind::Use, missing: 0, free, problem: None, can_download: false };
    }
    let missing = missing_bytes(target);
    let status = files(target);
    // A move copies both files, even the one already in the target: the old file stays until the new one is checked
    // (both need room at once). A download only fetches what isn't usable, but a wrong file in the way is replaced
    // after its verified copy is ready, so its bytes count too.
    let existing: u64 = if current_complete {
        status.iter().filter(|file| file.present).map(|file| file.size).sum()
    } else {
        status.iter().filter(|file| file.present && !file.ok()).map(|file| file.size).sum()
    };
    let needed = (if current_complete { TOTAL_BYTES } else { missing }) + existing + SPACE_MARGIN;
    if free.is_some_and(|free| free < needed) {
        return invalid(format!(
            "Not enough space in {}: {} needed.",
            target.display(),
            human_bytes(needed)
        ));
    }
    if current_complete {
        Plan { kind: Kind::Move, missing, free, problem: None, can_download: true }
    } else {
        Plan { kind: Kind::Download, missing, free, problem: None, can_download: false }
    }
}

pub fn human_bytes(bytes: u64) -> String {
    let units = ["bytes", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit < units.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} bytes")
    } else if value >= 10.0 {
        format!("{value:.0} {}", units[unit])
    } else {
        format!("{value:.1} {}", units[unit])
    }
}

/// Deletes only the two model files and their partial downloads, retrying while Windows lets go of an mmap.
pub fn delete_files(folder: &Path) -> Result<(), String> {
    if !folder.is_absolute() {
        return Err("the model folder path isn't absolute".into());
    }
    // A folder that is gone is nothing to delete; one whose disk isn't there must keep its cleanup setting, so it is
    // tried again when the disk comes back.
    match folder_state(folder, None) {
        FolderState::Missing => return Ok(()),
        FolderState::Unavailable | FolderState::WrongVolume | FolderState::NotFolder => {
            return Err(format!("{} isn't available", folder.display()));
        }
        FolderState::Available => {}
    }
    for name in MODEL_FILES {
        for path in [folder.join(name), folder.join(format!("{name}.partial"))] {
            remove_with_retry(&path).map_err(|error| format!("{}: {}", path.display(), error))?;
        }
    }
    Ok(())
}

fn remove_with_retry(path: &Path) -> std::io::Result<()> {
    for attempt in 0..20 {
        match std::fs::remove_file(path) {
            Ok(()) => return Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) if attempt == 19 => return Err(error),
            Err(_) => std::thread::sleep(Duration::from_millis(250)),
        }
    }
    Ok(())
}

/// A helper process that is running and can be stopped (the model transfer: download, copy, verify).
pub struct Transfer {
    child: Mutex<Option<Child>>,
}

impl Default for Transfer {
    fn default() -> Self {
        Transfer { child: Mutex::default() }
    }
}

impl Transfer {
    /// Runs a helper to its end, handing each JSON line to `on_event`. Returns the last line, which is `done` or
    /// `error`; a helper that ends without either is an error.
    pub fn run(&self, launch: Launch, mut on_event: impl FnMut(Value)) -> Result<Value, String> {
        let (child, stdin, stdout) = launch.spawn()?;
        drop(stdin);
        *self.child.lock().unwrap() = Some(child);
        let mut last = Value::Null;
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            let Ok(event) = serde_json::from_str::<Value>(&line) else { continue };
            if event["event"] == "done" || event["event"] == "error" {
                last = event.clone();
            }
            on_event(event);
        }
        let mut child = self.child.lock().unwrap().take();
        let status = child.as_mut().and_then(|child| child.wait().ok());
        match last["event"].as_str() {
            Some("done") => Ok(last),
            Some("error") => Err(last["message"].as_str().unwrap_or("the model operation failed").to_string()),
            _ => Err(match status.and_then(|status| status.code()) {
                Some(code) => format!("the model helper stopped (exit {code})"),
                None => "the model helper stopped".to_string(),
            }),
        }
    }

    pub fn stop(&self) {
        if let Some(mut child) = self.child.lock().unwrap().take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// `model-manifest`: the helper's names, sizes, hashes and URLs (not duplicated in the shell).
pub fn manifest(launch: Launch) -> Result<Value, String> {
    let (mut child, stdin, stdout) = launch.spawn()?;
    drop(stdin);
    let mut text = String::new();
    BufReader::new(stdout).read_to_string(&mut text).map_err(|error| format!("can't read the manifest: {error}"))?;
    let _ = child.wait();
    text.lines().filter_map(|line| serde_json::from_str::<Value>(line).ok()).next_back()
        .ok_or_else(|| "the helper didn't say what the model files are".to_string())
}

/// The storage block Settings and setup show.
pub fn info(shell: &crate::Shell) -> Value {
    let folder = shell.models_folder();
    let settings = shell.settings.lock().unwrap();
    let state = folder_state(&folder, settings.models_volume);
    let cleanup = settings.model_cleanup.as_ref().map(PathBuf::from);
    json!({
        "models": folder.display().to_string(),
        "modelTotalBytes": TOTAL_BYTES,
        "modelsFolderState": state.as_str(),
        "modelFiles": files(&folder).iter().map(|file| json!({
            "name": file.name, "bytes": file.bytes, "present": file.present, "size": file.size, "ok": file.ok(),
        })).collect::<Vec<_>>(),
        "modelRemoved": settings.model_removed,
        "modelCleanup": cleanup.as_ref().map(|folder| folder.display().to_string()),
        "modelCleanupBytes": cleanup.as_ref().map(|folder| used_bytes(folder)).unwrap_or(0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ops::Deref;

    /// A test folder that goes away with the test, even when an assertion fails (the model-sized files inside it are
    /// real disk space on Windows).
    struct TempFolder(PathBuf);

    impl Deref for TempFolder {
        type Target = Path;
        fn deref(&self) -> &Path { &self.0 }
    }

    impl AsRef<Path> for TempFolder {
        fn as_ref(&self) -> &Path { &self.0 }
    }

    impl Drop for TempFolder {
        fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); }
    }

    impl TempFolder {
        fn path(&self) -> PathBuf { self.0.clone() }
    }

    fn folder(name: &str) -> TempFolder {
        let folder = std::env::temp_dir().join(format!("DigUp-models-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        TempFolder(folder)
    }

    fn sparse(folder: &Path, name: &str, bytes: u64) -> PathBuf {
        let path = folder.join(name);
        let file = std::fs::File::create(&path).unwrap();
        file.set_len(bytes).unwrap();
        path
    }

    #[test]
    fn a_choice_beats_the_old_folder_and_dev_beats_both() {
        let legacy = folder("legacy");
        let fresh = folder("fresh");
        let chosen = folder("chosen");
        assert_eq!(resolve(None, None, &legacy, &fresh), fresh.path());
        sparse(&legacy, MODEL_FILES[0], 10);
        assert_eq!(resolve(None, None, &legacy, &fresh), legacy.path(),
                   "one file keeps the old folder (both stay together)");
        assert_eq!(resolve(None, Some(&chosen.display().to_string()), &legacy, &fresh), chosen.path());
        assert_eq!(resolve(Some(&fresh), Some(&chosen.display().to_string()), &legacy, &fresh), fresh.path());
        assert_eq!(resolve(None, Some("  "), &legacy, &fresh), legacy.path());
    }

    #[test]
    fn a_path_with_spaces_and_cyrillic_is_kept_as_chosen() {
        let chosen = folder("Мои модели — 2026");
        assert_eq!(resolve(None, Some(&chosen.display().to_string()), Path::new("C:/old"), Path::new("C:/new")),
                   chosen.path());
    }

    #[test]
    fn the_new_default_is_a_profile_folder_beside_the_old_one() {
        if cfg!(target_os = "macos") {
            assert_eq!(default_models_folder(), legacy_models_folder());
        } else {
            assert_eq!(default_models_folder(), legacy_models_folder().join(PROFILE_ID));
        }
    }

    #[test]
    fn folder_states() {
        let base = folder("states");
        let inside = base.join("Models");
        assert_eq!(folder_state(&inside, None), FolderState::Missing);
        std::fs::create_dir_all(&inside).unwrap();
        assert_eq!(folder_state(&inside, None), FolderState::Available);
        let file = base.join("file");
        std::fs::write(&file, b"x").unwrap();
        assert_eq!(folder_state(&file, None), FolderState::NotFolder);
        let volume = volume_id(&base).expect("a volume id for an existing folder");
        assert_eq!(folder_state(&inside, Some(volume)), FolderState::Available);
        assert_eq!(folder_state(&inside, Some(volume.wrapping_add(1))), FolderState::WrongVolume);
        assert!(free_space(&base).unwrap() > 0);
    }

    #[test]
    fn deleting_touches_only_the_model_files_and_partials() {
        let base = folder("delete");
        let text = sparse(&base, MODEL_FILES[0], MODEL_BYTES[0]);
        let partial = base.join(format!("{}.partial", MODEL_FILES[1]));
        std::fs::write(&partial, b"half").unwrap();
        let stranger = base.join("notes.txt");
        std::fs::write(&stranger, b"keep me").unwrap();
        delete_files(&base).unwrap();
        assert!(!text.exists() && !partial.exists());
        assert!(stranger.exists());
        assert_eq!(used_bytes(&base), 0);
    }

    #[test]
    fn deleting_a_folder_that_is_gone_is_already_done() {
        let base = folder("delete-gone");
        assert!(delete_files(&base.join("not-made-yet")).is_ok());
    }

    #[test]
    fn a_chosen_container_gets_the_profile_folder() {
        let container = folder("chosen-container");
        assert_eq!(chosen_folder(&container), container.join(PROFILE_ID));
        let profile = container.join(PROFILE_ID);
        assert_eq!(chosen_folder(&profile), profile, "a folder already named after the model stays as it is");
        std::fs::create_dir_all(&container).unwrap();
        sparse(&container, MODEL_FILES[0], 1);
        assert_eq!(chosen_folder(&container), container.path(),
                   "a folder holding a model file is used in place");
    }

    #[test]
    fn a_full_target_is_used_in_place() {
        let current = folder("plan-current");
        let target = folder("plan-target");
        sparse(&target, MODEL_FILES[0], MODEL_BYTES[0]);
        sparse(&target, MODEL_FILES[1], MODEL_BYTES[1]);
        let full = plan(&target, &current, true, None, Some(u64::MAX));
        assert_eq!(full.kind, Kind::Use);
        assert_eq!(full.missing, 0);
    }

    #[test]
    fn a_partial_target_moves_or_downloads() {
        let current = folder("plan-partial-current");
        let target = folder("plan-partial-target");
        sparse(&target, MODEL_FILES[0], MODEL_BYTES[0]);
        let moving = plan(&target, &current, true, None, Some(u64::MAX));
        assert_eq!(moving.kind, Kind::Move);
        assert!(moving.can_download);
        assert_eq!(moving.missing, MODEL_BYTES[1]);
        let downloading = plan(&target, &current, false, None, Some(u64::MAX));
        assert_eq!(downloading.kind, Kind::Download);
        assert!(!downloading.can_download);
    }

    #[test]
    fn a_move_needs_room_for_both_files_even_when_one_is_already_there() {
        let current = folder("plan-move-space-current");
        let target = folder("plan-move-space-target");
        sparse(&target, MODEL_FILES[0], MODEL_BYTES[0]);
        // The target's copy stays while both are copied again: its bytes, both new files, and the margin.
        let needed = TOTAL_BYTES + MODEL_BYTES[0] + SPACE_MARGIN;
        let tight = plan(&target, &current, true, None, Some(needed - 1));
        assert_eq!(tight.kind, Kind::Invalid);
        assert!(tight.problem.unwrap().contains("Not enough space"));
        assert_eq!(plan(&target, &current, true, None, Some(needed)).kind, Kind::Move);
    }

    #[test]
    fn a_full_disk_or_a_file_is_invalid() {
        let current = folder("plan-invalid-current");
        let target = folder("plan-invalid-target");
        let full_disk = plan(&target, &current, true, None, Some(0));
        assert_eq!(full_disk.kind, Kind::Invalid);
        assert!(full_disk.problem.unwrap().contains("Not enough space"));
        let file = current.join("a-file");
        std::fs::write(&file, b"x").unwrap();
        assert_eq!(plan(&file, &current, true, None, Some(u64::MAX)).kind, Kind::Invalid);
        assert_eq!(plan(Path::new("relative"), &current, true, None, Some(u64::MAX)).kind, Kind::Invalid);
    }

    #[test]
    fn the_shell_gets_a_model_folder_in_its_environment() {
        let models = Path::new("G:/Ai/DigUp/Models/embeddinggemma-2");
        let mut settings = Settings::default();
        let env = crate::helper_env_for(&settings, models);
        assert!(env.iter().any(|(name, value)| name == "DIGUP_MODELS" && value == &models.display().to_string()));
        assert!(!env.iter().any(|(name, _)| name == "DIGUP_DEVICE"));
        settings.processor_only = true;
        let env = crate::helper_env_for(&settings, models);
        assert!(env.iter().any(|(name, value)| name == "DIGUP_DEVICE" && value == "cpu"));
    }
}
