# Sticky Harness

A Windows-first desktop app for minimal floating sticky notes, plus a local
panel that shows what AI harness tasks are currently running on your machine.

Everything stays on your computer. There is no account, no sync and no server.

## Current Phase

**Phase 5 — Harness Task Note (Completed)**

A read-only "Harness Tasks" window shows what the local harness protocol
currently has running. It polls the registry once a second and lists live
active tasks grouped by harness, with the harness name, task title, status and
elapsed time. It is not a Markdown note: it has no editor, is never saved as a
note file, and its window position, size and Pin live in their own settings
file. See **Harness Tasks Window** below.

**Phase 4 — Harness Protocol (Completed)**

There is a small local protocol for reporting what an AI harness is running.
It is vendor-neutral: it knows nothing about Codex, DeepSeek or any other
harness, so an adapter for a real one goes in front of the protocol later
rather than inside it.

**Phase 3 — Desktop Experience (Completed)**

Notes are still edited as Markdown in the window itself: headings, emphasis,
lists, code, quotes, links and checkboxes. The file on disk is still one JSON
note, and `content` is the Markdown string. Ctrl+click opens a web or mail link.

The tray is the app's desktop surface:

| Tray item | What it does |
| --- | --- |
| New Note | Creates and opens another note |
| Harness Tasks | Opens the Harness Task Note, or focuses it if it is already open |
| Show All Notes | Reveals every window that is hidden, notes and the Harness Task Note alike |
| Hide All Notes | Hides every window without closing it |
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

The harness push endpoint adds no dependency: it is a small hand-written
`std::net` server, because three routes and a bounded body do not justify a web
framework.

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
│  │  ├─ harness/              # Harness protocol, registry, push endpoint
│  │  │  ├─ protocol.rs        # The model, status enum and validation
│  │  │  ├─ registry.rs        # In-memory state and active-task filtering
│  │  │  ├─ server.rs          # Loopback-only push endpoint
│  │  │  └─ adapter.rs         # Pull seam for a future harness adapter
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

The Harness Task Note keeps its window state in its own file next to that
directory, not inside it, because it is not a note:

```text
%APPDATA%\com.stickyharness.desktop\
├─ notes\                     # one JSON file per note (user content)
└─ harness-task-window.json    # position, size and Pin only; no task data
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
- **The Harness Task Note is not a note.** Closing it with `X` hides it instead
  of deleting anything, because nothing in it is user content. There is only
  ever one, and it never appears in the `notes` directory.
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
- **Harness state is runtime-only.** A harness that reports in is remembered
  until the app exits and is not written to disk, so a restart starts with an
  empty registry. See **Harness Protocol** above.

## Harness Protocol

A local AI harness can tell this app what it is running by POSTing to a
loopback-only endpoint. This is the foundation for the Harness Task Note
(Phase 5); no harness is integrated yet, and nothing is displayed in the UI.

The server binds `127.0.0.1:17899` only — never `0.0.0.0` — so nothing on your
network can reach it. State lives in memory and is gone when the app exits.

| Endpoint | What it does |
| --- | --- |
| `GET /health` | Liveness, plus the harness and active-task counts |
| `POST /api/harness/snapshot` | Submit this harness's current state |
| `GET /api/harness/snapshots` | Read back the latest snapshot per harness |

A snapshot describes tasks, not transcripts. Statuses are `running`, `waiting`,
`failed`, `completed`, `cancelled` and `unknown`; the first two count as active.
Times are Unix milliseconds, so a UI computes elapsed time itself.

```json
{
  "harness_id": "mybot",
  "harness_name": "MyBot",
  "updated_at": 1790340000000,
  "tasks": [
    {
      "task_id": "task-123",
      "title": "Run tests",
      "status": "running",
      "started_at": 1790340000000,
      "updated_at": 1790340000000
    }
  ]
}
```

Try it while the app is running (PowerShell):

```powershell
$body = '{"harness_id":"demo","harness_name":"Demo","tasks":[{"task_id":"t1","title":"Run tests","status":"running","started_at":1790340000000,"updated_at":1790340000000}]}'
Invoke-RestMethod http://127.0.0.1:17899/api/harness/snapshot -Method Post -ContentType application/json -Body $body
Invoke-RestMethod http://127.0.0.1:17899/api/harness/snapshots
```

Posting the same `harness_id` again replaces that harness's snapshot rather than
adding another. Invalid payloads are rejected with a `4xx` and a readable reason
and never reach the stored state. If the port is already in use, the app logs it
and keeps working; notes and the tray are unaffected.

## Harness Tasks Window

The **Harness Tasks** window answers one question: what is running right now.

Open it from the tray with **Harness Tasks**. It is a single window — asking for
it again shows and focuses the same one rather than creating a second. Closing
it with `X` only hides it; the tray item brings it back.

It shows live active tasks only: tasks whose status is `running` or `waiting`
and whose harness has reported in recently. A harness that stops reporting goes
stale, and its tasks drop out of the window without being deleted — the snapshot
is still there, and the next report brings them straight back. Tasks are grouped
under their harness name in the order the harnesses first reported, oldest task
first, so rows do not jump around between refreshes.

The list refreshes once a second, which is plenty for a status view; elapsed
times are computed from `started_at` in the window itself. Each row shows the
task title, its status and its elapsed time, plus a short `message` when the
harness sent one. Nothing else is displayed — no ids, no logs, no tool calls.

Harness state comes from the localhost protocol above, so the window is empty
until something reports in. The registry is in-memory only: **after an app
restart the window is restored (position, size and Pin intact) but the list
starts empty** until a harness POSTs again.

## Roadmap

- Phase 1 — Normal sticky notes ✅
- Phase 2 — Markdown / Todo ✅
- Phase 3 — Desktop experience ✅
- Phase 4 — Harness Protocol ✅
- Phase 5 — Harness Task Note ✅
- Phase 6 — Harness Adapters

Phases 1 to 5 are implemented, and nothing is pushed anywhere. See
`AI_HANDOFF.md` for the detailed current state and the next step.
