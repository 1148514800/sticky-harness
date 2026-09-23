use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{TrayIconBuilder, TrayIconEvent},
    AppHandle,
};

use crate::windows::show_or_create_main_window;

const TRAY_ID: &str = "sticky-harness-tray";
const MENU_ID_OPEN_WINDOW: &str = "open-window";
const MENU_ID_EXIT: &str = "exit";

/// Install the system tray icon with its `Open Window` / `Exit` menu.
///
/// The tray is the app's permanent entry point: closing every window must
/// leave it alive so the user can bring a window back.
pub fn init(app: &AppHandle) -> Result<(), String> {
    let open_item = MenuItem::with_id(app, MENU_ID_OPEN_WINDOW, "Open Window", true, None::<&str>)
        .map_err(|error| format!("could not build the tray \"Open Window\" item: {error}"))?;
    let separator = PredefinedMenuItem::separator(app)
        .map_err(|error| format!("could not build the tray separator: {error}"))?;
    let exit_item = MenuItem::with_id(app, MENU_ID_EXIT, "Exit", true, None::<&str>)
        .map_err(|error| format!("could not build the tray \"Exit\" item: {error}"))?;

    let menu = Menu::with_items(app, &[&open_item, &separator, &exit_item])
        .map_err(|error| format!("could not build the tray menu: {error}"))?;

    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        .tooltip("Sticky Harness")
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id.as_ref() {
            MENU_ID_OPEN_WINDOW => {
                if let Err(error) = show_or_create_main_window(app) {
                    eprintln!("[sticky-harness] tray \"Open Window\" failed: {error}");
                }
            }
            MENU_ID_EXIT => {
                // `exit` requests a shutdown with an explicit code, which the
                // run loop allows through (unlike a plain last-window close).
                app.exit(0);
            }
            other => {
                eprintln!("[sticky-harness] unhandled tray menu id: {other}");
            }
        })
        .on_tray_icon_event(|tray, event| {
            // Left-clicking the tray icon focuses or recreates the main window.
            if let TrayIconEvent::DoubleClick { .. } = event {
                if let Err(error) = show_or_create_main_window(tray.app_handle()) {
                    eprintln!("[sticky-harness] tray double click failed: {error}");
                }
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
