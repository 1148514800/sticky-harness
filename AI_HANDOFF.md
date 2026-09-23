# AI Handoff

This is the single source of truth for picking this project up cold. Read it
first, then check the code and `git log`; if this file disagrees with the code,
trust the code and fix this file.

## 1. Project Goal

A Windows-first desktop app for minimal floating sticky notes, plus a separate
note that shows which local AI harness tasks are currently running. Everything
is local: no account, no sync, no server.

## 2. Product Decisions

- Windows first; macOS/Linux later, so avoid Windows-only architecture.
- Tauri 2 + React + TypeScript + Vite.
- Users get multiple independent floating notes.
- Normal notes and the Harness Task Note are separate concepts.
- All data stays local.
- There is no large main window; notes are small floating windows.
- A system tray is the permanent entry point.
- No launch-at-startup by default.
- The Harness Task Note shows only currently running tasks.
- Harness integration will eventually support both push (harness reports in)
  and pull (app reads harness state).

## 3. Architecture

- **React/TypeScript** owns UI, per-view state and user interaction only.
- **Rust/Tauri** owns window lifecycle, the tray, OS capabilities, local paths
  and (later) harness communication. React asks Rust via commands; it never
  drives desktop lifecycle itself.
- **Windows**: each window mounts its own React root. `App.tsx` picks a view
  from that window's own label, so there is no shared root window and closing
  any window cannot affect the others.
- **Exit model**: closing windows is independent of quitting. `lib.rs` vetoes
  `ExitRequested` when `code` is `None` (a user/OS-initiated close) so the tray
  survives zero windows; the tray "Exit" item calls `app.exit(0)` with a code,
  which is allowed through.
- **Local data**: `paths.rs` is the only module that resolves the data
  directory. Nothing is written yet; Phase 1 starts using it.

## 4. Current Phase

Phase 0 — Desktop Skeleton

Status: Completed

Done:

- Tauri 2 + React + TS + Vite project skeleton with the agreed folder layout.
- Main window showing the Phase 0 placeholder and the resolved App Data path.
- Runtime creation of independent extra windows with unique labels.
- System tray with **Open Window** and **Exit**.
- Single owner for the app data directory, exposed to the UI via a command.
- Verified behaviour on Windows (see Verification).

Not done (intentionally, do not start without a new task):

- Note content, Markdown, todos, persistence, SQLite, themes, harness work.

## 5. Important Files

```text
src/App.tsx                      Chooses the view for the current window label
src/windows/MainWindow.tsx       Main window UI; fetches the App Data path
src/windows/TestNoteWindow.tsx   UI for dynamically created windows
src/components/NoteShell.tsx     Minimal rounded-window visual shell
src/components/DevToolbar.tsx    Temporary "Create Test Window" button
src/services/desktop.ts          Typed wrappers over Rust commands
src/types/desktop.ts             Window role/label types
src/utils/logger.ts              Console logging helpers

src-tauri/src/lib.rs             Setup, command registration, exit handling
src-tauri/src/windows.rs         Window creation, label allocation, show/focus
src-tauri/src/tray.rs            Tray icon, menu, menu-event handling
src-tauri/src/paths.rs           Sole owner of the local data directory
src-tauri/capabilities/default.json  Applies core permissions to all windows
src-tauri/tauri.conf.json        Window config and bundling
```

## 6. Current Behavior

- `npm run tauri dev` opens one main window (label `main`) titled "Sticky
  Harness", showing `便签`, `Phase 0`, `Desktop app is running.`, and the App
  Data path.
- The window is draggable, resizable, and has a minimum size.
- **Create Test Window** creates `test-note-1`, `test-note-2`, ... with no
  collisions, repeatedly.
- Closing any single window leaves the others and the process running.
- Closing *all* windows leaves the process and tray running.
- Tray **Open Window** focuses the main window, or recreates it if closed.
- Tray **Exit** quits the process.

## 7. Verification

Run on Windows (Node 24.11.0, Rust 1.91.1, MSVC toolchain):

```text
npm run typecheck      PASS
npm run build          PASS
cargo check            PASS (no warnings)
npm run tauri dev      PASS (app launches)
npm run tauri build    PARTIAL  .exe + .msi built; NSIS step fails (env issue)
```

Behaviour confirmed against the running app:

```text
App Data path logged/rendered   PASS  %APPDATA%\com.stickyharness.desktop
Main window renders             PASS
Created 4 extra windows         PASS  labels test-note-2..5, unique
Closed one window               PASS  5 -> 4 windows, process alive
Closed all windows              PASS  process + tray alive
Tray "Open Window" (no window)  PASS  recreated `main`
Tray "Exit"                     PASS  process exited
```

## 8. Known Issues

- `npm run tauri build` produces the app `.exe` and the `.msi` installer, but
  the **NSIS** bundle step fails with
  `failed to bundle project: 系统无法将文件移到不同的磁盘驱动器。 (os error 17)`
  while extracting its own toolchain. Verified environmental, not a project
  defect: a pristine `create-tauri-app` scaffold fails identically on this
  machine. `--bundles msi` succeeds.
- This machine's MSVC lives at `F:\software\VisualStudio` and is not registered
  with `vswhere`, so plain `cargo build` fails with a missing `link.exe`. Call
  `F:\software\VisualStudio\VC\Auxiliary\Build\vcvars64.bat` first. Environment
  quirk, not a project defect.
- Automated UI verification needed temporary CDP / UI-Automation helper scripts
  that have been deleted; re-create them if runtime behaviour must be re-tested.

## 9. Next Step

Next: Phase 1 — Normal Sticky Notes

Likely direction: real note windows with a stable note identity, persisted note
content stored under the `paths.rs` data directory, and window position/size
restored on restart. Replace the temporary test-window command and dev toolbar.

## 10. Latest Commit

Recorded by the commit that introduced this file:

```text
see `git log -1` — "feat: bootstrap desktop sticky note app"
```
