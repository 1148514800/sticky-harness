mod harness;
mod harness_window;
mod notes;
mod paths;
mod tray;

use tauri::{Manager, RunEvent};

/// Start the desktop app.
///
/// Window lifecycle, note persistence, the tray and local paths all live in
/// Rust; the React layer only renders UI and calls commands.
pub fn run() {
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
