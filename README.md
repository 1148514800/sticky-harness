# Sticky Harness

A Windows-first desktop app for minimal floating sticky notes, plus a local
panel that shows what AI harness tasks are currently running on your machine.

Everything stays on your computer. There is no account, no sync and no server.

## Current Phase

**Phase 7 — Adapter Management (Completed)**

Adapters no longer need a hand-edited JSON file. A **Harness Adapters** window in
the tray lists every adapter with its source and last status, and can add, edit,
enable, disable, delete and reload them. It writes the same
`harness-adapters.json` as before, and Rust still owns every validation rule, so
the window can never save a configuration the startup path would refuse. See
**Adapter Management** below.

The tray Exit path was measured and left unchanged: the Rust side finishes in
about 34 ms, and the couple of seconds that follow are Windows and WebView2
tearing the windows down, which no adapter affects. The 1.5 s flush budget stays
as it is, because it exists to save your last keystrokes.

**Phase 6 — Harness Adapters (Completed)**

Something now produces the harness state the app displays. Two adapters, both
local and both read-only, poll a harness's own status document: one reads a
JSON file, one reads a loopback HTTP endpoint. Which ones run is decided by
`harness-adapters.json`; with no file, nothing runs and the app behaves exactly
as it did before. See **Harness Adapters** below.

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
| Harness Adapters | Opens the Adapter Management window, or focuses it if it is already open |
| Show All Notes | Reveals every hidden window: notes, the Harness Task Note and the adapter window alike |
| Hide All Notes | Hides every window without closing it |
| Start with Windows | Ticks or unticks starting the app when you sign in |
| Exit | Quits the app and keeps every note |

Hiding is session-only: hidden notes are not deleted, not saved and not
remembered across a restart. **Start with Windows** is the real Windows
autostart entry, managed by the official Tauri autostart plugin and ticked from
the actual OS state. A note can be resized down to 220x160. Images, themes,
search and a general settings window are not in yet; adapters have their own
management window.

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
│  │  │  ├─ adapter.rs         # LocalJsonAdapter / LocalHttpAdapter
│  │  │  ├─ manager.rs         # Adapter config, workers, failure isolation
│  │  │  └─ adapters_window.rs # The Adapter Management window and commands
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
  notes on exit is never intended behaviour. Exit waits up to 1.5 s so the last
  keystrokes can be saved; after that, Windows and the WebView2 runtime need a
  couple of seconds to tear the windows down, which is normal for this stack and
  not something the app waits on.
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
- **The Adapter Management window follows the same rule.** Tray ->
  Harness Adapters opens it once; closing it hides it rather than deleting
  anything, `Show All` / `Hide All` include it, and its configuration lives in
  `harness-adapters.json` rather than in the window.
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

## Harness Adapters

A harness can report in two ways, and neither needs an SDK:

| Path | How it works |
| --- | --- |
| Pull | An adapter reads a JSON file or a loopback HTTP endpoint on a timer |
| Push | The harness POSTs to `/api/harness/snapshot` itself |

Both end in the same registry, so the Harness Tasks window cannot tell them
apart. Adapters are configured in `harness-adapters.json` next to the notes
directory:

```json
{
  "adapters": [
    { "name": "mybot", "kind": "local-json", "path": "C:/tmp/mybot.json" },
    { "name": "sidecar", "kind": "local-http", "port": 18001 }
  ]
}
```

| Field | Applies to | Meaning |
| --- | --- | --- |
| `name` | both | Identity in logs and in the stored snapshot's source |
| `kind` | both | `local-json` or `local-http` |
| `enabled` | both | Defaults to `true`; `false` means it never runs |
| `poll_interval_millis` | both | Defaults to 5000, floor 250, ceiling 600000 |
| `path` | `local-json` | The file to read |
| `port` | `local-http` | The loopback port to read |
| `http_path` | `local-http` | Defaults to `/api/harness/snapshot` |
| `timeout_millis` | `local-http` | Defaults to 1000 |

An unknown key is an error rather than being ignored, so a typo is reported
instead of silently disabling the field it was meant to set. A bad
configuration is logged and the app starts with no adapters: harness reporting
is a feature, never a prerequisite for notes or the tray.

What the adapters are allowed to do is deliberately small. `local-json` opens
one file and parses it, and never runs anything. `local-http` only ever
connects to `127.0.0.1`, with a request timeout and a 256 KB response cap; the
address is built from the port alone, so a configuration cannot point an adapter
at a remote host. Both refuse to buffer anything past that cap.

A failing adapter is isolated: it is logged and retried on its next tick, and
the other adapters keep running untouched. A failure never deletes the harness's
last good snapshot - the staleness rule retires it instead, so a producer that
comes back reappears on its own.

Adapters stop when the app exits, so **Exit** in the tray never waits on a
producer. The whole Rust exit path runs in a few tens of milliseconds; the second
or two that follows is Windows and the WebView2 runtime tearing down the windows,
which happens with or without adapters.

## Adapter Management

**Tray -> Harness Adapters** opens one window that lists every configured
adapter with its name, type, source, enabled state and last status. From there
you can:

| Action | What it does |
| --- | --- |
| Add Local JSON | A new adapter reading one JSON file |
| Add Local HTTP | A new adapter reading a loopback port |
| Edit | Change any field of an existing adapter, including its name |
| Enable / Disable | Stop or restart one adapter without touching the rest |
| Delete | Remove one adapter |
| Reload | Re-read `harness-adapters.json` after editing it by hand |

The status column comes from the last poll: `ok` with the harness it reported,
`error` with the producer's message, `rejected` when a producer sent something
the protocol refused, `waiting` before the first poll, or `off` while the
adapter is disabled. There is no log, no history and no per-task detail here -
the Harness Tasks window already answers "what is running".

The window writes the same `harness-adapters.json` as before, and every edit
goes through the same Rust validation the startup path uses. Two consequences
worth knowing:

- **A rejected edit changes nothing.** The message appears in the window, the
  file on disk is left exactly as it was, and every other adapter keeps running.
- **A file that cannot be read says so.** If `harness-adapters.json` is
  hand-edited into something that does not parse or does not validate, the window
  shows the reason instead of an empty list, and saving any adapter replaces the
  bad file with a good one. At startup the same file is logged and ignored, so
  notes and the tray always come up.
- **A name is not a free label.** Renaming an adapter is an edit and replaces
  the old entry; adding a second adapter with a name that is already taken is
  refused rather than silently overwriting the one you cannot see from the add
  form.

Disabling is a real stop, not a filtered view: that adapter's worker leaves and
its snapshot ages out through the normal staleness rule, exactly as if the
producer had gone quiet. Enabling it again starts it immediately.

Adding, editing, deleting, enabling or disabling writes the file first and only
then restarts the adapters, so a failed write leaves the running set untouched.
The restart covers every adapter, not just the one that changed - one code path
for validation and isolation is worth one skipped poll.

The window is a utility window, not a note: closing it hides it, `Show All
Notes` and `Hide All Notes` include it, and it is not restored on startup. The
tray item reopens it.

### Why there is no Codex or DeepSeek adapter

Both are supported through the paths above - a bridge process that writes a JSON
file, serves a loopback endpoint, or POSTs to the push endpoint. They are not
supported by reading their internal state directly, because on this machine
there is no stable, read-only source for "what is this harness running right
now". Codex's local database describes past sessions (titles, timestamps,
archived flags) and has no notion of a currently running task, so an adapter
built on it could only report guesses.

## Roadmap

- Phase 1 — Normal sticky notes ✅
- Phase 2 — Markdown / Todo ✅
- Phase 3 — Desktop experience ✅
- Phase 4 — Harness Protocol ✅
- Phase 5 — Harness Task Note ✅
- Phase 6 — Harness Adapters ✅
- Phase 7 — Adapter Management ✅
- Phase 8 — Codex / Harness Bridge Experience

Phases 1 to 7 are implemented, and nothing is pushed anywhere. See
`AI_HANDOFF.md` for the detailed current state and the next step.
