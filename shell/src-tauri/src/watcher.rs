//! Watching the chosen folders: a change where indexing reads asks the index helper to sync, two seconds after the last
//! one (a camera import or an unzip is a burst of events); in code search's folders a minute after the code goes quiet
//! (ten at most), as on the Mac, so the code index doesn't follow every save. `notify` uses ReadDirectoryChangesW on Windows, inotify on
//! Linux, FSEvents on a Mac. Changes where the crawl never goes are passed over (the engine's `Rules.isWalked`): hidden
//! folders (a leading dot, or Windows' hidden flag, which AppData has), developer folders, `.noindex` folders, skipped
//! folders, and DigUp's own index. With a home folder chosen, other apps' caches would ask for a sync every few seconds.
//! The paths of each burst go to the caller, which can pass over the echo of a move or delete DigUp just did itself
//! (`echo`).

use notify::{RecursiveMode, Watcher};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;

/// Folders the crawl never walks into, by name: the engine's `Rules.skippedFolderNames`.
const DEVELOPER_FOLDERS: [&str; 8] =
    ["node_modules", "bower_components", "venv", "site-packages", "__pycache__", "DerivedData", "Pods", "Carthage"];

pub struct FolderWatcher {
    _watcher: Option<notify::RecommendedWatcher>,
}

impl FolderWatcher {
    /// Watches `folders` (less `skipped` inside them); `changed` runs on the watcher's own thread after each burst of
    /// changes, with every path of the burst (none when the system dropped events): once nothing has changed for
    /// `quiet`, or `longest` after the burst began. `types` are the extensions the crawl skips; a change to a type it
    /// never indexes doesn't ask for a sync. A code watcher passes None: its types are the code index's. Dropping it
    /// stops.
    pub fn start(folders: &[PathBuf], skipped: Vec<PathBuf>, ignore: Option<PathBuf>, types: Option<Vec<String>>,
                 quiet: Duration, longest: Duration, changed: impl Fn(&[PathBuf]) + Send + 'static) -> FolderWatcher {
        let (sender, receiver) = mpsc::channel::<Vec<PathBuf>>();
        let roots = folders.to_vec();
        let watcher = notify::recommended_watcher(move |result: notify::Result<notify::Event>| {
            let Ok(event) = result else { return };
            if event.kind.is_access() {
                return;
            }
            let ignored = |path: &PathBuf| ignore.as_ref().is_some_and(|folder| path.starts_with(folder));
            // No paths: the system dropped events (an overflow), so anything may have changed: the burst keeps none,
            // and a sync runs.
            let empty = event.paths.is_empty();
            let paths: Vec<PathBuf> = event.paths.into_iter()
                .filter(|path| !ignored(path) && walked(path, &roots, &skipped)
                    && types.as_ref().is_none_or(|types| change_may_matter(path, types))).collect();
            if empty || !paths.is_empty() {
                let _ = sender.send(paths);
            }
        });
        let mut watcher = match watcher {
            Ok(watcher) => watcher,
            Err(error) => {
                eprintln!("digup-shell: can't watch folders: {error}");
                return FolderWatcher { _watcher: None };
            }
        };
        for folder in folders {
            if let Err(error) = watcher.watch(folder, RecursiveMode::Recursive) {
                eprintln!("digup-shell: can't watch {}: {error}", folder.display());
            }
        }
        std::thread::spawn(move || {
            while let Ok(mut paths) = receiver.recv() {
                let began = std::time::Instant::now();
                // An empty event is the system dropping events: a burst it joins needs a full sync, not its paths.
                let mut dropped = paths.is_empty();
                loop {
                    match receiver.recv_timeout(quiet) {
                        Ok(more) => {
                            if more.is_empty() {
                                dropped = true;
                            } else {
                                paths.extend(more);
                            }
                        }
                        Err(RecvTimeoutError::Timeout) => break,
                        Err(RecvTimeoutError::Disconnected) => return,
                    }
                    if began.elapsed() >= longest {
                        break;
                    }
                }
                if dropped {
                    paths.clear();
                }
                paths.sort_unstable();
                paths.dedup();
                changed(&paths);
            }
        });
        FolderWatcher { _watcher: Some(watcher) }
    }
}

/// The crawl reads `path`: it's inside a chosen folder, and nothing on the way to it from there is skipped. A skipped
/// folder counts only inside the chosen folders that contain it (one chosen inside it is read whole).
fn walked(path: &Path, roots: &[PathBuf], skipped: &[PathBuf]) -> bool {
    roots.iter().any(|root| {
        let Ok(below) = path.strip_prefix(root) else { return false };
        let mut folder = root.clone();
        below.components().all(|part| {
            folder.push(part);
            let name = part.as_os_str().to_string_lossy();
            !(name.starts_with('.')
                || DEVELOPER_FOLDERS.contains(&name.as_ref())
                || name.ends_with(".noindex")
                || skipped.contains(&folder)
                || hidden(&folder))
        })
    })
}

/// What the crawl indexes by extension (the portable `Rules.classify`): documents, PDFs and media matter; source
/// and data files are skipped unless code search's own index reads them. A change to anything else asks for no sync.
const DOCUMENT_EXTENSIONS: [&str; 15] = [
    "txt", "text", "md", "markdown", "typ", "rtf", "docx", "doc", "odt", "html", "htm", "xlsx", "pptx", "ods", "odp",
];
const IMAGE_EXTENSIONS: [&str; 25] = [
    "jpg", "jpeg", "jpe", "jfif", "png", "gif", "bmp", "tif", "tiff", "webp", "heic", "heif", "avif", "psd", "tga",
    "pnm", "ppm", "pgm", "cr2", "nef", "arw", "dng", "raf", "orf", "rw2",
];
const AUDIO_EXTENSIONS: [&str; 16] = [
    "mp3", "m4a", "m4b", "aac", "wav", "wave", "flac", "ogg", "oga", "opus", "aiff", "aif", "wma", "amr", "caf", "mka",
];
const VIDEO_EXTENSIONS: [&str; 15] = [
    "mp4", "m4v", "mov", "mkv", "webm", "avi", "wmv", "flv", "3gp", "3g2", "mpg", "mpeg", "m2v", "ogv", "mxf",
];
/// Mac bundles and library packages can sit on a shared disk; the crawl skips them whole (`FileSystem.packageExtensions`).
const PACKAGE_EXTENSIONS: [&str; 20] = [
    "app", "framework", "bundle", "plugin", "appex", "xpc", "kext", "xcodeproj", "xcworkspace", "playground",
    "photoslibrary", "photolibrary", "musiclibrary", "tvlibrary", "imovielibrary", "fcpbundle", "logicx", "band", "pkg", "dmg",
];

/// Whether a change at `path` can change what's indexed: a folder, or a file of a type the crawl reads, outside a
/// package (which the crawl skips whole) and not of a type the person excluded. A database rewritten every few seconds
/// would otherwise ask for a full sync each time. A path that's gone and has no extension may be a deleted folder.
fn change_may_matter(path: &Path, excluded: &[String]) -> bool {
    if is_package(path) {
        return false;
    }
    if path.is_dir() {
        return true;
    }
    let Some(extension) = path.extension().and_then(|extension| extension.to_str()).map(str::to_lowercase) else {
        // A file without an extension isn't indexed; a path that's gone without one may be a folder.
        return !path.exists();
    };
    if excluded.iter().any(|type_| type_.trim_start_matches('.').eq_ignore_ascii_case(&extension)) {
        return false;
    }
    // TypeScript's .ts and MPEG transport streams share an extension; a stream is video, and the rare .ts document
    // is worth a sync either way.
    if extension == "ts" || extension == "mts" || extension == "m2ts" || extension == "pdf" {
        return true;
    }
    DOCUMENT_EXTENSIONS.contains(&extension.as_str())
        || IMAGE_EXTENSIONS.contains(&extension.as_str())
        || AUDIO_EXTENSIONS.contains(&extension.as_str())
        || VIDEO_EXTENSIONS.contains(&extension.as_str())
}

/// A path inside a package (or the package folder itself), whose contents the crawl never reads.
fn is_package(path: &Path) -> bool {
    path.ancestors().any(|folder| {
        folder.extension().and_then(|extension| extension.to_str())
            .is_some_and(|extension| PACKAGE_EXTENSIONS.contains(&extension.to_lowercase().as_str()))
    })
}

/// Windows' hidden flag; elsewhere a leading dot is the rule, checked above.
#[cfg(windows)]
fn hidden(path: &Path) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
    std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_attributes() & FILE_ATTRIBUTE_HIDDEN != 0)
}

#[cfg(not(windows))]
fn hidden(_: &Path) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::{change_may_matter, walked};
    use std::path::{Path, PathBuf};

    #[test]
    fn only_changes_the_crawl_reads_matter() {
        let read = |path: &str| change_may_matter(&PathBuf::from(path), &[]);
        assert!(read("photo.jpg"));
        assert!(read("notes.md"));
        assert!(read("invoice.pdf"));
        assert!(read("clip.mp4"));
        assert!(read("tone.wav"));
        assert!(read("a.heic"));
        assert!(!read("server.log"));
        assert!(!read("collection-0.wt"));
        assert!(!read("metrics.interim.temp"));
        assert!(!read("data.json"));
        assert!(read("new folder"));   // gone and without an extension: a deleted folder
        assert!(!change_may_matter(Path::new("photo.jpg"), &["jpg".to_string()]));
        assert!(!read("Photos Library.photoslibrary/originals/a.heic"));
        assert!(!read("Tool.app/Contents/Resources/icon.png"));
    }

    #[test]
    fn walks_where_the_crawl_goes() {
        let home = PathBuf::from("/home/me");
        let roots = vec![home.clone(), home.join("Pictures/.private/shared")];
        let skipped = vec![home.join("Music"), home.join("Music/Mixes")];
        let read = |path: &str| walked(&home.join(path), &roots, &skipped);
        assert!(read("Documents/notes.md"));
        assert!(!read(".cache/thumbnails/x.png"));
        assert!(!read("code/app/node_modules/pkg/index.js"));
        assert!(!read("Archive.noindex/old.pdf"));
        assert!(!read("Music/song.mp3"));
        assert!(!read("Music/Mixes/one.mp3"));
        // A folder chosen inside a hidden or skipped one is read whole.
        assert!(read("Pictures/.private/shared/photo.jpg"));
        assert!(!walked(&PathBuf::from("/elsewhere/file.txt"), &roots, &skipped));
    }
}
