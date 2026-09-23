mod paths;
mod tray;
mod windows;

use tauri::RunEvent;

/// Start the desktop app.
///
/// Window lifecycle, the tray and local paths all live in Rust; the React
/// layer only renders UI and calls commands.
pub fn run() {
    let app = match tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            windows::create_test_window,
            paths::app_data_dir
        ])
        .setup(|app| {
            let handle = app.handle().clone();

            // Resolving the app data dir here gives an early, loggable
            // confirmation of where user data will live once Phase 1 lands.
            match paths::ensure_app_data_dir(&handle) {
                Ok(dir) => println!("[sticky-harness] app data dir: {}", dir.display()),
                Err(error) => eprintln!("[sticky-harness] {error}"),
            }

            if let Err(error) = tray::init(&handle) {
                // A missing tray must not stop the app from starting.
                eprintln!("[sticky-harness] tray initialisation failed: {error}");
            }

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
            // tray "Exit" item quits. Closing the last window reports `None`,
            // so it is vetoed to keep the tray and app process alive.
            if code.is_none() {
                api.prevent_exit();
            }
        }
    });
}
