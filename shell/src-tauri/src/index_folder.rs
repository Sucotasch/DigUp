//! The index's own folder: the two SQLite databases (with their WAL and shared-memory files), what indexing measured,
//! and the note a watchdog leaves. Settings can move all of it to another disk; the helpers must be stopped first, and
//! the old files leave only after the copy is in place. Logs stay where they were: they're small and not the index.

use crate::models;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// Everything that belongs to the index. `-wal`, `-shm` and `-journal` are SQLite's; `stopped-file.json` is the
/// watchdog's; `costs.json` is what indexing measured for the estimates.
const DATA_FILES: [&str; 10] = [
    "index.sqlite", "index.sqlite-wal", "index.sqlite-shm", "index.sqlite-journal",
    "code.sqlite", "code.sqlite-wal", "code.sqlite-shm", "code.sqlite-journal",
    "costs.json", "stopped-file.json",
];

const MOVE_SUFFIX: &str = ".digup-moving";
/// Room for SQLite to grow while the index settles in its new folder.
const SPACE_MARGIN: u64 = 100 << 20;

/// The index files that are in `folder` now.
pub fn data_files(folder: &Path) -> Vec<PathBuf> {
    DATA_FILES.iter().map(|name| folder.join(name)).filter(|path| path.is_file()).collect()
}

/// How much the index takes on disk now.
pub fn bytes(folder: &Path) -> u64 {
    data_files(folder).iter().filter_map(|path| std::fs::metadata(path).ok()).map(|file| file.len()).sum()
}

/// A file with one of the index's names is already in `folder` (another DigUp index, or a folder that isn't empty).
fn conflict(folder: &Path) -> Option<PathBuf> {
    DATA_FILES.iter().map(|name| folder.join(name)).find(|path| path.exists())
}

/// What moving the index to `target` would mean: `same`, `move`, or `invalid` with a reason. Fast: sizes, not a copy.
pub fn plan(current: &Path, target: &Path) -> Value {
    let bytes = bytes(current);
    let free = models::free_space(target);
    let invalid = |problem: String| json!({
        "kind": "invalid", "current": current.display().to_string(), "target": target.display().to_string(),
        "bytes": bytes, "free": free, "problem": problem,
    });
    if !target.is_absolute() {
        return invalid("Choose a full path.".into());
    }
    if models::same_path(current, target) {
        return json!({
            "kind": "same", "current": current.display().to_string(), "target": target.display().to_string(),
            "bytes": bytes, "free": free, "problem": null,
        });
    }
    if inside(target, current) {
        return invalid("Choose a folder outside the current index folder.".into());
    }
    match models::folder_state(target, None) {
        models::FolderState::Unavailable => return invalid("That disk isn't available.".into()),
        models::FolderState::NotFolder => return invalid(format!("{} isn't a folder.", target.display())),
        // WrongVolume can't come back here: it needs an expected volume, and a move to another disk is allowed.
        models::FolderState::Available | models::FolderState::Missing | models::FolderState::WrongVolume => {}
    }
    if let Some(file) = conflict(target) {
        return invalid(format!("That folder already has an index ({}): Use Existing Index… works on it where it is.",
                               file.display()));
    }
    let previews = target.join("previews");
    if previews.exists() {
        return invalid(format!("That folder already has DigUp's previews ({}).", previews.display()));
    }
    let needed = bytes.saturating_add(SPACE_MARGIN);
    if free.is_some_and(|free| free < needed) {
        return invalid(format!(
            "Not enough free space in {}: {} needed, {} free.",
            target.display(),
            models::human_bytes(needed),
            models::human_bytes(free.unwrap_or(0))
        ));
    }
    json!({
        "kind": "move", "current": current.display().to_string(), "target": target.display().to_string(),
        "bytes": bytes, "free": free, "problem": null,
    })
}

/// Copies the index's files to `target` through temporary names; on any failure the copies this call made are removed
/// again and `source` is left untouched. Returns how many bytes were copied.
pub fn copy_to(source: &Path, target: &Path) -> Result<u64, String> {
    let mut copied: Vec<PathBuf> = Vec::new();
    let mut made_target = false;
    if !target.exists() {
        std::fs::create_dir_all(target)
            .map_err(|error| format!("Can't make {}: {error}", target.display()))?;
        made_target = true;
    }
    let result = (|| -> Result<u64, String> {
        let mut total = 0;
        for name in DATA_FILES {
            let from = source.join(name);
            if !from.is_file() {
                continue;
            }
            let to = target.join(name);
            if to.exists() {
                return Err(format!("{} appeared in the new folder while the index was moving.", to.display()));
            }
            let temporary = target.join(format!("{name}{MOVE_SUFFIX}"));
            let _ = std::fs::remove_file(&temporary);
            let mut input = std::fs::File::open(&from)
                .map_err(|error| format!("Can't read {}: {error}", from.display()))?;
            let mut output = std::fs::File::create(&temporary)
                .map_err(|error| format!("Can't write {}: {error}", temporary.display()))?;
            std::io::copy(&mut input, &mut output)
                .map_err(|error| format!("Can't copy {}: {error}", from.display()))?;
            output.sync_all().map_err(|error| format!("Can't finish {}: {error}", temporary.display()))?;
            drop(output);
            drop(input);
            std::fs::rename(&temporary, &to)
                .map_err(|error| format!("Can't put {} in place: {error}", to.display()))?;
            copied.push(to);
            total += std::fs::metadata(&from).map(|file| file.len()).unwrap_or(0);
        }
        Ok(total)
    })();
    if result.is_err() {
        for path in copied {
            let _ = std::fs::remove_file(path);
        }
        for name in DATA_FILES {
            let _ = std::fs::remove_file(target.join(format!("{name}{MOVE_SUFFIX}")));
        }
        if made_target {
            let _ = std::fs::remove_dir(target);
        }
    }
    result
}

/// Removes the index's files from the old folder, retrying while Windows lets go of a database. Returns the paths that
/// couldn't be removed; a stranger in the folder is never touched.
pub fn cleanup_files(folder: &Path) -> Vec<PathBuf> {
    let mut left = Vec::new();
    for name in DATA_FILES {
        let path = folder.join(name);
        if !path.exists() {
            continue;
        }
        let mut removed = false;
        for attempt in 0..20 {
            match std::fs::remove_file(&path) {
                Ok(()) => {
                    removed = true;
                    break;
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    removed = true;
                    break;
                }
                Err(_) if attempt < 19 => std::thread::sleep(std::time::Duration::from_millis(100)),
                Err(_) => break,
            }
        }
        if !removed {
            left.push(path);
        }
    }
    left
}

/// The index has moved: its files and the previews it made go (previews are remade where the index goes). Only for the
/// index's own folder, never for a folder a copy is being rolled back from.
pub fn cleanup(folder: &Path) -> Vec<PathBuf> {
    let mut left = cleanup_files(folder);
    let previews = folder.join("previews");
    if previews.is_dir() {
        let mut removed = false;
        for attempt in 0..20 {
            match std::fs::remove_dir_all(&previews) {
                Ok(()) => {
                    removed = true;
                    break;
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    removed = true;
                    break;
                }
                Err(_) if attempt < 19 => std::thread::sleep(std::time::Duration::from_millis(100)),
                Err(_) => break,
            }
        }
        if !removed {
            left.push(previews);
        }
    }
    left
}

/// Whether `path` is inside `folder` (both normalized the way Windows compares paths).
fn inside(path: &Path, folder: &Path) -> bool {
    let path = normalized(path);
    let folder = normalized(folder);
    !folder.is_empty() && path.len() > folder.len()
        && path.starts_with(&format!("{folder}{}", std::path::MAIN_SEPARATOR))
}

fn normalized(path: &Path) -> String {
    let separator = std::path::MAIN_SEPARATOR;
    let separator_text = separator.to_string();
    let mut text = path.to_string_lossy().replace('/', &separator_text);
    while text.ends_with(separator) {
        text.pop();
    }
    if cfg!(windows) {
        text.make_ascii_lowercase();
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("DigUp-index-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        folder
    }

    #[test]
    fn only_the_index_files_are_moved_and_the_old_folder_keeps_strangers() {
        let old = folder("move-old");
        let new = old.parent().unwrap().join(format!("{}-new", old.file_name().unwrap().to_string_lossy()));
        let _ = std::fs::remove_dir_all(&new);
        std::fs::write(old.join("index.sqlite"), b"index").unwrap();
        std::fs::write(old.join("index.sqlite-wal"), b"wal").unwrap();
        std::fs::write(old.join("code.sqlite"), b"code").unwrap();
        std::fs::write(old.join("costs.json"), b"{}").unwrap();
        std::fs::write(old.join("worker.log"), b"log").unwrap();
        std::fs::write(old.join("notes.txt"), b"keep").unwrap();
        std::fs::create_dir(old.join("previews")).unwrap();
        std::fs::write(old.join("previews").join("page.jpg"), b"jpeg").unwrap();

        let plan = plan(&old, &new);
        assert_eq!(plan["kind"], "move");
        assert_eq!(copy_to(&old, &new).unwrap(), 14);
        assert!(new.join("index.sqlite").is_file());
        assert!(new.join("index.sqlite-wal").is_file());
        assert!(new.join("code.sqlite").is_file());
        assert!(new.join("costs.json").is_file());
        assert!(!new.join("worker.log").exists());
        assert!(!new.join("notes.txt").exists());

        assert!(cleanup(&old).is_empty());
        assert!(!old.join("index.sqlite").exists());
        assert!(old.join("worker.log").exists(), "logs are not the index");
        assert!(old.join("notes.txt").exists(), "a stranger is never touched");
        assert!(!old.join("previews").exists(), "previews are remade in the new folder");
        let _ = std::fs::remove_dir_all(&new);
        let _ = std::fs::remove_dir_all(&old);
    }

    #[test]
    fn a_failed_copy_leaves_the_source_and_the_target_as_they_were() {
        let old = folder("copy-old");
        let new = folder("copy-new");
        std::fs::write(old.join("index.sqlite"), b"index").unwrap();
        std::fs::write(old.join("costs.json"), b"costs").unwrap();
        std::fs::write(new.join("costs.json"), b"keep").unwrap();

        let error = copy_to(&old, &new).unwrap_err();
        assert!(error.contains("appeared in the new folder"), "{error}");
        assert!(old.join("index.sqlite").is_file());
        assert_eq!(std::fs::read(old.join("costs.json")).unwrap(), b"costs");
        assert!(!new.join("index.sqlite").exists(), "the copy is rolled back");
        assert_eq!(std::fs::read(new.join("costs.json")).unwrap(), b"keep");
        let _ = std::fs::remove_dir_all(&old);
        let _ = std::fs::remove_dir_all(&new);
    }

    #[test]
    fn a_folder_with_an_index_is_refused_and_so_is_the_same_one() {
        let current = folder("plan-current");
        let target = folder("plan-target");
        assert_eq!(plan(&current, &target)["kind"], "move");
        std::fs::write(target.join("index.sqlite"), b"other").unwrap();
        let refused = plan(&current, &target);
        assert_eq!(refused["kind"], "invalid");
        assert!(refused["problem"].as_str().unwrap().contains("already has an index"));
        assert_eq!(plan(&current, &current)["kind"], "same");
        let inside = current.join("nested");
        assert_eq!(plan(&current, &inside)["kind"], "invalid");
        let with_previews = folder("plan-previews");
        std::fs::create_dir(with_previews.join("previews")).unwrap();
        let refused = plan(&current, &with_previews);
        assert_eq!(refused["kind"], "invalid");
        assert!(refused["problem"].as_str().unwrap().contains("previews"));
        let _ = std::fs::remove_dir_all(&current);
        let _ = std::fs::remove_dir_all(&target);
        let _ = std::fs::remove_dir_all(&with_previews);
    }
}
