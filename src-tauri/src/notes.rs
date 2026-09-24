//! Note persistence and note-window lifecycle.
//!
//! This module is the single owner of "what a note is" and "how a note window
//! is created". Startup restore, the in-window `+` button and the tray
//! `New Note` item all funnel through [`create_and_open`], so there is exactly
//! one code path for producing a note window.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{
    AppHandle, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder, WindowEvent,
};

use crate::paths;

/// Window labels look like `note-<id>`; the id is also the JSON file stem.
pub const NOTE_LABEL_PREFIX: &str = "note-";

const WINDOW_TITLE: &str = "便签";
/// Minimum window size in logical pixels, so it stays usable at any DPI.
const MIN_WIDTH: f64 = 240.0;
const MIN_HEIGHT: f64 = 160.0;
const DEFAULT_WIDTH: u32 = 330;
const DEFAULT_HEIGHT: u32 = 300;
/// Offset of the first cascaded (never-positioned) window from the monitor origin.
const CASCADE_START: i32 = 60;
const CASCADE_STEP: i32 = 32;
const CASCADE_WRAP: i32 = 10;
/// Quiet period after the last move/resize before window geometry is written.
const WINDOW_STATE_DEBOUNCE: Duration = Duration::from_millis(400);
/// A restored window must be at least this visible on some monitor.
const MIN_VISIBLE_WIDTH: f64 = 80.0;
const MIN_VISIBLE_HEIGHT: f64 = 40.0;

/// Persisted geometry for one note window, in physical pixels.
///
/// Every field is optional so a hand-written or partially corrupt record still
/// loads: missing geometry falls back to a safe cascaded position.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WindowState {
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    #[serde(default)]
    pub always_on_top: bool,
}

/// One sticky note: identity, text and window geometry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoteRecord {
    /// Stable for the lifetime of the note; never regenerated on restart.
    pub id: String,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub created_at: u64,
    #[serde(default)]
    pub updated_at: u64,
    #[serde(default)]
    pub window: WindowState,
}

/// Process-wide state: the exit flag, id sequence and move/resize debouncing.
#[derive(Default)]
pub struct NoteRuntime {
    exiting: AtomicBool,
    next_sequence: AtomicU64,
    /// Latest debounce generation per note id; a stale generation is dropped.
    generations: Mutex<HashMap<String, u64>>,
}

impl NoteRuntime {
    /// Mark the process as shutting down.
    ///
    /// While set, closing a window must not delete its note: the tray `Exit`
    /// item closes every window, and that must never look like a user delete.
    pub fn begin_exit(&self) {
        self.exiting.store(true, Ordering::SeqCst);
    }

    fn is_exiting(&self) -> bool {
        self.exiting.load(Ordering::SeqCst)
    }

    fn next_sequence(&self) -> u64 {
        self.next_sequence.fetch_add(1, Ordering::Relaxed)
    }

    /// Register a new save attempt for `note_id` and return its generation.
    fn bump_generation(&self, note_id: &str) -> u64 {
        let mut generations = self.generations.lock().unwrap_or_else(|error| error.into_inner());
        let entry = generations.entry(note_id.to_string()).or_insert(0);
        *entry += 1;
        *entry
    }

    fn generation(&self, note_id: &str) -> Option<u64> {
        let generations = self.generations.lock().unwrap_or_else(|error| error.into_inner());
        generations.get(note_id).copied()
    }
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

/// Note ids become file names, so only accept a conservative character set.
fn is_valid_note_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn label_for(note_id: &str) -> String {
    format!("{NOTE_LABEL_PREFIX}{note_id}")
}

fn note_file(app: &AppHandle, note_id: &str) -> Result<PathBuf, String> {
    if !is_valid_note_id(note_id) {
        return Err(format!("refusing to use the invalid note id \"{note_id}\""));
    }
    Ok(paths::notes_dir(app)?.join(format!("{note_id}.json")))
}

/// Write a record by writing a sibling temp file and renaming it into place, so
/// an interrupted write can never leave a half-written JSON file behind.
fn write_record(path: &Path, record: &NoteRecord) -> Result<(), String> {
    let json = serde_json::to_string_pretty(record)
        .map_err(|error| format!("could not serialise note \"{}\": {error}", record.id))?;

    let temp = path.with_extension("json.tmp");
    fs::write(&temp, json)
        .map_err(|error| format!("could not write {}: {error}", temp.display()))?;

    // The rename is the atomic path and what normally runs. Some Windows
    // setups (filesystem filter drivers on the AppData tree) refuse the
    // rename with `ERROR_NOT_SAME_DEVICE` even though both files sit in the
    // same directory, so fall back to copying over the destination instead of
    // losing the save entirely.
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

fn save_record(app: &AppHandle, record: &NoteRecord) -> Result<(), String> {
    write_record(&note_file(app, &record.id)?, record)
}

/// Load one note from disk.
fn load_record(app: &AppHandle, note_id: &str) -> Result<NoteRecord, String> {
    let path = note_file(app, note_id)?;
    let raw = fs::read_to_string(&path)
        .map_err(|error| format!("could not read {}: {error}", path.display()))?;
    let mut record: NoteRecord = serde_json::from_str(&raw)
        .map_err(|error| format!("could not parse {}: {error}", path.display()))?;

    // The file name is the authoritative id, because that is the path every
    // save is written back to. A file whose inner id disagrees would otherwise
    // be saved under that stray id and silently fork the note in two.
    record.id = note_id.to_string();

    Ok(record)
}

/// Load every readable note, skipping (and reporting) individual bad files so
/// one corrupt note cannot stop the others from restoring.
fn load_all_records(app: &AppHandle) -> Result<Vec<NoteRecord>, String> {
    let dir = paths::notes_dir(app)?;
    let entries = fs::read_dir(&dir)
        .map_err(|error| format!("could not list {}: {error}", dir.display()))?;

    let mut records = Vec::new();
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                eprintln!("[sticky-harness] could not read a notes directory entry: {error}");
                continue;
            }
        };

        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }

        // The file stem *is* the note id, so it wins over whatever the JSON
        // claims. That keeps `id`, the file name and the window label in sync
        // even if a stray file was hand-edited or copied in.
        let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
            eprintln!(
                "[sticky-harness] skipping note with an unreadable file name: {}",
                path.display()
            );
            continue;
        };

        if !is_valid_note_id(stem) {
            eprintln!(
                "[sticky-harness] skipping note with an invalid id in its file name: {}",
                path.display()
            );
            continue;
        }

        let raw = match fs::read_to_string(&path) {
            Ok(raw) => raw,
            Err(error) => {
                eprintln!(
                    "[sticky-harness] skipping unreadable note {}: {error}",
                    path.display()
                );
                continue;
            }
        };

        match serde_json::from_str::<NoteRecord>(&raw) {
            Ok(mut record) => {
                if record.id != stem {
                    eprintln!(
                        "[sticky-harness] note {} claims id \"{}\"; using the file name instead",
                        path.display(),
                        record.id
                    );
                    record.id = stem.to_string();
                }
                records.push(record);
            }
            Err(error) => {
                eprintln!(
                    "[sticky-harness] skipping corrupt note {}: {error}",
                    path.display()
                );
            }
        }
    }

    // Stable ordering keeps cascaded fallback positions predictable.
    records.sort_by(|a, b| a.created_at.cmp(&b.created_at).then_with(|| a.id.cmp(&b.id)));
    Ok(records)
}

/// A note with no usable saved geometry is cascaded near a real monitor.
fn cascade_position(app: &AppHandle, index: i32) -> PhysicalPosition<i32> {
    let origin = app
        .primary_monitor()
        .ok()
        .flatten()
        .map(|monitor| *monitor.position())
        .unwrap_or(PhysicalPosition::new(0, 0));

    let step = CASCADE_STEP * (index % CASCADE_WRAP);
    PhysicalPosition::new(origin.x + CASCADE_START + step, origin.y + CASCADE_START + step)
}

fn rect_visible(x: i32, y: i32, width: u32, height: u32, monitors: &[(i32, i32, u32, u32)]) -> bool {
    monitors.iter().any(|(mx, my, mw, mh)| {
        let overlap_x =
            (x + width as i32).min(mx + *mw as i32) - x.max(*mx);
        let overlap_y =
            (y + height as i32).min(my + *mh as i32) - y.max(*my);
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

/// Apply saved geometry, keeping the window on a monitor that still exists.
fn apply_window_state(app: &AppHandle, window: &WebviewWindow, state: &WindowState, cascade_index: i32) {
    let width = state.width.unwrap_or(DEFAULT_WIDTH).max(MIN_WIDTH as u32);
    let height = state.height.unwrap_or(DEFAULT_HEIGHT).max(MIN_HEIGHT as u32);

    if let Err(error) = window.set_size(PhysicalSize::new(width, height)) {
        eprintln!("[sticky-harness] could not restore the window size: {error}");
    }

    let monitors = monitor_bounds(app);
    let saved = match (state.x, state.y) {
        (Some(x), Some(y)) if rect_visible(x, y, width, height, &monitors) => Some((x, y)),
        (Some(_), Some(_)) => {
            // The monitor this note lived on is gone; fall back instead of
            // restoring the window somewhere the user cannot reach it.
            eprintln!("[sticky-harness] saved position is off-screen; using a fallback position");
            None
        }
        _ => None,
    };

    let position = match saved {
        Some((x, y)) => PhysicalPosition::new(x, y),
        None => cascade_position(app, cascade_index),
    };

    if let Err(error) = window.set_position(position) {
        eprintln!("[sticky-harness] could not restore the window position: {error}");
    }
}

/// Read the window's current geometry back and persist it.
fn persist_window_state(app: &AppHandle, note_id: &str) -> Result<(), String> {
    let Some(window) = app.get_webview_window(&label_for(note_id)) else {
        return Ok(());
    };

    let position = window
        .outer_position()
        .map_err(|error| format!("could not read the window position: {error}"))?;
    let size = window
        .inner_size()
        .map_err(|error| format!("could not read the window size: {error}"))?;

    let mut record = load_record(app, note_id)?;
    record.window.x = Some(position.x);
    record.window.y = Some(position.y);
    record.window.width = Some(size.width);
    record.window.height = Some(size.height);
    save_record(app, &record)
}

/// Coalesce a burst of move/resize events into one write.
fn schedule_window_state_save(app: &AppHandle, note_id: &str) {
    let generation = app
        .state::<NoteRuntime>()
        .bump_generation(note_id);

    let app = app.clone();
    let note_id = note_id.to_string();

    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(WINDOW_STATE_DEBOUNCE).await;

        // A newer event arrived while we waited, so let that one do the write.
        if app.state::<NoteRuntime>().generation(&note_id) != Some(generation) {
            return;
        }

        if let Err(error) = persist_window_state(&app, &note_id) {
            eprintln!("[sticky-harness] could not save window state for \"{note_id}\": {error}");
        }
    });
}

/// Build, position and show the window for an existing note record.
fn open_note_window(app: &AppHandle, record: &NoteRecord, cascade_index: i32) -> Result<(), String> {
    let label = label_for(&record.id);

    let window = WebviewWindowBuilder::new(app, &label, WebviewUrl::default())
        .title(WINDOW_TITLE)
        .inner_size(DEFAULT_WIDTH as f64, DEFAULT_HEIGHT as f64)
        .min_inner_size(MIN_WIDTH, MIN_HEIGHT)
        .resizable(true)
        .always_on_top(record.window.always_on_top)
        // Stay hidden until the saved geometry is applied, so the window never
        // flashes at the default position first.
        .visible(false)
        .build()
        .map_err(|error| format!("could not create the window for note \"{}\": {error}", record.id))?;

    apply_window_state(app, &window, &record.window, cascade_index);
    register_window_events(app, &window, &record.id);

    if let Err(error) = window.show() {
        eprintln!("[sticky-harness] could not show note \"{}\": {error}", record.id);
    }
    if let Err(error) = window.set_focus() {
        eprintln!("[sticky-harness] could not focus note \"{}\": {error}", record.id);
    }

    Ok(())
}

/// Wire up the two window behaviours a note has: it deletes itself when the
/// user closes it, and it remembers its geometry.
fn register_window_events(app: &AppHandle, window: &WebviewWindow, note_id: &str) {
    let app = app.clone();
    let note_id = note_id.to_string();

    window.on_window_event(move |event| match event {
        WindowEvent::CloseRequested { .. } => {
            // Closing a note means deleting it, except while the whole app is
            // quitting - then the data must survive.
            if app.state::<NoteRuntime>().is_exiting() {
                println!("[sticky-harness] exiting: keeping note \"{note_id}\"");
                return;
            }

            if let Err(error) = delete_note(&app, &note_id) {
                eprintln!("[sticky-harness] could not delete note \"{note_id}\": {error}");
            }
        }
        WindowEvent::Moved(_) | WindowEvent::Resized(_) => {
            schedule_window_state_save(&app, &note_id);
        }
        _ => {}
    });
}

/// Delete a note's record file. Removing a missing file is not an error.
fn delete_note(app: &AppHandle, note_id: &str) -> Result<(), String> {
    let path = note_file(app, note_id)?;
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("could not delete {}: {error}", path.display())),
    }
}

fn next_note_id(app: &AppHandle) -> String {
    let sequence = app.state::<NoteRuntime>().next_sequence();
    format!("{:x}-{:x}", now_millis(), sequence)
}

/// Create the record for a brand new empty note and persist it.
fn build_new_record(app: &AppHandle) -> Result<NoteRecord, String> {
    let timestamp = now_millis();
    let record = NoteRecord {
        id: next_note_id(app),
        content: String::new(),
        created_at: timestamp,
        updated_at: timestamp,
        window: WindowState::default(),
    };

    save_record(app, &record)?;
    Ok(record)
}

/// The one entry point for making a new note window.
///
/// `+` in a note, the tray `New Note` item and the empty-startup case all call
/// this, so there is a single place that knows how a note comes into being.
pub fn create_and_open(app: &AppHandle) -> Result<NoteRecord, String> {
    let record = build_new_record(app)?;
    let cascade_index = app.webview_windows().len() as i32;
    open_note_window(app, &record, cascade_index)?;
    println!("[sticky-harness] created note \"{}\"", record.id);
    Ok(record)
}

/// Mark the process as shutting down, so window closes stop meaning "delete".
///
/// The tray `Exit` item calls this immediately before `app.exit`.
pub fn begin_exit(app: &AppHandle) {
    app.state::<NoteRuntime>().begin_exit();
}

/// Open every stored note, creating one blank note when there is nothing to
/// restore. Returns how many windows were opened.
pub fn restore(app: &AppHandle) -> Result<usize, String> {
    let records = load_all_records(app)?;

    if records.is_empty() {
        println!("[sticky-harness] no saved notes found; creating a blank note");
        create_and_open(app)?;
        return Ok(1);
    }

    let mut opened = 0;
    for (index, record) in records.iter().enumerate() {
        match open_note_window(app, record, index as i32) {
            Ok(()) => opened += 1,
            Err(error) => eprintln!(
                "[sticky-harness] could not restore note \"{}\": {error}",
                record.id
            ),
        }
    }

    Ok(opened)
}

/// Create a new note window (the `+` button and the tray `New Note` item).
///
/// Kept `async` on purpose, like every command that creates a window: window
/// creation is carried out by the very event loop that is delivering the
/// invoke, so a synchronous version deadlocks.
#[tauri::command]
pub async fn new_note(app: AppHandle) -> Result<NoteRecord, String> {
    create_and_open(&app)
}

/// Read one note back, used by a note window while it is loading.
#[tauri::command]
pub async fn get_note(app: AppHandle, id: String) -> Result<NoteRecord, String> {
    load_record(&app, &id)
}

/// Persist edited text and refresh `updated_at`.
#[tauri::command]
pub async fn save_note_content(app: AppHandle, id: String, content: String) -> Result<(), String> {
    let mut record = load_record(&app, &id)?;

    if record.content == content {
        return Ok(());
    }

    record.content = content;
    record.updated_at = now_millis();
    save_record(&app, &record)
}

/// Toggle always-on-top for one note, in the OS and on disk.
#[tauri::command]
pub async fn set_note_pinned(app: AppHandle, id: String, pinned: bool) -> Result<(), String> {
    let mut record = load_record(&app, &id)?;

    if let Some(window) = app.get_webview_window(&label_for(&id)) {
        window
            .set_always_on_top(pinned)
            .map_err(|error| format!("could not change always-on-top: {error}"))?;
    }

    record.window.always_on_top = pinned;
    record.updated_at = now_millis();
    save_record(&app, &record)
}
