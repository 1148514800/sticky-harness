use std::path::PathBuf;

use tauri::{AppHandle, Manager};

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
