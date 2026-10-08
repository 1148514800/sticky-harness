//! The Harness Task Note: one read-only window that shows what is running.
//!
//! This is deliberately a separate module from [`notes`](crate::notes). A normal
//! note is user content that owns a JSON file, a Markdown body and the rule that
//! closing it deletes it. The Harness Task Note is none of those things: it is a
//! read-only view of [`HarnessRegistry`](crate::harness::registry::HarnessRegistry),
//! it stores no task data, and closing it hides it rather than destroying it.
//! Keeping them apart is what stops `notes.rs` from becoming the module that
//! knows about every kind of window.
//!
//! What is shared with a normal note is only the geometry idea, so the small
//! helpers below take a label and a state rather than a note id.

use std::fs;
use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{
    AppHandle, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder, WindowEvent,
};

use crate::paths;

/// The window label. Fixed and explicit, never `note-<id>`, so `App.tsx` can
/// tell the two window kinds apart from the label alone.
pub const HARNESS_TASK_LABEL: &str = "harness-tasks";
/// The window title, which is also the heading inside the window.
pub const HARNESS_TASK_TITLE: &str = "Harness Tasks";
const DEFAULT_WIDTH: u32 = 360;
const DEFAULT_HEIGHT: u32 = 420;
/// Small enough to stay usable, large enough for one task entry plus its header.
const MIN_WIDTH: f64 = 240.0;
const MIN_HEIGHT: f64 = 180.0;
/// Offset used when there is no saved geometry, so it does not hide behind a note.
const FIRST_POSITION: i32 = 120;
/// Quiet period after the last move/resize before geometry is written.
const WINDOW_STATE_DEBOUNCE: Duration = Duration::from_millis(400);
/// A restored window must be at least this visible on some monitor.
const MIN_VISIBLE_WIDTH: f64 = 80.0;
const MIN_VISIBLE_HEIGHT: f64 = 40.0;

/// The little that is persisted about this window.
///
/// Geometry and `always_on_top` only. Task data is never written here: it comes
/// from the registry and is gone when the app exits, so there is nothing to
/// store and nothing that can go stale on disk.
///
/// `created` is what makes the window survive a restart once the user has asked
/// for it, without needing a way to create it implicitly. An absent file means
/// the user has never opened the window.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HarnessWindowConfig {
    /// Whether the user has ever opened this window.
    #[serde(default)]
    pub created: bool,
    #[serde(default)]
    pub x: Option<i32>,
    #[serde(default)]
    pub y: Option<i32>,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
    #[serde(default)]
    pub always_on_top: bool,
}

/// Read the saved config, or a fresh one when there is nothing usable.
///
/// A corrupt or unreadable file is reported and treated as "not created yet":
/// losing a window's position is a much smaller problem than being unable to
/// start, and the user can simply open the window again.
fn load_config(app: &AppHandle) -> HarnessWindowConfig {
    let path = match config_path(app) {
        Ok(path) => path,
        Err(error) => {
            eprintln!("[sticky-harness] {error}");
            return HarnessWindowConfig::default();
        }
    };

    if !path.exists() {
        return HarnessWindowConfig::default();
    }

    match fs::read_to_string(&path) {
        Ok(raw) => match serde_json::from_str(&raw) {
            Ok(config) => config,
            Err(error) => {
                eprintln!(
                    "[sticky-harness] skipping the unreadable harness window config {}: {error}",
                    path.display()
                );
                HarnessWindowConfig::default()
            }
        },
        Err(error) => {
            eprintln!(
                "[sticky-harness] could not read {}: {error}",
                path.display()
            );
            HarnessWindowConfig::default()
        }
    }
}

fn config_path(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    paths::harness_task_window_config(app)
}

/// Write the config, temp file then rename, like a note record.
///
/// `panic = "abort"` in release and an interrupted write are both reasons not to
/// truncate the live file in place.
fn write_config(path: &Path, config: &HarnessWindowConfig) -> Result<(), String> {
    let json = serde_json::to_string_pretty(config)
        .map_err(|error| format!("could not serialise the harness window config: {error}"))?;

    let temp = path.with_extension("json.tmp");
    fs::write(&temp, json)
        .map_err(|error| format!("could not write {}: {error}", temp.display()))?;

    if let Err(rename_error) = fs::rename(&temp, path) {
        if let Err(copy_error) = fs::copy(&temp, path) {
            let _ = fs::remove_file(&temp);
            return Err(format!(
                "could not replace {}: {rename_error} (copy fallback also failed: {copy_error})",
                path.display()
            ));
        }
        eprintln!(
            "[sticky-harness] rename was refused for {} ({rename_error}); used the copy fallback",
            path.display()
        );
    }

    let _ = fs::remove_file(&temp);
    Ok(())
}

fn save_config(app: &AppHandle, config: &HarnessWindowConfig) -> Result<(), String> {
    write_config(&config_path(app)?, config)
}

/// The current window, if it is open.
///
/// Named `window_of` rather than `window` because several functions here take a
/// `window` parameter, and a helper that shares the name is shadowed inside
/// them - which reads as a mysterious "expected function" error.
fn window_of(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window(HARNESS_TASK_LABEL)
}

/// Where a window with no usable saved geometry should appear.
fn default_position(app: &AppHandle) -> PhysicalPosition<i32> {
    let origin = app
        .primary_monitor()
        .ok()
        .flatten()
        .map(|monitor| *monitor.position())
        .unwrap_or(PhysicalPosition::new(0, 0));

    PhysicalPosition::new(origin.x + FIRST_POSITION, origin.y + FIRST_POSITION)
}

/// Whether a rect still overlaps some existing monitor enough to be usable.
fn rect_visible(
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    monitors: &[(i32, i32, u32, u32)],
) -> bool {
    monitors.iter().any(|(mx, my, mw, mh)| {
        let overlap_x = (x + width as i32).min(mx + *mw as i32) - x.max(*mx);
        let overlap_y = (y + height as i32).min(my + *mh as i32) - y.max(*my);
        overlap_x as f64 >= MIN_VISIBLE_WIDTH && overlap_y as f64 >= MIN_VISIBLE_HEIGHT
    })
}

fn monitor_bounds(app: &AppHandle) -> Vec<(i32, i32, u32, u32)> {
    app.available_monitors()
        .map(|monitors| {
            monitors
                .into_iter()
                .map(|monitor| {
                    let position = monitor.position();
                    let size = monitor.size();
                    (position.x, position.y, size.width, size.height)
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Apply saved geometry, keeping the window reachable on a monitor that exists.
fn apply_config(app: &AppHandle, window: &WebviewWindow, config: &HarnessWindowConfig) {
    let width = config.width.unwrap_or(DEFAULT_WIDTH).max(MIN_WIDTH as u32);
    let height = config
        .height
        .unwrap_or(DEFAULT_HEIGHT)
        .max(MIN_HEIGHT as u32);

    if let Err(error) = window.set_size(PhysicalSize::new(width, height)) {
        eprintln!("[sticky-harness] could not restore the harness window size: {error}");
    }

    let monitors = monitor_bounds(app);
    let position = match (config.x, config.y) {
        (Some(x), Some(y)) if rect_visible(x, y, width, height, &monitors) => {
            PhysicalPosition::new(x, y)
        }
        (Some(_), Some(_)) => {
            // The monitor this window lived on is gone; fall back rather than
            // restoring it somewhere the user cannot reach.
            eprintln!(
                "[sticky-harness] saved harness window position is off-screen; using a fallback position"
            );
            default_position(app)
        }
        _ => default_position(app),
    };

    if let Err(error) = window.set_position(position) {
        eprintln!("[sticky-harness] could not restore the harness window position: {error}");
    }
}

/// Read the window's geometry back and persist it.
fn persist_geometry(app: &AppHandle) {
    let Some(window) = window_of(app) else {
        return;
    };

    let (Ok(position), Ok(size)) = (window.outer_position(), window.inner_size()) else {
        eprintln!("[sticky-harness] could not read the harness window geometry");
        return;
    };

    let mut config = load_config(app);
    let unchanged = config.x == Some(position.x)
        && config.y == Some(position.y)
        && config.width == Some(size.width)
        && config.height == Some(size.height);
    if unchanged {
        return;
    }

    config.created = true;
    config.x = Some(position.x);
    config.y = Some(position.y);
    config.width = Some(size.width);
    config.height = Some(size.height);

    if let Err(error) = save_config(app, &config) {
        eprintln!("[sticky-harness] could not save the harness window geometry: {error}");
    }
}

/// Coalesce a burst of move/resize events into one write.
fn schedule_geometry_save(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(WINDOW_STATE_DEBOUNCE).await;
        persist_geometry(&app);
    });
}

/// Wire up the two behaviours this window has.
///
/// Closing hides: this window is a view of live state, not user content, so `X`
/// must not throw away the fact that the user wants it. The tray item brings it
/// back, and the registry, the API and every note are untouched by either.
fn register_window_events(app: &AppHandle, window: &WebviewWindow) {
    let app = app.clone();

    window.on_window_event(move |event| match event {
        WindowEvent::CloseRequested { api, .. } => {
            // Prevent the close so the window (and its polling view) survives;
            // hiding is the documented meaning of X here.
            api.prevent_close();
            if let Some(open_window) = window_of(&app) {
                if let Err(error) = open_window.hide() {
                    eprintln!("[sticky-harness] could not hide the harness window: {error}");
                }
            }
        }
        WindowEvent::Moved(_) | WindowEvent::Resized(_) => schedule_geometry_save(&app),
        _ => {}
    });
}

/// Show the existing window, or create it the first time.
///
/// The tray item, the startup restore and the test helpers all come through
/// here, so there is exactly one way this window comes into being and exactly
/// one place that enforces "at most one".
///
/// Returns `true` when a new window was created.
pub fn open(app: &AppHandle) -> Result<bool, String> {
    if let Some(window) = window_of(app) {
        // Already open: show it if it was hidden, then bring it forward. Never
        // create a second one.
        if !window.is_visible().unwrap_or(false) {
            window
                .show()
                .map_err(|error| format!("could not show the harness window: {error}"))?;
        }
        let _ = window.set_focus();
        return Ok(false);
    }

    let mut config = load_config(app);
    // Opening the window is what makes it persist across restarts.
    config.created = true;

    let window = WebviewWindowBuilder::new(app, HARNESS_TASK_LABEL, WebviewUrl::default())
        .title(HARNESS_TASK_TITLE)
        .inner_size(DEFAULT_WIDTH as f64, DEFAULT_HEIGHT as f64)
        .min_inner_size(MIN_WIDTH, MIN_HEIGHT)
        .resizable(true)
        .always_on_top(config.always_on_top)
        // Hidden until the saved geometry is applied, so it never flashes.
        .visible(false)
        .build()
        .map_err(|error| format!("could not create the harness window: {error}"))?;

    apply_config(app, &window, &config);
    register_window_events(app, &window);

    if let Err(error) = save_config(app, &config) {
        eprintln!("[sticky-harness] could not record the harness window: {error}");
    }

    window
        .show()
        .map_err(|error| format!("could not show the harness window: {error}"))?;
    let _ = window.set_focus();

    Ok(true)
}

/// Recreate the window on startup, but only if the user ever opened it.
/// (Disabled: startup should restore only notes; tasks open manually.)
#[allow(dead_code)]
///
/// Harness *state* is not restored - the registry starts empty on purpose - so
/// the window comes back showing "No running tasks" until a harness reports.
pub fn restore(app: &AppHandle) -> Result<bool, String> {
    let config = load_config(app);
    if !config.created {
        return Ok(false);
    }

    if let Some(window) = window_of(app) {
        // Restored before this ran; just make sure it is on screen and honest
        // about its pin state.
        let _ = window.show();
        return Ok(false);
    }

    let window = WebviewWindowBuilder::new(app, HARNESS_TASK_LABEL, WebviewUrl::default())
        .title(HARNESS_TASK_TITLE)
        .inner_size(DEFAULT_WIDTH as f64, DEFAULT_HEIGHT as f64)
        .min_inner_size(MIN_WIDTH, MIN_HEIGHT)
        .resizable(true)
        .always_on_top(config.always_on_top)
        .visible(false)
        .build()
        .map_err(|error| format!("could not restore the harness window: {error}"))?;

    apply_config(app, &window, &config);
    register_window_events(app, &window);

    window
        .show()
        .map_err(|error| format!("could not show the restored harness window: {error}"))?;

    Ok(true)
}

/// Turn always-on-top on or off for this window and remember it.
///
/// Same meaning as Pin on a normal note, but stored in this window's own config
/// rather than a note file.
pub fn set_pinned(app: &AppHandle, pinned: bool) -> Result<(), String> {
    if let Some(window) = window_of(app) {
        window
            .set_always_on_top(pinned)
            .map_err(|error| format!("could not change always-on-top: {error}"))?;
    }

    let mut config = load_config(app);
    config.always_on_top = pinned;
    config.created = true;
    save_config(app, &config)
}

/// Show the window if it exists, without creating it.
pub fn show_if_open(app: &AppHandle) -> bool {
    let Some(window) = window_of(app) else {
        return false;
    };
    let _ = window.show();
    true
}

/// Hide the window if it exists, without destroying it.
pub fn hide_if_open(app: &AppHandle) -> bool {
    let Some(window) = window_of(app) else {
        return false;
    };
    if let Err(error) = window.hide() {
        eprintln!("[sticky-harness] could not hide the harness window: {error}");
        return false;
    }
    true
}

/// Open the Harness Tasks window, creating it if needed.
#[tauri::command]
pub async fn open_harness_tasks_window(app: AppHandle) -> Result<bool, String> {
    open(&app)
}

/// Whether the window exists and is currently on screen.
#[tauri::command]
pub async fn harness_window_status(app: AppHandle) -> Result<HarnessWindowStatus, String> {
    let Some(window) = window_of(&app) else {
        return Ok(HarnessWindowStatus {
            open: false,
            visible: false,
            pinned: false,
        });
    };

    Ok(HarnessWindowStatus {
        open: true,
        visible: window.is_visible().unwrap_or(false),
        pinned: window.is_always_on_top().unwrap_or(false),
    })
}

/// What the frontend needs to know about this window, without guessing.
#[derive(Debug, Clone, Serialize)]
pub struct HarnessWindowStatus {
    pub open: bool,
    pub visible: bool,
    pub pinned: bool,
}

/// Pin or unpin this window.
#[tauri::command]
pub async fn set_harness_window_pinned(app: AppHandle, pinned: bool) -> Result<(), String> {
    set_pinned(&app, pinned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_config_round_trips_through_json() {
        let config = HarnessWindowConfig {
            created: true,
            x: Some(120),
            y: Some(140),
            width: Some(360),
            height: Some(420),
            always_on_top: true,
        };

        let json = serde_json::to_string(&config).unwrap();
        let parsed: HarnessWindowConfig = serde_json::from_str(&json).unwrap();

        assert!(parsed.created);
        assert_eq!(parsed.x, Some(120));
        assert_eq!(parsed.y, Some(140));
        assert_eq!(parsed.width, Some(360));
        assert_eq!(parsed.height, Some(420));
        assert!(parsed.always_on_top);
    }

    #[test]
    fn an_empty_config_is_not_created() {
        let parsed: HarnessWindowConfig = serde_json::from_str("{}").unwrap();

        assert!(
            !parsed.created,
            "an absent file must not restore the window"
        );
        assert!(parsed.x.is_none());
        assert!(!parsed.always_on_top);
    }

    #[test]
    fn a_partial_config_loads_and_only_carries_what_it_has() {
        let parsed: HarnessWindowConfig =
            serde_json::from_str(r#"{ "created": true, "width": 400, "height": 500 }"#).unwrap();

        assert!(parsed.created);
        assert_eq!(parsed.width, Some(400));
        assert_eq!(parsed.height, Some(500));
        assert!(
            parsed.x.is_none(),
            "missing geometry falls back, it does not fail"
        );
        assert!(!parsed.always_on_top);
    }

    #[test]
    fn a_window_rect_that_touches_a_monitor_is_visible() {
        let monitors = [(0, 0, 1920, 1080)];

        assert!(rect_visible(100, 100, 360, 420, &monitors));
        assert!(rect_visible(1700, 900, 360, 420, &monitors));
    }

    #[test]
    fn a_window_rect_off_every_monitor_is_not_visible() {
        let monitors = [(0, 0, 1920, 1080)];

        assert!(!rect_visible(9000, 9000, 360, 420, &monitors));
        assert!(!rect_visible(-5000, -5000, 360, 420, &monitors));
        // Barely peeking in is not enough to grab.
        assert!(!rect_visible(1910, 100, 360, 420, &monitors));
    }
}
