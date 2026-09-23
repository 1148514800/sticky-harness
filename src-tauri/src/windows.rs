use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

/// Label of the main test window.
pub const MAIN_WINDOW_LABEL: &str = "main";
/// Prefix used for every dynamically created test window.
const TEST_WINDOW_PREFIX: &str = "test-note-";

/// Minimum size every sticky window must keep.
const MIN_WIDTH: f64 = 240.0;
const MIN_HEIGHT: f64 = 160.0;
/// Default size for dynamically created test windows.
const DEFAULT_WIDTH: f64 = 320.0;
const DEFAULT_HEIGHT: f64 = 260.0;

/// Build a note window that loads the React app for the supplied label.
fn build_window(app: &AppHandle, label: &str, title: &str) -> Result<(), String> {
    let window = WebviewWindowBuilder::new(app, label, WebviewUrl::default())
        .title(title)
        .inner_size(DEFAULT_WIDTH, DEFAULT_HEIGHT)
        .min_inner_size(MIN_WIDTH, MIN_HEIGHT)
        .resizable(true)
        .center()
        .build()
        .map_err(|error| format!("could not create window \"{label}\": {error}"))?;

    // A failure to focus is not fatal: the window already exists.
    if let Err(error) = window.set_focus() {
        eprintln!("[sticky-harness] could not focus window \"{label}\": {error}");
    }

    Ok(())
}

/// Pick the next free `test-note-N` label so repeatedly created windows never
/// collide, even after earlier ones were closed.
fn next_test_window_label(app: &AppHandle) -> String {
    let existing = app.webview_windows();
    let mut index = existing.len() + 1;

    loop {
        let candidate = format!("{TEST_WINDOW_PREFIX}{index}");
        if !existing.contains_key(&candidate) {
            return candidate;
        }
        index += 1;
    }
}

/// Create a new, independent test window and return its label.
///
/// Phase 0 only proves that Tauri can host several unrelated windows at once.
/// Phase 1 replaces this with real note windows.
///
/// Keep this command `async`. Window creation is carried out by the event loop
/// that is already busy delivering this invoke, so a synchronous version
/// deadlocks: the window is built but the promise never resolves. Returning a
/// future moves the work off that in-progress message.
#[tauri::command]
pub async fn create_test_window(app: AppHandle) -> Result<String, String> {
    let label = next_test_window_label(&app);
    build_window(&app, &label, "便签")?;

    Ok(label)
}

/// Show and focus the main window, creating it again if it was closed.
///
/// Used by the tray menu so closing the last window never leaves the app
/// unreachable.
pub fn show_or_create_main_window(app: &AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        if let Err(error) = window.unminimize() {
            eprintln!("[sticky-harness] could not unminimize the main window: {error}");
        }
        window
            .show()
            .map_err(|error| format!("could not show the main window: {error}"))?;
        window
            .set_focus()
            .map_err(|error| format!("could not focus the main window: {error}"))?;

        return Ok(());
    }

    build_window(app, MAIN_WINDOW_LABEL, "Sticky Harness")
}
