# Sticky Harness

A Windows-first desktop app for minimal floating sticky notes, plus a local
panel that shows what AI harness tasks are currently running on your machine.

Everything stays on your computer. There is no account, no sync and no server.

## Current Phase

**Phase 3 — Desktop Experience (Completed)**

Notes are still edited as Markdown in the window itself: headings, emphasis,
lists, code, quotes, links and checkboxes. The file on disk is still one JSON
note, and `content` is the Markdown string. Ctrl+click opens a web or mail link.

The tray is the app's desktop surface:

| Tray item | What it does |
| --- | --- |
| New Note | Creates and opens another note |
| Show All Notes | Reveals every note window that is hidden |
| Hide All Notes | Hides every note window without closing it |
| Start with Windows | Ticks or unticks starting the app when you sign in |
| Exit | Quits the app and keeps every note |

Hiding is session-only: hidden notes are not deleted, not saved and not
remembered across a restart. **Start with Windows** is the real Windows
autostart entry, managed by the official Tauri autostart plugin and ticked from
the actual OS state. A note can be resized down to 220x160. Images, themes,
search, a settings window and harness integration are not in yet.

## Tech Stack

| Layer | Choice |
| --- | --- |
| Shell | Tauri 2 (Rust) |
| UI | React 19 + TypeScript |
| Bundler | Vite 8 |
| Package manager | npm |

Rust plugins in use: `tauri-plugin-opener` (open links in the default app) and
`tauri-plugin-autostart` (the official launch-at-startup implementation behind
the tray's **Start with Windows** item). Both are declared in
`src-tauri/Cargo.toml`; there is no hand-written registry or startup-folder
code of our own.

## Requirements

- Windows 10/11 (this is the only platform verified so far)
- [Node.js](https://nodejs.org/) 20 or newer (developed on 24)
- [Rust](https://rustup.rs/) 1.77 or newer (developed on 1.91)
- Microsoft C++ Build Tools (the MSVC linker) — required to compile Rust
- WebView2 runtime — preinstalled on current Windows 10/11

> If `cargo build` reports a missing `link.exe` linker, a Visual Studio or Build
> Tools installation with the "Desktop development with C++" workload is
> missing. Installing it fixes the build.

## Install

```bash
npm install
```

## Run In Development

```bash
npm run tauri dev
```

This starts Vite on port 1420 and launches the desktop app. On first run it
opens one blank note. Use the `+` button inside a note, or **New Note** in the
tray menu, to create more.

Other useful scripts:

```bash
npm run dev        # Vite only, in a browser (no Tauri APIs available)
npm run typecheck  # TypeScript, no emit
npm run build      # typecheck + production frontend build
```

## Build The Windows App

```bash
npm run tauri build
```

Installers and executables are written to `src-tauri/target/release/`
(and `src-tauri/target/release/bundle/` for installers).

If the NSIS installer step fails with a cross-drive error while extracting its
toolchain, build just the MSI instead:

```bash
npm run tauri build -- --bundles msi
```

## Project Structure

```text
sticky-harness/
├─ src/                        # React + TypeScript (UI only)
│  ├─ services/                # Thin wrappers over Rust commands
│  ├─ types/                   # Shared frontend types
│  ├─ utils/                   # Small helpers (logging)
│  ├─ windows/                 # One view per window role
│  ├─ App.tsx                  # Root component, picks a view per window
│  └─ main.tsx                 # React entry point
├─ src-tauri/                  # Rust (desktop lifecycle and OS access)
│  ├─ src/
│  │  ├─ lib.rs                # App entry: setup, commands, exit handling
│  │  ├─ notes.rs              # Note identity, persistence and window lifecycle
│  │  ├─ tray.rs               # Tray icon, its menu and the autostart checkbox
│  │  └─ paths.rs              # The one owner of the local data directory
│  ├─ capabilities/default.json
│  └─ tauri.conf.json
├─ AI_HANDOFF.md               # Living handoff doc for AI-assisted work
└─ README.md
```

## Responsibilities

The split matters and should stay intact as the app grows:

- **React / TypeScript** owns UI, view state and user interaction.
- **Rust / Tauri** owns window lifecycle, note identity, persistence, the tray,
  OS capabilities, local paths, and (later) local harness communication.

React never drives desktop lifecycle or touches note files directly; it asks
Rust through commands.

## Local Data

Every note is one JSON file under the OS app data directory resolved by Tauri,
currently on Windows:

```text
%APPDATA%\com.stickyharness.desktop\
└─ notes\
   ├─ <note-id>.json
   └─ <note-id>.json
```

`src-tauri/src/paths.rs` is the only module that decides this location. Never
hardcode a path or place user data in the project directory.

## Development Behaviour To Know

- **Closing a note deletes it.** The window `X` button removes both the window
  and its JSON file, with no confirmation.
- **Closing every note does not quit the app.** The tray stays alive so you can
  create a note again from **New Note**.
- **Choosing Exit in the tray quits the process and keeps every note.** Deleting
  notes on exit is never intended behaviour. Exit waits briefly so the last
  keystrokes can be saved.
- **Ctrl+click opens a link.** A normal click edits it. Only http, https and
  mailto links open.
- **Hide All Notes hides; it never deletes.** Every note window disappears, but
  its file, text, size and Pin are untouched, so Show All Notes brings them back
  exactly as they were. Hidden is a session state: restarting the app shows the
  notes again.
- **New Note does not reveal hidden notes.** Hiding is only undone by Show All
  Notes, so a note you hid stays hidden even while you create a new one.
- **Start with Windows is the real thing.** It writes the same Windows
  autostart entry the OS itself uses, through the official Tauri plugin; the
  tick is read back from the OS, so it is still correct if the entry was changed
  outside the app. There is no registry editing and no startup-folder shortcut
  of our own.
- **A note can be as small as 220x160.** At that size the `+` and Pin buttons,
  todo checkboxes and scrolling all still work; the toolbar takes 29 px and the
  note keeps the rest.
- **Exit saves hidden notes too.** Quitting flushes every open note, whether or
  not it is currently hidden.

## Roadmap

- Phase 1 — Normal sticky notes ✅
- Phase 2 — Markdown / Todo ✅
- Phase 3 — Desktop experience ✅
- Phase 4 — Harness Protocol
- Phase 5 — Harness Task Note
- Phase 6 — Harness Adapters

Phases 1 to 3 are implemented, and nothing is pushed anywhere. See
`AI_HANDOFF.md` for the detailed current state and the next step.
