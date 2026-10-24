//! Changes DigUp itself made: a move or a delete updates the index itself (`file_ops`), so the folder watcher's echo of
//! the same change must not start a rescan. A move notes both ends, a delete the file, for a few seconds; on a Mac,
//! whose FSEvents report the folder, the folders too. A burst of watcher events is skipped only when every path in it
//! was noted. Copy is never noted: the copy has to be indexed. An index update that failed leaves no note, so the rescan
//! happens.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// How long a note lives: the watcher waits two seconds after the last change, and a burst of a big move takes a while.
/// A later, real change to the same path after the note is used (or this long) is not taken for our echo.
const REMEMBER: Duration = Duration::from_secs(15);

type Notes = HashMap<String, Instant>;

fn notes() -> &'static Mutex<Notes> {
    static NOTES: OnceLock<Mutex<Notes>> = OnceLock::new();
    NOTES.get_or_init(Mutex::default)
}

/// The key paths compare by: Windows doesn't tell `C:\Photos\x.JPG` from `c:\photos\X.jpg`; elsewhere a path is its
/// bytes.
fn key(path: &Path) -> String {
    let text = path.to_string_lossy().replace('/', std::path::MAIN_SEPARATOR_STR);
    #[cfg(windows)]
    let text = text.to_lowercase();
    text
}

fn remember(path: &Path, until: Instant) {
    notes().lock().unwrap().insert(key(path), until);
}

fn remember_with_parent(path: &Path, until: Instant) {
    remember(path, until);
    // FSEvents reports a rename as a change of the folder; ReadDirectoryChangesW names both ends. Noting the folder on
    // Windows would swallow a real change to any other file in it for these seconds.
    #[cfg(not(windows))]
    if let Some(parent) = path.parent() {
        remember(parent, until);
    }
}

/// A file that moved: both ends (and on a Mac, the folders they were in).
pub fn note_move(from: &Path, to: &Path) {
    let until = Instant::now() + REMEMBER;
    remember_with_parent(from, until);
    remember_with_parent(to, until);
}

/// A file that was deleted (to the Recycle Bin).
pub fn note_file(path: &Path) {
    remember_with_parent(path, Instant::now() + REMEMBER);
}

/// Whether every path in this watcher burst is one DigUp itself changed. When it is, the notes are used up: an event
/// for the same path later (a real change, not the echo) asks for a sync. A burst with anything else in it is not ours
/// and asks for one too; its notes stay for the next burst.
pub fn ours(paths: &[PathBuf]) -> bool {
    if paths.is_empty() {
        return false;   // the system dropped events: anything may have changed
    }
    let now = Instant::now();
    let mut notes = notes().lock().unwrap();
    notes.retain(|_, until| *until > now);
    if !paths.iter().all(|path| notes.contains_key(&key(path))) {
        return false;
    }
    for path in paths {
        notes.remove(&key(path));
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

    fn clean() -> std::sync::MutexGuard<'static, ()> {
        let guard = ONE_AT_A_TIME.lock().unwrap_or_else(|error| error.into_inner());
        notes().lock().unwrap().clear();
        guard
    }

    #[test]
    fn a_move_covers_both_ends_and_on_a_mac_their_folders() {
        let _guard = clean();
        let from = Path::new("/one/photo.jpg");
        let to = Path::new("/two/photo (2).jpg");
        note_move(from, to);
        assert!(ours(&[PathBuf::from("/one/photo.jpg"), PathBuf::from("/two/photo (2).jpg")]));
        #[cfg(not(windows))]
        {
            assert!(ours(&[PathBuf::from("/one")]));
            assert!(ours(&[PathBuf::from("/two")]));
        }
        #[cfg(windows)]
        {
            // Windows names the file itself: a change to another file in the folder is real.
            assert!(!ours(&[PathBuf::from("/one")]));
            assert!(!ours(&[PathBuf::from("/two")]));
        }
        assert!(!ours(&[PathBuf::from("/one/other.jpg")]));
    }

    #[test]
    fn a_delete_covers_the_file_and_on_a_mac_its_folder() {
        let _guard = clean();
        let file = Path::new("/one/gone.png");
        note_file(file);
        #[cfg(not(windows))]
        assert!(ours(&[PathBuf::from("/one")]));
        #[cfg(windows)]
        assert!(!ours(&[PathBuf::from("/one")]));
        assert!(ours(&[file.to_path_buf()]));
        // Used up: the same path changing again is not our echo.
        assert!(!ours(&[file.to_path_buf()]));
    }

    #[test]
    fn a_burst_with_anything_else_is_not_ours() {
        let _guard = clean();
        let file = PathBuf::from("/one/photo.jpg");
        note_file(&file);
        assert!(!ours(&[file.clone(), PathBuf::from("/one/notes.txt")]));
        // The note waited for the echo's own burst.
        assert!(ours(&[file]));
    }

    #[test]
    fn an_expired_note_does_not_cover() {
        let _guard = clean();
        let file = Path::new("/one/photo.jpg");
        note_file(file);
        let old = Instant::now() - Duration::from_secs(1);
        notes().lock().unwrap().insert(key(file), old);
        assert!(!ours(&[file.to_path_buf()]));
    }

    #[test]
    fn nothing_is_ours_without_paths() {
        let _guard = clean();
        note_file(Path::new("/one/photo.jpg"));
        assert!(!ours(&[]));
    }

    #[cfg(windows)]
    #[test]
    fn windows_compares_without_case() {
        let _guard = clean();
        note_move(Path::new("C:/One/Photo.JPG"), Path::new("C:/Two/Photo (2).jpg"));
        assert!(ours(&[PathBuf::from("c:\\one\\photo.jpg")]));
        assert!(ours(&[PathBuf::from("c:\\two\\photo (2).JPG")]));
    }
}
