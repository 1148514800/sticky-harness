use std::path::PathBuf;

use tauri::{AppHandle, Manager};

/// The name the OS gives this app's data directory. Tauri derives it from the
/// bundle identifier, so the two must stay equal or a diagnostic written before
/// the app handle exists would land where the app never looks.
const APP_DATA_DIR_NAME: &str = "com.stickyharness.desktop";

/// The data directory's name, for the one caller that has to find it without an
/// app handle: the panic hook, which runs when the process may already be dying.
/// Every other path is resolved through the functions in this module.
pub fn app_data_dir_name() -> &'static str {
    APP_DATA_DIR_NAME
}

/// Resolve the single, OS-specific app data directory for this app.
///
/// This is the one place that decides where notes, config, harness settings and
/// window state will live, so no other module has to build paths by hand.
pub fn resolve_app_data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map_err(|error| format!("could not resolve the app data directory: {error}"))
}

/// Resolve the app data directory, creating it if needed.
pub fn ensure_app_data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = resolve_app_data_dir(app)?;

    std::fs::create_dir_all(&dir)
        .map_err(|error| format!("could not create {}: {error}", dir.display()))?;

    Ok(dir)
}

/// Resolve `<app data>/notes`, creating it if needed.
///
/// Each note is stored as `<note-id>.json` in this directory.
pub fn notes_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = ensure_app_data_dir(app)?.join("notes");

    std::fs::create_dir_all(&dir)
        .map_err(|error| format!("could not create {}: {error}", dir.display()))?;

    Ok(dir)
}

/// Resolve `<app data>/harness-task-window.json`.
///
/// The Harness Task Note is not a normal note, so its window config lives
/// beside `notes/` rather than inside it. The file name comes from here for the
/// same reason every other app data path does: one module decides where data
/// lives.
pub fn harness_task_window_config(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(ensure_app_data_dir(app)?.join("harness-task-window.json"))
}

/// Resolve `<app data>/harness-adapters.json`.
///
/// The adapter configuration is app configuration, not user content and not
/// harness state, so it lives beside `notes/` rather than inside it. A missing
/// file simply means no adapters are configured.
pub fn harness_adapters_config(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(ensure_app_data_dir(app)?.join("harness-adapters.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The name above is the one path fact this module cannot ask Tauri for, so
    /// it is checked against configuration rather than trusted to stay in sync.
    #[test]
    fn the_data_directory_name_matches_the_bundle_identifier() {
        let config: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json"))
            .expect("tauri.conf.json should parse");

        assert_eq!(
            config["identifier"].as_str(),
            Some(APP_DATA_DIR_NAME),
            "the app data directory name and the bundle identifier must stay equal"
        );
    }
}
