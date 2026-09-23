# Sticky Harness

A Windows-first desktop app for minimal floating sticky notes, plus a local
panel that shows what AI harness tasks are currently running on your machine.

Everything stays on your computer. There is no account, no sync and no server.

## Current Phase

**Phase 0 — Desktop Skeleton (Completed)**

Phase 0 deliberately builds only the desktop shell: a window, the ability to
create more windows at runtime, a system tray, and one place that owns the
local data directory. Notes, Markdown, todos, saving and harness integration do
not exist yet.

## Tech Stack

| Layer | Choice |
| --- | --- |
| Shell | Tauri 2 (Rust) |
| UI | React 19 + TypeScript |
| Bundler | Vite 8 |
| Package manager | npm |

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

This starts Vite on port 1420 and launches the desktop app. The app window
shows the current phase and the resolved App Data path. Use
**Create Test Window** to open additional windows.

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
│  ├─ components/              # Reusable UI pieces
│  ├─ windows/                 # One view per window role
│  ├─ services/                # Thin wrappers over Rust commands
│  ├─ types/                   # Shared frontend types
│  ├─ utils/                   # Small helpers (logging)
│  ├─ App.tsx                  # Root component, picks a view per window
│  └─ main.tsx                 # React entry point
├─ src-tauri/                  # Rust (desktop lifecycle and OS access)
│  ├─ src/
│  │  ├─ lib.rs                # App entry: setup, commands, exit handling
│  │  ├─ tray.rs               # System tray icon and its menu
│  │  ├─ windows.rs            # Window creation and focus/show logic
│  │  └─ paths.rs              # The one owner of the local data directory
│  ├─ capabilities/default.json
│  └─ tauri.conf.json
├─ AI_HANDOFF.md               # Living handoff doc for AI-assisted work
└─ README.md
```

## Responsibilities

The split matters and should stay intact as the app grows:

- **React / TypeScript** owns UI, view state and user interaction.
- **Rust / Tauri** owns window lifecycle, the tray, OS capabilities, local
  paths, and (later) local harness communication.

React never drives desktop lifecycle directly; it asks Rust through commands.

## Local Data

All future user data — notes, config, harness settings, window state — will live
under the OS app data directory resolved by Tauri, currently:

```text
%APPDATA%\com.stickyharness.desktop
```

`src-tauri/src/paths.rs` is the only module that decides this location. Never
hardcode a path or place user data in the project directory.

## Development Behaviour To Know

- Closing a window does **not** quit the app. The tray stays alive so windows
  can be reopened.
- Choosing **Exit** in the tray menu quits the process.
- Newly created windows are named `test-note-1`, `test-note-2`, and so on.

## Roadmap

- Phase 1 — Normal sticky notes
- Phase 2 — Markdown / Todo
- Phase 3 — Desktop experience
- Phase 4 — Harness Protocol
- Phase 5 — Harness Task Note
- Phase 6 — Harness Adapters

Only Phase 0 is implemented. See `AI_HANDOFF.md` for the detailed current
state and next step.
