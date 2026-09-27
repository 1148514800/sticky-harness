mod harness;
mod harness_window;
mod notes;
mod paths;
mod tray;

use std::path::{Path, PathBuf};

use tauri::{Manager, RunEvent};

/// Report a panic somewhere a release user can actually find it.
///
/// `release` builds compile with `panic = "abort"`, so a panic in this app is
/// not a printed message followed by a stack unwinding past the Rust boundary -
/// it is an immediate process death, and the default hook's message goes to a
/// stderr that a windowed Windows app does not have. That turns any internal
/// panic into a silent disappearance from the user's point of view.
///
/// This hook keeps the crash a crash (abort is deliberate: unwinding through
/// the FFI boundary to Tauri/WebView2 is undefined behaviour, so the profile is
/// not changed here) but writes the panic's location and message to a file
/// before the process dies, so a report can say what happened instead of
/// guessing. The file is small, overwritten on each panic, and lives beside the
/// app's own data rather than in the source tree.
fn install_panic_logger() {
    std::panic::set_hook(Box::new(|info| {
        let location = info
            .location()
            .map(|location| {
                format!("{}:{}:{}", location.file(), location.line(), location.column())
            })
            .unwrap_or_else(|| "unknown location".to_string());

        let message = if let Some(text) = info.payload().downcast_ref::<&str>() {
            (*text).to_string()
        } else if let Some(text) = info.payload().downcast_ref::<String>() {
            text.clone()
        } else {
            "a panic with no text payload".to_string()
        };

        // Still print it, because a developer running from a terminal sees it.
        eprintln!("[sticky-harness] panic at {location}: {message}");

        if let Some(root) = app_data_root() {
            write_panic_log(&root, now_millis(), &location, &message);
        }
    }));
}

/// Where the panic log goes: the same data directory the rest of the app uses,
/// resolved through `paths` so the name still has one owner. `None` when the
/// variable is absent, in which case the panic is still printed and nothing is
/// written. The hook cannot use the Tauri resolver because it has no app handle
/// and may run while the process is already dying.
fn app_data_root() -> Option<PathBuf> {
    let base = std::env::var_os("APPDATA")?;
    Some(PathBuf::from(base).join(paths::app_data_dir_name()))
}

/// Write one line describing a panic. Returns the path it wrote, if it could.
///
/// Failures are deliberately swallowed: a panic is already the worst moment for
/// the process, and a diagnostic that itself panics would hide the original.
fn write_panic_log(root: &Path, stamp: u64, location: &str, message: &str) -> Option<PathBuf> {
    std::fs::create_dir_all(root).ok()?;
    let path = root.join("panic.log");
    std::fs::write(&path, format!("{stamp}\t{location}\t{message}\n")).ok()?;
    Some(path)
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

/// Start the desktop app.
///
/// Window lifecycle, note persistence, the tray and local paths all live in
/// Rust; the React layer only renders UI and calls commands.
pub fn run() {
    install_panic_logger();

    let app = match tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        // OS launch-at-startup is owned entirely by the official plugin:
        // no registry edits and no startup-folder shortcuts of our own.
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .manage(notes::NoteRuntime::default())
        .invoke_handler(tauri::generate_handler![
            notes::new_note,
            notes::get_note,
            notes::save_note_content,
            notes::set_note_pinned,
            notes::confirm_exit_flush,
            notes::exit_app,
            notes::show_notes,
            notes::hide_notes,
            harness::list_active_harness_tasks,
            harness::list_live_active_harness_tasks,
            harness::list_harness_snapshots,
            harness_window::harness_window_status,
            harness_window::set_harness_window_pinned,
            harness::harness_api_port,
            harness::harness_is_stale,
            harness::adapters_window::open_adapters_window,
            harness::adapters_window::list_adapters,
            harness::adapters_window::save_adapter,
            harness::adapters_window::set_adapter_enabled,
            harness::adapters_window::delete_adapter,
            harness::adapters_window::reload_adapters,
        ])
        .setup(|app| {
            let handle = app.handle().clone();

            // The harness registry and its local push endpoint. This starts the
            // registry even when the port is taken, because the app must keep
            // working; the failure is logged, not fatal.
            app.manage(harness::init());

            // Adapters need the app data directory, so they start after the
            // handle exists. A configuration problem leaves the push endpoint
            // working and the adapters empty, never the app broken.
            harness::start_adapters(
                &handle,
                &app.state::<harness::HarnessState>(),
            );

            // Resolving the app data dir here gives an early, loggable
            // confirmation of where user notes live.
            match paths::ensure_app_data_dir(&handle) {
                Ok(dir) => println!("[sticky-harness] app data dir: {}", dir.display()),
                Err(error) => eprintln!("[sticky-harness] {error}"),
            }

            if let Err(error) = tray::init(&handle) {
                // A missing tray must not stop the app from starting.
                eprintln!("[sticky-harness] tray initialisation failed: {error}");
            }

            // `setup` runs on the event loop thread, so restoring notes (which
            // builds windows) is handed to the async runtime instead of
            // blocking the loop that has to deliver the window events.
            tauri::async_runtime::spawn(async move {
                match notes::restore(&handle) {
                    Ok(count) => println!("[sticky-harness] restored {count} note window(s)"),
                    Err(error) => eprintln!("[sticky-harness] could not restore notes: {error}"),
                }

                // The Harness Task Note restores separately: it is not a note,
                // and it only comes back if the user ever opened it.
                match harness_window::restore(&handle) {
                    Ok(true) => println!("[sticky-harness] restored the harness tasks window"),
                    Ok(false) => {}
                    Err(error) => eprintln!(
                        "[sticky-harness] could not restore the harness tasks window: {error}"
                    ),
                }
            });

            Ok(())
        })
        .build(tauri::generate_context!())
    {
        Ok(app) => app,
        Err(error) => {
            eprintln!("[sticky-harness] failed to start the application: {error}");
            std::process::exit(1);
        }
    };

    app.run(|app_handle, event| {
        if let RunEvent::ExitRequested { api, code, .. } = event {
            // `code` is `Some` only for programmatic exits, which is how the
            // tray "Exit" item quits. Deleting the last note reports `None`,
            // so it is vetoed to keep the tray and app process alive.
            if code.is_none() {
                api.prevent_exit();
                return;
            }

            // Adapters are background producers, so they are told to stop here
            // rather than being waited on: a hung producer must never delay
            // Tray Exit.
            app_handle.state::<harness::HarnessState>().stop_adapters();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The panic log is the only thing a release user can report, so it has to
    /// actually land where the app can find it again.
    #[test]
    fn the_panic_log_records_the_location_and_message() {
        let dir = std::env::temp_dir().join(format!("sticky-harness-panic-test-{}", now_millis()));
        let path = write_panic_log(&dir, 1234, "src-tauri/src/lib.rs:1:1", "something broke")
            .expect("the panic log should be written");

        let written = std::fs::read_to_string(&path).expect("the panic log should be readable");
        assert_eq!(written, "1234\tsrc-tauri/src/lib.rs:1:1\tsomething broke\n");
        assert_eq!(path, dir.join("panic.log"));

        std::fs::remove_dir_all(&dir).ok();
    }

    /// A directory that cannot be created must not turn a panic into a second
    /// panic; the diagnostic is best-effort by design.
    #[test]
    fn an_unwritable_root_is_not_fatal() {
        let file = std::env::temp_dir().join(format!("sticky-harness-panic-file-{}", now_millis()));
        std::fs::write(&file, "not a directory").expect("the fixture file should be writable");

        // A path whose parent is a regular file cannot be a directory.
        let impossible = file.join("nested");
        assert!(write_panic_log(&impossible, 1, "here", "boom").is_none());

        std::fs::remove_file(&file).ok();
    }
}
