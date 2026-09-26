use tauri::{
    menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
    AppHandle,
};
use tauri_plugin_autostart::ManagerExt;

use crate::harness_window;
use crate::notes;

const TRAY_ID: &str = "sticky-harness-tray";
const MENU_ID_NEW_NOTE: &str = "new-note";
const MENU_ID_HARNESS_TASKS: &str = "harness-tasks";
const MENU_ID_SHOW_ALL: &str = "show-all-notes";
const MENU_ID_HIDE_ALL: &str = "hide-all-notes";
const MENU_ID_AUTOSTART: &str = "start-with-windows";
const MENU_ID_EXIT: &str = "exit";

/// Read the real OS autostart state rather than trusting a cached flag.
///
/// The plugin owns the registration, so this is the single source of truth for
/// whether the checkbox should be ticked, including changes made outside the
/// app while it was closed.
pub fn autostart_enabled(app: &AppHandle) -> bool {
    match app.autolaunch().is_enabled() {
        Ok(enabled) => enabled,
        Err(error) => {
            eprintln!("[sticky-harness] could not read the autostart state: {error}");
            false
        }
    }
}

/// Turn OS autostart on or off through the official plugin.
///
/// Returns the state the OS actually reports afterwards, so the caller can put
/// the checkbox back to the truth when the change fails.
pub fn set_autostart(app: &AppHandle, enable: bool) -> bool {
    let manager = app.autolaunch();
    let result = if enable {
        manager.enable()
    } else {
        manager.disable()
    };

    if let Err(error) = result {
        eprintln!(
            "[sticky-harness] could not {} autostart: {error}",
            if enable { "enable" } else { "disable" }
        );
    }

    autostart_enabled(app)
}

/// Install the system tray icon and its menu.
///
/// The tray is the app's permanent entry point: hiding or deleting every note
/// must leave it alive, so the user can always create or reveal a note again.
pub fn init(app: &AppHandle) -> Result<(), String> {
    let new_note_item = MenuItem::with_id(app, MENU_ID_NEW_NOTE, "New Note", true, None::<&str>)
        .map_err(|error| format!("could not build the tray \"New Note\" item: {error}"))?;
    let harness_item =
        MenuItem::with_id(app, MENU_ID_HARNESS_TASKS, "Harness Tasks", true, None::<&str>)
            .map_err(|error| {
                format!("could not build the tray \"Harness Tasks\" item: {error}")
            })?;
    let show_item = MenuItem::with_id(app, MENU_ID_SHOW_ALL, "Show All Notes", true, None::<&str>)
        .map_err(|error| format!("could not build the tray \"Show All Notes\" item: {error}"))?;
    let hide_item = MenuItem::with_id(app, MENU_ID_HIDE_ALL, "Hide All Notes", true, None::<&str>)
        .map_err(|error| format!("could not build the tray \"Hide All Notes\" item: {error}"))?;
    let autostart_item = CheckMenuItem::with_id(
        app,
        MENU_ID_AUTOSTART,
        "Start with Windows",
        true,
        // Start from the real OS state so the tick is honest on the first open.
        autostart_enabled(app),
        None::<&str>,
    )
    .map_err(|error| format!("could not build the tray \"Start with Windows\" item: {error}"))?;
    let exit_item = MenuItem::with_id(app, MENU_ID_EXIT, "Exit", true, None::<&str>)
        .map_err(|error| format!("could not build the tray \"Exit\" item: {error}"))?;

    let first_separator = PredefinedMenuItem::separator(app)
        .map_err(|error| format!("could not build a tray separator: {error}"))?;
    let second_separator = PredefinedMenuItem::separator(app)
        .map_err(|error| format!("could not build a tray separator: {error}"))?;
    let third_separator = PredefinedMenuItem::separator(app)
        .map_err(|error| format!("could not build a tray separator: {error}"))?;

    let menu = Menu::with_items(
        app,
        &[
            &new_note_item,
            &harness_item,
            &first_separator,
            &show_item,
            &hide_item,
            &second_separator,
            &autostart_item,
            &third_separator,
            &exit_item,
        ],
    )
    .map_err(|error| format!("could not build the tray menu: {error}"))?;

    // The event handler needs its own handle to the checkbox so a failed
    // autostart change can be undone in the menu the user is looking at.
    let autostart_for_events = autostart_item.clone();

    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        .tooltip("Sticky Harness")
        .show_menu_on_left_click(true)
        .on_menu_event(move |app, event| match event.id.as_ref() {
            MENU_ID_NEW_NOTE => {
                // Same entry point as the in-window `+` button. Notes created
                // while others are hidden appear on their own; hiding is never
                // undone implicitly.
                if let Err(error) = notes::create_and_open(app) {
                    eprintln!("[sticky-harness] tray \"New Note\" failed: {error}");
                }
            }
            MENU_ID_HARNESS_TASKS => {
                // One window at most: if it exists this shows and focuses it,
                // and only the first click creates it.
                match harness_window::open(app) {
                    Ok(true) => println!("[sticky-harness] opened the harness tasks window"),
                    Ok(false) => println!("[sticky-harness] focused the harness tasks window"),
                    Err(error) => {
                        eprintln!("[sticky-harness] tray \"Harness Tasks\" failed: {error}")
                    }
                }
            }
            MENU_ID_SHOW_ALL => match notes::show_all_notes(app) {
                Ok(count) => println!("[sticky-harness] showed {count} note window(s)"),
                Err(error) => eprintln!("[sticky-harness] tray \"Show All Notes\" failed: {error}"),
            },
            MENU_ID_HIDE_ALL => match notes::hide_all_notes(app) {
                Ok(count) => println!("[sticky-harness] hid {count} note window(s)"),
                Err(error) => eprintln!("[sticky-harness] tray \"Hide All Notes\" failed: {error}"),
            },
            MENU_ID_AUTOSTART => {
                // The click already flipped the checkmark, so the requested
                // state is its negation. Whatever the OS reports afterwards is
                // what the checkbox is set back to.
                let requested = !autostart_enabled(app);
                let actual = set_autostart(app, requested);
                if let Err(error) = autostart_for_events.set_checked(actual) {
                    eprintln!("[sticky-harness] could not update the autostart checkbox: {error}");
                }
                println!("[sticky-harness] autostart requested={requested} actual={actual}");
            }
            MENU_ID_EXIT => {
                // Let every note flush its last edit before quitting, so text
                // typed less than one debounce ago is not lost. The command
                // sets the exiting flag itself, which stops the window closes
                // that follow from being mistaken for user deletes.
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    notes::exit_app(app).await;
                });
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
