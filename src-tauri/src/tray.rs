use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
    AppHandle,
};

use crate::notes;

const TRAY_ID: &str = "sticky-harness-tray";
const MENU_ID_NEW_NOTE: &str = "new-note";
const MENU_ID_EXIT: &str = "exit";

/// Install the system tray icon with its `New Note` / `Exit` menu.
///
/// The tray is the app's permanent entry point: deleting every note must leave
/// the tray alive so the user can create a new one.
pub fn init(app: &AppHandle) -> Result<(), String> {
    let new_note_item = MenuItem::with_id(app, MENU_ID_NEW_NOTE, "New Note", true, None::<&str>)
        .map_err(|error| format!("could not build the tray \"New Note\" item: {error}"))?;
    let separator = PredefinedMenuItem::separator(app)
        .map_err(|error| format!("could not build the tray separator: {error}"))?;
    let exit_item = MenuItem::with_id(app, MENU_ID_EXIT, "Exit", true, None::<&str>)
        .map_err(|error| format!("could not build the tray \"Exit\" item: {error}"))?;

    let menu = Menu::with_items(app, &[&new_note_item, &separator, &exit_item])
        .map_err(|error| format!("could not build the tray menu: {error}"))?;

    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        .tooltip("Sticky Harness")
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id.as_ref() {
            MENU_ID_NEW_NOTE => {
                // Same entry point as the in-window `+` button.
                if let Err(error) = notes::create_and_open(app) {
                    eprintln!("[sticky-harness] tray \"New Note\" failed: {error}");
                }
            }
            MENU_ID_EXIT => {
                // Mark the exit first: `app.exit` closes every window, and
                // those closes must not be mistaken for user deletes.
                notes::begin_exit(app);

                // An explicit exit code passes the run loop, unlike a plain
                // last-window close, which `lib.rs` vetoes.
                app.exit(0);
            }
            other => {
                eprintln!("[sticky-harness] unhandled tray menu id: {other}");
            }
        });

    if let Some(icon) = app.default_window_icon().cloned() {
        builder = builder.icon(icon);
    } else {
        eprintln!("[sticky-harness] no bundled window icon found; tray icon uses the default.");
    }

    builder
        .build(app)
        .map_err(|error| format!("could not create the tray icon: {error}"))?;

    Ok(())
}
