//! The Adapter Management window: one small window for configuring adapters.
//!
//! This is a utility surface, not a dashboard. It shows one row per configured
//! adapter - name, type, source, enabled, last status - and offers add, edit,
//! delete, enable/disable and reload. Everything it writes goes through
//! [`AdapterConfig`], so the window cannot invent a configuration the file
//! format would not accept.
//!
//! Two rules shape it:
//!
//! - **Rust validates, the window only asks.** Every mutation re-runs the same
//!   [`AdapterEntry::validate`] the startup path uses, against the whole
//!   resulting configuration. An invalid edit is refused with a message and the
//!   file on disk is left exactly as it was, so a bad edit can never make the
//!   next start worse than the last.
//! - **A bad adapter never breaks the others.** Applying a configuration starts
//!   the good adapters and skips the ones that cannot be built, exactly like
//!   startup. The window itself is never a prerequisite for notes or the tray.

use serde::Serialize;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent};

use super::manager::{
    load_checked_for, load_config_for, save_config, AdapterConfig, AdapterConfigError, AdapterEntry,
    AdapterKind, AdapterOutcome, RunState,
};
use super::HarnessState;

/// The window label. Fixed and explicit, like `harness-tasks`, so the frontend
/// can tell the three window kinds apart from the label alone.
pub const ADAPTERS_LABEL: &str = "harness-adapters";
/// The window title, which is also the heading inside the window.
pub const ADAPTERS_TITLE: &str = "Harness Adapters";
const DEFAULT_WIDTH: u32 = 460;
const DEFAULT_HEIGHT: u32 = 420;
/// Small enough to stay usable; the table needs a little more room than a note.
const MIN_WIDTH: f64 = 360.0;
const MIN_HEIGHT: f64 = 240.0;
/// The status a disabled adapter reports instead of a pending poll.
const OFF_LABEL: &str = "off";

/// One adapter as the window shows it.
///
/// A flat, display-only projection: the configuration plus the last health
/// reading. Deliberately not `AdapterEntry` itself, so the window cannot
/// round-trip a field it never showed.
#[derive(Debug, Clone, Serialize)]
pub struct AdapterView {
    pub name: String,
    pub kind: AdapterKind,
    pub enabled: bool,
    /// The short "where does this read from" string: a file path, or the
    /// loopback URL for an HTTP adapter. Never contains a remote host.
    pub source: String,
    /// `ok`, `error`, `rejected`, `waiting` or `off`.
    pub status: String,
    /// The failure text, when there is one worth showing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// Local clock reading of the last poll attempt, in Unix milliseconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checked_at: Option<u64>,
    /// Local clock reading of the last successful poll, in Unix milliseconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub succeeded_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub poll_interval_millis: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_millis: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

/// Everything the window renders in one poll.
#[derive(Debug, Clone, Serialize)]
pub struct AdaptersView {
    pub adapters: Vec<AdapterView>,
    /// How many adapters are actually running, for the footer.
    pub running: usize,
    /// Why the file on disk could not be used, when that is the case.
    ///
    /// A file that does not parse starts the app with no adapters, which is
    /// correct but would otherwise look identical to "no adapters configured".
    /// Saying so is the difference between a user fixing a typo and a user
    /// wondering where their adapters went.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub problem: Option<String>,
}

/// The status one row shows.
///
/// A disabled adapter is not waiting to poll - it is not there, so it reports
/// its own state rather than the generic pending one, which would promise a
/// poll that will never come.
pub(crate) fn status_label(enabled: bool, outcome: Option<&AdapterOutcome>) -> String {
    if !enabled {
        return OFF_LABEL.to_string();
    }
    outcome
        .map(|outcome| outcome.label().to_string())
        .unwrap_or_else(|| AdapterOutcome::Pending.label().to_string())
}

/// What is on screen: the configuration joined with the last health reading.
fn view(app: &AppHandle, state: &HarnessState) -> AdaptersView {
    // A bad file is reported rather than silently rendering as no adapters.
    let (config, problem) = match load_checked_for(app) {
        Ok(config) => (config, None),
        Err(error) => (AdapterConfig::default(), Some(error)),
    };
    let statuses = state.adapter_statuses();

    let adapters = config
        .adapters
        .iter()
        .map(|entry| {
            let status = statuses.iter().find(|status| status.name == entry.name);
            let outcome = status.map(|status| &status.outcome);

            AdapterView {
                name: entry.name.clone(),
                kind: entry.kind,
                enabled: entry.enabled,
                source: entry.source_label(),
                status: status_label(entry.enabled, outcome),
                detail: outcome.and_then(|outcome| outcome.detail().map(str::to_string)),
                checked_at: status.and_then(|status| status.checked_at),
                succeeded_at: status.and_then(|status| status.succeeded_at),
                poll_interval_millis: entry.poll_interval_millis,
                timeout_millis: entry.timeout_millis,
                http_path: entry.http_path.clone(),
                port: entry.port,
                path: entry.path.clone(),
            }
        })
        .collect();

    AdaptersView {
        adapters,
        running: state.adapter_run_state().adapters(),
        problem,
    }
}

/// The current window, if it is open.
fn window_of(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window(ADAPTERS_LABEL)
}

/// Show the existing window, or create it the first time.
///
/// Same "at most one" rule as the Harness Task Note, and the same reason: the
/// tray item, the startup restore and the tests all come through here.
pub fn open(app: &AppHandle) -> Result<bool, String> {
    if let Some(window) = window_of(app) {
        if !window.is_visible().unwrap_or(false) {
            window
                .show()
                .map_err(|error| format!("could not show the adapter window: {error}"))?;
        }
        let _ = window.set_focus();
        return Ok(false);
    }

    let window = WebviewWindowBuilder::new(app, ADAPTERS_LABEL, WebviewUrl::default())
        .title(ADAPTERS_TITLE)
        .inner_size(DEFAULT_WIDTH as f64, DEFAULT_HEIGHT as f64)
        .min_inner_size(MIN_WIDTH, MIN_HEIGHT)
        .resizable(true)
        .build()
        .map_err(|error| format!("could not create the adapter window: {error}"))?;

    register_window_events(&window);

    let _ = window.set_focus();
    Ok(true)
}

/// Closing hides.
///
/// This window is a view over a configuration file, so `X` must not throw away
/// the fact that the user opened it; the tray item brings it back. Nothing is
/// destroyed, and no adapter keeps running because of it.
fn register_window_events(window: &WebviewWindow) {
    let window_handle = window.clone();
    window.on_window_event(move |event| {
        if let WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
            if let Err(error) = window_handle.hide() {
                eprintln!("[sticky-harness] could not hide the adapter window: {error}");
            }
        }
    });
}

/// Show the window, if it exists, without creating it.
pub fn show_if_open(app: &AppHandle) -> bool {
    let Some(window) = window_of(app) else {
        return false;
    };
    let _ = window.show();
    true
}

/// Hide the window, if it exists, without destroying it.
pub fn hide_if_open(app: &AppHandle) -> bool {
    let Some(window) = window_of(app) else {
        return false;
    };
    if let Err(error) = window.hide() {
        eprintln!("[sticky-harness] could not hide the adapter window: {error}");
        return false;
    }
    true
}

/// Apply a configuration to the running adapters, keeping the good ones.
///
/// This is the one place a configuration reaches the manager, so add, edit,
/// delete, enable, disable and reload all behave identically and all keep the
/// "one bad adapter does not stop the others" rule.
fn apply(state: &HarnessState, config: &AdapterConfig) -> RunState {
    let run_state = state.apply_adapter_config(config);
    match run_state {
        RunState::Idle => println!("[sticky-harness] no harness adapters configured"),
        RunState::Running(count) => println!("[sticky-harness] {count} harness adapter(s) running"),
    }
    run_state
}

/// Write the configuration, then apply it.
///
/// Order matters: the file is written first and only then are the adapters
/// restarted, so a write failure leaves the running set untouched rather than
/// applying something that is not on disk.
fn commit(
    app: &AppHandle,
    state: &HarnessState,
    config: &AdapterConfig,
) -> Result<AdaptersView, AdapterConfigError> {
    config.validate()?;
    let path = super::manager::config_path(app)?;
    save_config(&path, config)?;
    apply(state, config);
    Ok(view(app, state))
}

/// Read the current configuration, or an empty one the window can add to.
fn current_config(app: &AppHandle) -> AdapterConfig {
    load_config_for(app)
}

/// Tray-facing: open the adapter management window.
#[tauri::command]
pub async fn open_adapters_window(app: AppHandle) -> Result<bool, String> {
    open(&app)
}

/// The adapters as the window should show them.
#[tauri::command]
pub async fn list_adapters(
    app: AppHandle,
    state: tauri::State<'_, HarnessState>,
) -> Result<AdaptersView, String> {
    Ok(view(&app, &state))
}

/// Add a new adapter, replace an existing one, or rename one.
///
/// `previous_name` is how an edit that changes the name stays an edit. Without
/// it, renaming would look like an add and leave the old adapter behind, which
/// is the one way this window could silently double a configuration.
#[tauri::command]
pub async fn save_adapter(
    app: AppHandle,
    state: tauri::State<'_, HarnessState>,
    entry: AdapterEntry,
    previous_name: Option<String>,
) -> Result<AdaptersView, String> {
    let mut config = current_config(&app);

    match previous_name.as_deref() {
        // An edit: rename in place, so the adapter keeps its position and the
        // old name does not survive as a second, identical adapter.
        Some(previous) => {
            if previous != entry.name {
                if !config.remove(previous) {
                    return Err(format!("no adapter is named {previous:?}"));
                }
            }
        }
        // An add: a name that is already taken is a mistake the user wants to
        // hear about, not a silent overwrite of an adapter they cannot see
        // from the add form.
        None => {
            if config.adapters.iter().any(|existing| existing.name == entry.name) {
                return Err(format!(
                    "an adapter is already named {:?}; edit it instead, or choose another name",
                    entry.name
                ));
            }
        }
    }

    config.put(entry);
    commit(&app, &state, &config)
}

/// Turn one adapter on or off without touching anything else about it.
#[tauri::command]
pub async fn set_adapter_enabled(
    app: AppHandle,
    state: tauri::State<'_, HarnessState>,
    name: String,
    enabled: bool,
) -> Result<AdaptersView, String> {
    let mut config = current_config(&app);
    let index = config
        .adapters
        .iter()
        .position(|entry| entry.name == name)
        .ok_or_else(|| format!("no adapter is named {name:?}"))?;
    config.adapters[index].enabled = enabled;
    commit(&app, &state, &config)
}

/// Remove one adapter.
#[tauri::command]
pub async fn delete_adapter(
    app: AppHandle,
    state: tauri::State<'_, HarnessState>,
    name: String,
) -> Result<AdaptersView, String> {
    let mut config = current_config(&app);
    if !config.remove(&name) {
        return Err(format!("no adapter is named {name:?}"));
    }
    commit(&app, &state, &config)
}

/// Re-read the configuration file and start from it.
///
/// The escape hatch for hand-editing: the file is the contract, so reloading it
/// must not require a restart.
#[tauri::command]
pub async fn reload_adapters(
    app: AppHandle,
    state: tauri::State<'_, HarnessState>,
) -> Result<AdaptersView, String> {
    // Read the file directly here rather than through the lenient loader: a
    // hand-edited file that does not parse should say so instead of silently
    // emptying the list and looking like "no adapters configured".
    let config = load_checked_for(&app)?;
    apply(&state, &config);
    Ok(view(&app, &state))
}
