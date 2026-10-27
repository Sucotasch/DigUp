//! What the person chose: the folders DigUp searches, what it skips in them, the hotkey, and whether setup is done. A
//! JSON file in the app's config folder (%APPDATA%\com.sucotasch.digupplus on Windows,
//! ~/.config/com.sucotasch.digupplus on Linux). Launch arguments (`--roots`, `--hotkey`) stand in for it in dev runs.

use crate::hours::Hours;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    /// The folders searched and kept indexed.
    pub folders: Vec<String>,
    /// Folders inside them that aren't (`FolderSelection` in the engine).
    pub skipped: Vec<String>,
    /// File extensions left out ("heic", "txt").
    pub skip_types: Vec<String>,
    /// The panel's hotkey (Tauri's accelerator form); empty: the default.
    pub hotkey: String,
    /// Onboarding is done: the shell starts in the tray.
    pub set_up: bool,
    /// The panel sits centered where the desktop places windows itself (Wayland), in a window that fills the screen.
    pub center_panel: bool,
    /// Index only between these times of day (hours.rs); the times are kept while it's off.
    pub index_hours_on: bool,
    pub index_hours: Hours,
    /// Code search: its folders (an index of its own, code.sqlite), the folders skipped inside them, and the editor its
    /// results open in (editors.rs: an editor's id, "" for the first one found, "default" for the file's default app).
    pub code_folders: Vec<String>,
    pub code_skipped: Vec<String>,
    pub code_editor: String,
    /// Code search was turned off and its index is still to be deleted (Windows won't delete a file a helper has open).
    pub delete_code_index: bool,
    /// Where the window was left, for the next time it opens.
    pub window: Option<WindowPlace>,
    /// A click on the tray icon (Windows): "panel" shows the search panel, anything else opens the window.
    pub tray_click: String,
    /// The model runs on the processor even where the graphics card measured faster (Settings → General): for a card
    /// whose results come back wrong or empty. The helpers get `DIGUP_DEVICE=cpu`.
    pub processor_only: bool,
    /// Asking before moving or copying several files was turned off (Settings → General): one file never asks.
    pub confirm_move_copy_off: bool,
    /// Asking before deleting (to the Recycle Bin) was turned off (Settings → General).
    pub confirm_delete_off: bool,
    /// The folder with both model files (Settings → Storage). None: the default folder.
    pub models_folder: Option<String>,
    /// The volume the folder was chosen on: a drive letter that comes back with another disk must not be written to.
    pub models_volume: Option<u64>,
    /// The person deleted the model files: don't download them again on the next start.
    pub model_removed: bool,
    /// The old folder after a move: its files go once the new model is ready (retried at the next start on failure).
    pub model_cleanup: Option<String>,
    /// Where the index lives (Settings → Storage). None: the default folder under the app's data folder.
    pub index_folder: Option<String>,
}

/// The window's size inside its frame (logical pixels), where it was (physical; Wayland doesn't tell), and whether it
/// filled the screen.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct WindowPlace {
    pub width: f64,
    pub height: f64,
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub maximized: bool,
}

impl Settings {
    /// The file's settings, or the defaults when there's none (or it can't be read).
    pub fn load(path: &Path) -> Settings {
        std::fs::read(path).ok().and_then(|data| serde_json::from_slice(&data).ok()).unwrap_or_default()
    }

    /// Written whole to a temporary file first, so a crash never leaves half of it.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(folder) = path.parent() {
            std::fs::create_dir_all(folder)?;
        }
        let temporary = path.with_extension("json.new");
        std::fs::write(&temporary, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(temporary, path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_settings_survive_a_round_trip_and_older_files_default_them() {
        let folder = std::env::temp_dir().join(format!("DigUp-settings-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join("settings.json");
        let settings = Settings { models_folder: Some("G:\\Ai\\Модели".into()), models_volume: Some(42),
                                      model_removed: true, model_cleanup: Some("C:\\old".into()),
                                      index_folder: Some("H:\\DigUp\\Index".into()), ..Settings::default() };
        settings.save(&path).unwrap();
        let loaded = Settings::load(&path);
        assert_eq!(loaded.models_folder, settings.models_folder);
        assert_eq!(loaded.models_volume, Some(42));
        assert!(loaded.model_removed);
        assert_eq!(loaded.model_cleanup, settings.model_cleanup);
        assert_eq!(loaded.index_folder, settings.index_folder);
        std::fs::write(&path, br#"{"setUp":true}"#).unwrap();
        let older = Settings::load(&path);
        assert!(older.models_folder.is_none() && older.models_volume.is_none() && !older.model_removed);
        assert!(older.index_folder.is_none());
        let _ = std::fs::remove_dir_all(&folder);
    }
}
