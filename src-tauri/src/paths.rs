use std::path::PathBuf;

use tauri::{AppHandle, Manager};

/// Resolve the single, OS-specific app data directory for this app.
///
/// This is the one place that decides where notes, config, harness settings and
/// window state will live. Nothing is written yet in Phase 0 — callers only
/// resolve the location so no other module has to build paths by hand.
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

/// Expose the app data directory to the frontend so the path can be confirmed
/// during development.
#[tauri::command]
pub fn app_data_dir(app: AppHandle) -> Result<String, String> {
    let dir = ensure_app_data_dir(&app)?;
    Ok(dir.to_string_lossy().to_string())
}
