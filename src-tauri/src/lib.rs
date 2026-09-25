mod notes;
mod paths;
mod tray;

use tauri::RunEvent;

/// Start the desktop app.
///
/// Window lifecycle, note persistence, the tray and local paths all live in
/// Rust; the React layer only renders UI and calls commands.
pub fn run() {
    let app = match tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(notes::NoteRuntime::default())
        .invoke_handler(tauri::generate_handler![
            notes::new_note,
            notes::get_note,
            notes::save_note_content,
            notes::set_note_pinned,
            notes::confirm_exit_flush,
            notes::exit_app,
        ])
        .setup(|app| {
            let handle = app.handle().clone();

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

    app.run(|_app_handle, event| {
        if let RunEvent::ExitRequested { api, code, .. } = event {
            // `code` is `Some` only for programmatic exits, which is how the
            // tray "Exit" item quits. Deleting the last note reports `None`,
            // so it is vetoed to keep the tray and app process alive.
            if code.is_none() {
                api.prevent_exit();
            }
        }
    });
}
