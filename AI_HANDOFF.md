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
- One note = one JSON file, one window, one stable id. No SQLite.
- Closing a note window means deleting that note, with no confirmation.
- Tray Exit must never delete notes.

## 3. Architecture

- **React/TypeScript** owns UI, per-view state and user interaction only.
- **Rust/Tauri** owns note identity, persistence, window lifecycle, the tray,
  OS capabilities, local paths and (later) harness communication. React asks
  Rust via commands and never touches note files or desktop lifecycle itself.
- **Windows**: each window mounts its own React root. `App.tsx` reads that
  window's own label and renders `NoteWindow` for `note-<id>` labels, so
  there is no shared root window and closing any note cannot affect the others.
- **One entry point per action**: `notes::create_and_open` creates a note for
  startup, the in-note `+` button and the tray New Note item alike. Do not add
  a second window-creation path when the Harness note arrives.
- **Note identity**: `notes.rs` generates `{unix-millis:x}-{sequence:x}` ids.
  The id is the JSON file stem and is embedded in the window label
  (`note-<id>`), so the file name is the authoritative id.
- **Persistence**: `paths.rs` is the only module that resolves the data
  directory; `notes.rs` is the only module that reads or writes note files.
- **Adapter boundary**: `harness/adapter.rs` owns the *pull* seam - the
  `HarnessAdapter` trait and the two implementations that read a producer's own
  document, `LocalJsonAdapter` (one file, read-only, size-capped) and
  `LocalHttpAdapter` (loopback only, request timeout, 256 KB response cap).
  `harness/manager.rs` owns their lifecycle: it reads
  `<AppData>/harness-adapters.json`, runs one worker thread per enabled
  adapter, isolates a failing adapter from the others, and writes successes
  into the registry through the very same `HarnessSnapshot` validation the
  push endpoint uses. An adapter is a *source*, never a path into the app: it
  cannot write notes or reach the network beyond `127.0.0.1`.
  `harness/adapters_window.rs` is the config surface in front of that: it renders
  the configuration joined with the last health reading, and every mutation it
  offers re-validates the whole resulting configuration before writing the file,
  so the window cannot save what the startup path would refuse.
- **Liveness is local**: `harness/registry.rs` decides what is alive, and the
  only input is the local clock reading of the last time a snapshot actually
  *changed*. Nothing compares a producer's clock with ours, so a producer
  clock that is wrong cannot make its task immortal or instantly dead, and a
  pull adapter re-reading an unchanged document slowly goes quiet on its own.
- **Harness boundary**: `harness/protocol.rs` owns what a harness snapshot *is*
  and every validation rule; `harness/registry.rs` owns the running state and is
  the only place that decides what "active" means; `harness/server.rs` is the
  loopback-only push transport; `harness/adapter.rs` is the pull seam. No vendor
  name appears anywhere in that list, which is the point: a Codex or DeepSeek
  adapter is written *in front of* this model, translating its own format into
  `HarnessSnapshot`. If the core had to understand a vendor, every new harness
  would mean changing the registry, the note and the tests.
- **Exit model**: two independent ideas. `lib.rs` vetoes `ExitRequested` when
  `code` is `None`, so deleting every note leaves the tray alive. The tray
  Exit item calls `notes::begin_exit` and then `app.exit(0)`. While the
  `exiting` flag is set, `CloseRequested` returns early instead of deleting,
  so shutdown can never be mistaken for a user delete.
- **Visibility is not lifetime**: `notes::hide_all_notes` only calls
  `window.hide()` and `notes::show_all_notes` only calls `window.show()` plus
  `set_focus()`. Neither creates a window, touches a file, or changes
  geometry, content or Pin, so hidden is a session-only state that vanishes on
  restart. Hiding is never undone implicitly: a note created while others are
  hidden appears on its own and leaves the rest hidden.

### Persistence format

One note is one file: `<AppData>/notes/<note-id>.json`. The file name is
the note id and the window label is `note-<id>`, so id, file and window
always agree. A file whose inner `id` disagrees is corrected to its file name
on load rather than forking into a second file.

```json
{
  "id": "1a0d3f574bb-0",
  "content": "",
  "created_at": 1790262293502,
  "updated_at": 1790262293502,
  "window": {
    "x": 60, "y": 60, "width": 330, "height": 300,
    "always_on_top": false
  }
}
```

- Written temp-file-then-rename; if the rename is refused it falls back to
  copying over the target (see Known Issues).
- Every field except `id` is optional on load, so a partial or
  hand-written file still opens. Geometry is physical pixels; size is the
  inner size, position is the outer position.
- `content` is Markdown source. The editor turns it into a rich document
  only in memory; Rust stores the string and does not interpret it. Phase 1
  plain text is valid Markdown, so old notes open as paragraphs.
- A note window needs `core:window:allow-destroy` in its capability. The
  frontend `onCloseRequested` listener closes the window for real by calling
  `destroy()` once its handler returns, so without that permission the X
  button deleted the note's JSON while the window itself stayed open, leaving
  a window whose file was already gone. Deletion is driven by Rust's
  `CloseRequested` handler; the permission only lets the window finish closing.
- A file that cannot be parsed is logged and skipped; the other notes still
  load. A saved position with no real overlap with any current monitor is
  replaced by a cascaded position on the primary monitor.

### Window lifecycle

- `notes::create_and_open` is the only way a note window comes into
  being: startup restore, the in-note `+` button and tray New Note all call it.
- Windows are built hidden, positioned, then shown, so they never flash at
  the default position.
- `CloseRequested` deletes the note unless the process is exiting.
- `Moved`/`Resized` schedule a debounced geometry write (400 ms, latest
  event wins).
- Window-creating commands are `async` on purpose: window creation happens
  on the event loop delivering the invoke, so a synchronous command
  deadlocks.
- The minimum inner size is 220x160; `MIN_WIDTH`/`MIN_HEIGHT` in `notes.rs` are
  the only place it is set. Below that the toolbar and the note body stop
  fitting together.

## 4. Current Phase

Phase 10 — Final QA / v1.0 Release

Status: Completed

Release: v1.0.0

Done:

- Phases 1 to 9, unchanged in behaviour: no feature was added, no abstraction
  moved and no data format changed. See 4b, 4c, 4d, 4e and 4f below.
- **One version, five files.** `tauri.conf.json`, `Cargo.toml`, the
  `sticky-harness` entry in `Cargo.lock`, `package.json` and both
  `package-lock.json` entries all read `1.0.0`, so the installer, the crate and
  the frontend agree. The bundle identifier is still `com.stickyharness.desktop`
  and the AppData path did not move, so existing installs upgrade in place.
- **The upgrade was actually exercised.** The 0.1.0 build already installed here
  was upgraded with the 1.0.0 MSI: exit 0, one uninstall entry at 1.0.0, and a
  before/after sha256 of every file under `%APPDATA%\com.stickyharness.desktop`
  that came back identical.
- **Everything was re-run on the installed build.** New Note, Markdown and todo,
  Pin, geometry, Harness Tasks, Harness Adapters, Local JSON and Local HTTP
  together, the bridge for Codex and DeepSeek, Hide All / Show All, Start with
  Windows, restart persistence and Tray Exit. A refused adapter edit and a
  corrupt `harness-adapters.json` both failed safely.
- **The hard exit was stressed and not reproduced.** Three `0xc0000409` events,
  all on the same pre-panic-hook build, are recorded honestly rather than
  explained away. See 4a and Known Issues.

There is no separate integration for Codex or DeepSeek: both report through the
same bridge, JSON or HTTP path as any other harness, and neither got a Direct
adapter. The full Phase 10 write-up is in 4a below, and Phase 9's is in 4f.

## 4a. Phase 10 — Final QA / v1.0 Release (Completed)

Done:

- Phases 1 to 9, unchanged: no feature was added, no abstraction changed and no
  data format moved. This phase is a version bump, a pass over the real installed
  build and an honest record of what it turned up.
- **One version, five files.** `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml`,
  the `sticky-harness` package entry in `src-tauri/Cargo.lock`, `package.json`
  and both `package-lock.json` entries read `1.0.0`. The installer follows
  `tauri.conf.json`, so the artifact is `Sticky Harness_1.0.0_x64_en-US.msi`
  (2,703,360 bytes). `identifier` is still `com.stickyharness.desktop`, so the
  AppData directory, the notes and every install path are untouched. The `0.1.0`
  strings that remain in the lockfiles belong to unrelated dependency crates.
- **The upgrade path is real, not theoretical.** The 0.1.0 build already installed
  on this machine was upgraded in place with the 1.0.0 MSI: exit 0, a single
  uninstall entry now reading 1.0.0, the new executable in place, and a
  before/after sha256 of every file under `%APPDATA%\com.stickyharness.desktop`
  that came back identical - notes, `harness-adapters.json` and
  `harness-task-window.json`.
- **The whole product was re-run on the installed 1.0.0 build:** New Note (tray
  and in-note `+`), a Markdown heading and a `- [ ]` todo stored and rendered, Pin
  toggled both ways to disk, geometry written and restored, Harness Tasks, Harness
  Adapters, a Local JSON adapter and a Local HTTP adapter live at the same time,
  the bridge driving Codex and DeepSeek (start, update, running<->waiting, `done`,
  `fail` and `cancel`), Hide All / Show All, Start with Windows ON then OFF,
  restart persistence, and Tray Exit. A refused adapter edit (empty path, then a
  duplicate name) and a corrupt `harness-adapters.json` were each refused with a
  readable message, left the file as it was, kept the other adapters running and
  did not crash the app.
- **Tray Exit was measured two ways so the number means something.** The
  application leaves as soon as it is asked to: `exit_app` - the command the tray
  Exit item runs - to process-gone measured 20 ms, 47 ms, 61 ms and 76 ms across
  runs. The seconds that follow are Windows/Tauri/WebView2 teardown, which no part
  of this app controls, and a tray-click-to-gone figure of ~3.4 s with three
  windows is that teardown plus ~1 s per extra WebView2 window, not the app's own
  exit path.
- **The hard exit was stressed, in a bounded way, and did not reproduce.** Four
  full launch -> harness churn (8 pushes) -> exit cycles on the 1.0.0 build, plus
  12 create/close note cycles and 30 snapshot POSTs: the app exited cleanly every
  time, no `panic.log` was written, and the Windows Error Reporting count for
  `sticky-harness.exe` stayed at three. No speculative change was made to chase
  it; writing a fix for a fault that will not reproduce would be guessing.
- **Nothing temporary shipped.** 73 tracked files, no fixtures, no CDP or
  UI-Automation helpers, no logs, no scratch scripts; the MSI's own file table
  holds only the executable, its library, the bridge script and the one module
  the bridge imports.

Not done (intentionally):

- Code signing, auto-update and MSIX/Store packaging. The MSI is unsigned and
  there is no update feed; both are distribution decisions with their own key and
  threat-model questions, and neither belongs in a QA phase.
- Any change to the exit path, the panic profile or the hard-exit investigation
  beyond collecting evidence. `panic = "abort"` and the panic hook stay as they
  are.
- Any new feature. The product surface is frozen for v1.0.0.

## 4e. Phase 8 — Codex / Harness Bridge Experience (Completed)

Done:

- Phases 1 to 7: sticky notes, Markdown editing, desktop experience, the local
  harness protocol, the Harness Task Note, the harness adapters and the Adapter
  Management window. See 4b, 4c and 4d below.
- **A harness can report in three commands.** `sticky-harness-bridge` is a
  zero-dependency Node CLI (`bin/sticky-harness-bridge.mjs`) over a pure,
  I/O-free module (`src/harness/bridge.ts`). `start`, `update`, `done`, `fail`
  and `cancel` all end in one place: one `HarnessSnapshot` POSTed to
  `127.0.0.1:17899/api/harness/snapshot`. It is a *producer*, so it added no
  abstraction - the protocol, the registry, the push endpoint, the adapters and
  the Harness Task Note are untouched.
- **Node rather than Rust, on purpose.** The project already needs Node to be
  built and run, so a script costs no second toolchain, no second build step and
  no second artifact. Its whole dependency list is `node:http`, `node:fs`,
  `node:os` and `node:path`; Node 24 strips the types from the `.ts` module it
  imports directly, so the tested logic and the running code are the same code.
  The split follows the seam: the *app* is Rust because it owns windows, the
  tray and lifetimes, while a *reporter* that talks to a loopback endpoint is a
  script.
- **One generic bridge, never one per vendor.** The only inputs that distinguish
  a harness are `harness id/name`, `task id/title` and `status`. There is no
  `CodexBridge`, no `DeepSeekBridge`, no `MyBotBridge` and no SDK; a new harness
  is a command, not a code path. A vendor-specific bridge would mean the
  protocol had learned about vendors, which is what the protocol exists to
  prevent.
- **One task, one stable id.** `start` writes a five-field session file
  (harness, task id, title, status, message) under `<user profile>\.sticky-harness\`,
  and every later call reads it, so no call can fork a second row. The id is
  derived once from the title (`Refactor runtime` -> `refactor-runtime`) and
  then never recomputed, so `update --title` renames the row rather than moving
  it. `start` over a live task warns on stderr and replaces it; the warning
  exists because a replaced task is otherwise invisible.
- **A finished task leaves no session behind.** `done`, `fail` and `cancel`
  report a terminal status and then delete the session file, so a later `update`
  answers "no current task" instead of inventing one. Terminal statuses are
  never active, so the row leaves the Harness Tasks window on the next refresh
  while the snapshot stays in the registry like any other finished task.
- **The bridge only reports.** It never starts, stops or prompts a harness,
  never reads a conversation or a harness's own files, never writes a note and
  never kills an agent. It sends what you told it and nothing else.
- **Failure is one readable sentence.** A refused connection, an unusable
  endpoint and an unanswered request each become a specific line on stderr with
  a non-zero exit - `nothing is listening on 127.0.0.1:17899; start Sticky
  Harness first` - and leave no partial state, because the session file is
  written only after the app has accepted the snapshot. A snapshot the app
  *refuses* is reported with the validator's own reason, passed through rather
  than paraphrased.
- **Two wrappers, no SDK.** `examples/run-task.ps1` (a `-Work` scriptblock) and
  `examples/run-task.sh` report a start, run one command and report the outcome
  - including a failure - so a crashed run cannot leave a task sitting in the
  window. They are examples to copy, not an integration to install.
- **`npm run bridge`** is the in-repo entry point; `node
  bin/sticky-harness-bridge.mjs` works anywhere. `--dry-run` prints the snapshot
  and sends nothing, and `--json` echoes what was sent, so the bridge can be
  checked without a running app.

Codex integration mode: Bridge

DeepSeek integration mode: Bridge

Both stay on the standard push and pull paths, and neither gets a Direct
adapter. The investigation is unchanged from 4c: Codex's `state_5.sqlite` holds
thread and session metadata but no stable current-running-task state, and
`codex app-server` was not reachable here, so an adapter built on it could only
report guesses; DeepSeek exposes no stable read-only status source either. The
bridge above is the documented, runnable way both are reached, and it reads
neither harness's private state. Do not re-open Direct until one of them
publishes a documented, read-only "currently running" source.

Not done (intentionally, do not start without a new task):

- No automatic parsing of any harness's internal database, cache or session
  store.
- No Settings expansion, no task history, no log or terminal view, no harness
  control, no prompt sending, no cloud, no SQLite and no remote API.
- No SDK, plugin loader or scripting host. One bridge, one protocol, and the
  same two adapters remain the whole integration surface.

## 4f. Phase 9 — Release / Packaging Polish (Completed)

Done:

- Phases 1 to 8: sticky notes, Markdown editing, desktop experience, the local
  harness protocol, the Harness Task Note, the harness adapters, the Adapter
  Management window and the harness bridge. See 4b, 4c, 4d and 4e below.
- **The app now installs like a normal Windows program.** `npm run tauri build`
  produces a per-user MSI - `Sticky Harness_0.1.0_x64_en-US.msi`, about 2.6 MB -
  that needs no administrator rights and installs to
  `%LOCALAPPDATA%\Programs\Sticky Harness`, with a Start Menu entry, a desktop
  shortcut and an *Uninstall Sticky Harness* entry. No data format changed: the
  installer and the app read the same `<AppData>/com.stickyharness.desktop` files
  as every earlier phase did.
- **Bundling is deliberately MSI-only and minimal.** `bundle.targets` is
  `["msi"]`, because the NSIS bundler cannot extract its own toolchain on this
  machine (an environment fault, reproduced from a pristine scaffold - see Known
  Issues), and a release that cannot build everywhere is worse than one installer
  format. Two `bundle.resources` entries copy `bin/sticky-harness-bridge.mjs` and
  `src/harness/bridge.ts` next to the executable, so the installed copy of the
  bridge reports to the app from anywhere without the repository.
- **Installing touches nothing of yours; uninstalling touches only its own
  files.** A fresh install, an install-over-install upgrade and an uninstall were
  each run while a real user-data directory was present: the note,
  `harness-adapters.json` and `harness-task-window.json` survived all three, and
  reinstalling brought every one back. Uninstall removed the program folder, both
  shortcuts and the registry entry, and left `%APPDATA%\com.stickyharness.desktop`
  untouched. *Start with Windows* is a separate Windows Run entry and is not
  removed by uninstalling.
- **A crash now leaves a note behind.** `lib.rs` installs a panic hook before
  the app is built. Release compiles with `panic = "abort"`, so an internal panic
  is an instant process death whose message would otherwise go to a stderr a
  windowed app does not have; the hook still prints, then writes one line -
  timestamp, location, message - to `<AppData>/panic.log`. `panic = "abort"` is
  deliberately unchanged: unwinding across the FFI boundary into Tauri/WebView2
  would be undefined behaviour.
- **The panic log's path has one owner.** The hook runs before an app handle
  exists and may run while the process is dying, so it cannot use the Tauri
  resolver; `paths::app_data_dir_name` is the one place that knows the directory
  name, and a `paths` test asserts it still equals the bundle `identifier` in
  `tauri.conf.json`. A hook that quietly wrote somewhere the app never reads
  would be worse than no hook at all.
- **A rare hard exit is documented, not hidden.** Two `0xc0000409` events were
  recorded on 2026-09-27 (see Known Issues); neither could be reproduced in 22+
  controlled attempts afterwards, so nothing was "fixed" to pretend otherwise.
  The panic hook above exists so that a recurrence leaves a readable location
  instead of a guess.
- **First run is genuinely empty.** With no prior data the app opens one blank
  note, Harness Tasks and Harness Adapters both say they are empty, and no sample
  data, test adapter or development path appears anywhere.

Not done (intentionally, do not start without a new task):

- Code signing, auto-update and MSIX/Store packaging. Signing in particular is a
  distribution step with its own key management and was not attempted, so the
  MSI is unsigned; NSIS is not attempted on this machine for the reason above.
- Any change to the data format, and any migration code, because nothing in the
  format had to change. An install-over-install upgrade reads the old files
  unchanged.

## 4d. Phase 7 — Adapter Management / Product Polish (Completed)

Done:

- Phases 1 to 6: sticky notes, Markdown editing, desktop experience, the local
  harness protocol, the Harness Task Note and the harness adapters. See 4b and
  4c below.
- **Adapters no longer need a hand-edited file.** Tray -> Harness Adapters opens
  one singleton window listing every configured adapter with its name, type,
  source, enabled state and last status. It can add a local JSON or local HTTP
  adapter, edit one including renaming it, enable or disable it, delete it and
  reload the file. The format of `<AppData>/harness-adapters.json` is unchanged;
  it is still the contract, and the window is just another writer of it.
- **The window asks, Rust decides.** Every mutation runs the same
  `AdapterConfig::validate` the startup path uses, against the whole resulting
  configuration, before anything is written. A rejected edit returns its reason
  to the window, leaves the file on disk exactly as it was, and leaves every
  other adapter running. An add whose name is already taken is refused rather
  than silently overwriting an adapter the add form cannot see.
- **A rename is an edit, not an add.** `save_adapter` takes the adapter's
  previous name, so changing a name replaces the entry in place instead of
  leaving the old one behind - the one way this window could have quietly
  doubled a configuration.
- **Applying a configuration replaces the running set in one step.**
  `HarnessState::apply_adapter_config` stops the previous workers and starts the
  new set; the file is written before the adapters restart, so a write failure
  leaves the running set untouched rather than applying something that is not on
  disk. Add, edit, delete, enable, disable and reload all go through it, so they
  inherit the startup rule that one bad adapter never stops the others.
- **Each adapter reports a last outcome.** `AdapterStatus` / `AdapterOutcome`
  live in memory only and are one line per adapter: `ok` with the harness id it
  reported, `error` with the producer's message, `rejected` when the producer
  sent something the protocol refused, `waiting` before the first poll, or
  `off` while the adapter is disabled. There
  is no history and no log to read - the Harness Task Note still answers "what is
  running", and the window answers only "is this source healthy".
- **Reload is the escape hatch for hand-editing.** The file remains editable in
  any text editor; Reload re-reads and validates it and restarts the adapters
  without a restart of the app.
- **A broken file says so.** `load_checked_for` is what the window reads
  through, so a `harness-adapters.json` that does not parse, or that fails
  validation, is reported in the window with its real reason instead of rendering
  as an empty list that looks like "no adapters configured". Startup keeps the
  lenient behaviour: it logs the reason and runs with no adapters, because
  harness reporting must never stop notes or the tray.
- **Exit needed no change.** The Rust exit path was re-measured and finishes in
  about 34 ms; the remaining latency is Windows/Tauri/WebView2 teardown that no
  adapter affects, and `EXIT_FLUSH_TIMEOUT` stays at 1500 ms so the last
  keystroke is still saved reliably. See Known Issues for the measurements.

Codex integration mode: Bridge

DeepSeek integration mode: Bridge

Unchanged from Phase 6: both harnesses are supported through the standard push
and pull paths, and neither gets a Direct adapter. The reasoning is in 4c.

Not done (intentionally, do not start without a new task):

- Settings UI beyond adapters: themes, images, search. No dashboard, no task
  history, no log, no terminal and no tool-call view in the adapter window.
- Codex or DeepSeek control, prompt sending, any remote network access, SQLite,
  cloud, authentication, WebSocket or SSE. Local HTTP still means loopback only.
- Per-adapter SDKs, a plugin loader or a scripting host. Two adapters, one trait,
  one manager and one registry remain the whole abstraction.

## 4b. Phase 4 — Harness Protocol (Completed)

Done:

- A vendor-neutral protocol for "what is this harness running":
  `HarnessSnapshot` / `HarnessTask` / `HarnessStatus`, defined in
  `harness/protocol.rs` with no reference to any real harness.
- A status enum of `running`, `waiting`, `failed`, `completed`, `cancelled` and
  `unknown`. `unknown` exists only so a future adapter can report a producer
  state it cannot map without inventing a more flattering answer.
- `HarnessStatus::is_active()` is the single definition of "in progress":
  running and waiting. Phase 5 must ask this rather than reimplementing it.
- `HarnessRegistry` holds the latest snapshot per harness in memory, keyed by
  `harness_id`, so a harness that reports again is updated and never duplicated.
- A local push endpoint on `127.0.0.1:17899` accepts snapshots over HTTP:
  `GET /health`, `POST /api/harness/snapshot`, `GET /api/harness/snapshots`.
- Validation rejects empty or oversized ids, empty names, empty titles,
  unknown statuses, malformed timestamps, `updated_at` before `started_at`,
  duplicate task ids, more than 256 tasks and bodies over 256 KB.
- Staleness is computed, never assumed, from the app's own clock: a snapshot
  is stale when `now - received_at` passes the timeout. Phase 4 used the
  producer's `updated_at` as well; Phase 6 removed that, because it made
  staleness depend on the producer's clock agreeing with ours (see 4c).
  Nothing is deleted on staleness.
- The registry is deliberately not persisted. Harness state describes live
  processes, so an empty registry after a restart is the truth.

## 4c. Phase 6 — Harness Adapters (Completed)

Done:

- Phases 1 to 5: sticky notes, Markdown editing, desktop experience, the local
  harness protocol and the Harness Task Note. See 4b below.
- Something finally produces the state the app displays. Two adapters poll a
  harness's own status document and report it through the existing protocol:
  `LocalJsonAdapter` reads a JSON file, `LocalHttpAdapter` reads a loopback
  HTTP endpoint.
- `AdapterManager` owns the whole lifecycle: it loads
  `<AppData>/harness-adapters.json`, runs one worker thread per enabled
  adapter, isolates failures, and writes successes into the existing
  `HarnessRegistry`. No second abstraction: an adapter is still the Phase 4
  `HarnessAdapter` trait, and a snapshot from an adapter is byte-identical to
  one that arrived over HTTP push.
- **Staleness no longer trusts any producer clock.** It is decided only by the
  local time of the last *new evidence* the app saw:


  ```
  now - received_at > stale_timeout   =>   the snapshot is stale
  ```

  `received_at` is the app's own clock reading, never the producer's, so a
  producer whose clock is 30 minutes behind or an hour ahead cannot stale its
  own snapshot early or late.
- **A repeated read is not new evidence.** `HarnessRegistry::upsert` compares
  the incoming snapshot with the stored one and moves `received_at` only when
  something actually changed (harness id, harness name, `updated_at`, or the
  task list). A pull adapter that keeps re-reading identical content therefore
  stops renewing the snapshot's liveness, and a producer that has silently died
  still goes stale after the timeout even though its file or endpoint is still
  readable. Real new content - including a producer-side `updated_at` bump or a
  single task status change - refreshes it immediately.
- The transport is not evidence. `ValidatedSnapshot::differs_from` deliberately
  ignores `source`, so re-labelling the same content as it moves between push
  and pull cannot masquerade as progress.
- **Push is unchanged and still works as a heartbeat.** A repeated POST of
  identical content does not need to count as evidence: the arrival of a POST is
  itself the heartbeat, so push producers keep behaving exactly as they did in
  Phase 5.
- Stale still only hides. `list_live_active_tasks()` is still the single
  consumer of the stale rule, and a stale snapshot is never deleted or
  overwritten - it stays in the registry, so a producer that starts reporting
  again reappears with no mutation in between.
- Failure isolation is per adapter: one adapter's error is logged and retried on
  its next tick, and every other adapter keeps polling untouched. A failure
  never clears the last good snapshot.
- Adapter workers stop on Exit. `AdapterManager::stop` raises a flag and the
  workers leave within their 50 ms sleep slice, so Tray Exit is never held up by
  a slow or hanging adapter.

Codex integration mode: Bridge

DeepSeek integration mode: Bridge

Both harnesses are supported through the two standard paths - a bridge process
that writes a JSON file, serves a loopback endpoint, or POSTs to
`/api/harness/snapshot`. Neither gets a Direct adapter, because on this machine
neither exposes a stable, read-only, repeatable source for "what is this harness
running right now":

- Codex's local `state_5.sqlite` `threads` table holds session metadata only
  (`id`, `rollout_path`, `created_at`, `updated_at`, `title`, `archived`,
  `model`, `cwd`). There is no per-thread run status, so any adapter on top of
  it would report guesses.
- `codex agents` browses past sessions on a daemon, and
  `codex app-server daemon version` failed here with
  `failed to connect to ~/.codex/app-server-control/app-server-control.sock`.
  Codex Desktop is also the host process of this very app, so a Direct adapter
  would have nothing external to observe.
- No equivalent stable status source was found for DeepSeek, so no specialised
  adapter was written for it either.

Not done at the time (Phase 7 added the adapter management surface):

- An adapter management UI and editing outside the JSON file. Phase 6 logged a
  malformed or missing configuration instead of surfacing it; Phase 7 is where
  both changed.
- Codex or DeepSeek control, prompt sending, task history, logs, tool calls, a
  terminal view, search, notifications, SQLite, cloud, authentication,
  WebSocket, SSE, and any remote network access. Local HTTP means loopback only.

## 5. Important Files

```
src/App.tsx                           Picks the view from this window label
src/windows/NoteWindow.tsx            One note: + button, Pin, autosave
src/windows/HarnessTaskWindow.tsx     Read-only task view, polls once a second
src/windows/AdaptersWindow.tsx        Adapter rows + add/edit form, polls every 2s
src/components/HarnessTaskList.tsx    Renders harness groups, tasks, empty state
src/harness/elapsed.ts                MM:SS / H:MM:SS elapsed formatting
src/harness/grouping.ts               Groups live tasks by harness, Core order
src/components/NoteEditor.tsx         WYSIWYG Markdown editor for one note
src/editor/markdown.ts                 Markdown <-> editor document conversion
src/editor/todoInput.ts                Turns a typed `- [ ] ` into a checkbox
src/editor/links.ts                    Which clicks open a link, and which URLs
src/services/desktop.ts                Typed wrappers over the Rust note commands
src/types/desktop.ts                  NoteRecord/NoteWindowState + label parsing
src/utils/logger.ts                   Console logging helpers
src/harness/bridge.ts                 Bridge logic: args, session, snapshot (pure)
src/harness/bridge.test.ts            Its tests; no I/O, no clock, no argv
src/styles.css                        Minimal note styling

src-tauri/src/lib.rs                  Setup, command registration, exit veto
src-tauri/src/notes.rs                Note identity, persistence, window lifecycle
src-tauri/src/tray.rs                 Tray icon, menu, autostart checkbox
src-tauri/src/paths.rs                Sole owner of the data directory and its name
src-tauri/src/harness_window.rs       The Harness Task Note window and its state
src-tauri/src/harness/mod.rs          Harness wiring, Tauri commands, init
src-tauri/src/harness/protocol.rs     The protocol model, status enum, constants
src-tauri/src/harness/registry.rs     In-memory snapshots and active filtering
src-tauri/src/harness/server.rs       Loopback-only push endpoint
src-tauri/src/harness/adapter.rs      HarnessAdapter trait + LocalJson/LocalHttp
src-tauri/src/harness/manager.rs      AdapterManager: config, workers, isolation
src-tauri/src/harness/adapters_window.rs  Adapter Management window + its commands
src-tauri/src/harness/tests.rs        Core tests (validation, registry, stale)
src-tauri/capabilities/default.json   Core permissions for note-*, harness-tasks,
                                      harness-adapters
src-tauri/tauri.conf.json             No startup window; MSI bundle, bundled bridge
bin/sticky-harness-bridge.mjs         The bridge CLI: session file + one POST
examples/run-task.ps1                 Report, run one command, report the outcome
examples/run-task.sh                  The same wrapper for sh
```

## 6. Current Behavior

- `npm run tauri dev` launches straight into the user notes: every saved note
  reopens with its content, position, size and always-on-top, or one blank note
  appears when nothing was saved.
- `+` in a note, and New Note in the tray, both create another note and never
  collide with existing ids.
- Typing saves itself shortly after the last keystroke; there is no Save button
  and no "saved" toast. The stored text is Markdown, including todo checkboxes.
- Ctrl+click opens a link in the default browser or mail app. A plain click
  edits it. Anything other than http, https or mailto is ignored.
- Tray Exit waits up to 1.5s for every open note to finish its last save, then
  quits. It still never deletes notes.
- Moving or resizing a note is written back to disk after a short quiet period,
  so restarting restores the layout.
- Pin toggles always-on-top for that note only, and survives a restart.
- Closing a note window deletes that note and leaves the others untouched.
- Deleting every note leaves the process and tray running; New Note brings the
  app back.
- Hide All Notes hides every note window and keeps them: nothing is deleted and
  nothing is written, so Show All Notes returns the same notes on the same
  screen. A restart shows them again, because hidden is not persisted.
- New Note while notes are hidden opens the new note and leaves the others
  hidden.
- Start with Windows in the tray ticks and unticks the real Windows Run entry
  through the official autostart plugin, and starts ticked if the entry is
  already there.
- A note can be resized down to 220x160; the toolbar stays usable and the note
  body keeps most of the height.
- The Harness Task Note opens from Tray -> Harness Tasks, shows only live active
  tasks grouped by harness, and refreshes once a second. Closing it with X
  hides it; the tray item focuses the same window again rather than creating a
  second one.
- Show All and Hide All cover the Harness Task Note too. Hiding it deletes
  nothing and changes no registry state; showing it never creates it if it was
  never opened.
- Tray -> Harness Adapters opens one Adapter Management window listing every
  configured adapter with its name, type, source, enabled state and last status.
  It can add, edit, rename, enable, disable, delete and reload adapters; a
  refused edit shows the reason and changes nothing. Closing the window hides
  it, Show All / Hide All include it, and the tray item reopens the same window
  rather than creating a second one.
- A disabled adapter is a real stop, not a filtered view: its worker leaves and
  its snapshot ages out through the normal staleness rule. Enabling it again
  starts it immediately.
- Tray Exit quits the process and keeps every note file, including notes that
  were hidden.
- A local harness can POST its current state to
  `http://127.0.0.1:17899/api/harness/snapshot` and the app remembers it in
  memory; `GET /api/harness/snapshots` reads it back, and the Harness Task
  Note displays what is active. Nothing about it is persisted.
- `sticky-harness-bridge start|update|done|fail|cancel` reports the current
  task from any harness as that same POST. One task at a time, one stable id
  across calls, and a session file that is deleted when the task finishes; the
  bridge never reads a harness or drives it. A task reports as live for about
  five minutes after its last call, so a long task should call `update`.
- A task stops being listed when its harness goes quiet for longer than the
  stale timeout (5 minutes by default). The snapshot is not deleted, so the next
  report restores the row.
- The Harness Task Note window keeps its own position, size and Pin in
  `harness-task-window.json`, which is not a note file and never contains task
  data. After a restart the window comes back the same and the list is empty.
- If the harness port is already taken, the app logs the failure and starts
  anyway: notes and the tray are unaffected.
- A corrupt or hand-edited note file is logged and skipped without stopping the
  other notes from loading.
- The release build is one per-user MSI. Installing, upgrading or uninstalling it
  never touches `%APPDATA%\com.stickyharness.desktop`, the installed copy carries
  the bridge at `bin\sticky-harness-bridge.mjs`, and an internal crash writes
  `<AppData>/panic.log` before the process aborts.

## 7. Verification

Run on Windows (Node 24.11.0, Rust 1.91.1, MSVC at F:\software\VisualStudio):

```
npm test                  PASS  67 tests (markdown, todos, link rules)
npm run typecheck         PASS
npm run build             PASS  (only the pre-existing chunk-size warning)
cargo check --all-targets PASS  (no warnings)
cargo build               PASS
cargo build --release     PASS
```

Re-run for Phase 3, after the tray, autostart, minimum size and CSS changes:

```
npm test                  PASS  67 tests (3 files)
npm run typecheck         PASS
npm run build             PASS  (same pre-existing chunk-size warning)
cargo check --all-targets PASS  (no warnings)
cargo build               PASS
```

Re-run for Phase 4, which also adds the Rust core tests:

```
npm test                  PASS  67 tests (3 files)
npm run typecheck         PASS
npm run build             PASS  (same pre-existing chunk-size warning)
cargo test                PASS  39 tests (validation, registry, staleness)
cargo check --all-targets PASS  (no warnings)
cargo build               PASS
```

Re-run for Phase 5, which adds the Harness Task Note, its window module and its
frontend helpers:

```
npm test                  PASS  85 tests (5 files)
npm run typecheck         PASS
npm run build             PASS  (same pre-existing chunk-size warning)
cargo test                PASS  52 tests (validation, registry, staleness,
                                harness window config)
cargo check --all-targets PASS  (no warnings)
cargo build               PASS
```

Behaviour confirmed against a running build, driven through the WebView2 CDP
endpoint and real Win32 window messages. The Markdown editor checklist below
was re-run after the textarea was replaced, and the Phase 2 items were added:

```
Phase 2 (Markdown / Todo)
Todo cold start        PASS  `- [ ] Task`, `- [x] Task` become real task items
Todo persistence       PASS  clicking a checkbox round-trips through Markdown
Underscore round-trip  PASS  FINAL_TOKEN_123456 is stored unescaped
Quick Exit x3          PASS  typed text survived all three Tray Exit runs
  round 1              input->exit <300 ms; process gone in 378 ms
  round 2              input->exit <300 ms; process gone in 357 ms
  round 3              input->exit <300 ms; process gone in 384 ms
  each run             disk + UI = FINAL_TOKEN_123456
In-flight save + exit  PASS  OLD_STATE_ then FINAL_STATE_123456 within the
                             debounce window saved in full; process gone in
                             383 ms; disk + UI = OLD_STATE_FINAL_STATE_123456
Close = Delete race A  PASS  X inside the 300 ms debounce: window gone, JSON
                             gone, still absent after 3 s, not restored
Close = Delete race B  PASS  content + move + resize + Pin all pending at the
                             moment of X: JSON never reappeared, not restored
os error 32            PASS  10/10 serial create -> immediate get_note reads
                             succeeded; the single earlier occurrence did not
                             reproduce (see Known Issues)
Phase 1 regression     PASS  see the Phase 1 checklist below
```

Phase 1 checklist, re-run on the current Markdown editor:

```
1 New notes        PASS  created from the in-note `+` button and from tray
                         New Note; ids and labels unique
2 Content restore  PASS  every note's Markdown returned after a restart
3 Window restore   PASS  moved/resized notes returned at the same position and
                         size (physical px)
4 Pin persist      PASS  only the pinned note came back pinned
5 Delete           PASS  window gone, its JSON gone, others untouched
6 Close all notes  PASS  all JSON removed; process + tray still alive;
                         tray New Note recreated a note
7 Tray Exit        PASS  process exited; every note JSON survived and all
                         notes restored on the next start
8 Corrupt JSON     PASS  bad file logged + skipped; the other notes loaded
Off-screen recovery PASS  a note saved at (9000,9000) was pulled back on screen
```

Phase 3 (Desktop Experience), every scenario run against a live build:

```
Tray menu            PASS  8 items: New Note, Show All Notes, Hide All Notes,
                           Start with Windows, Exit and three separators
A  3 notes -> Hide All PASS 3 windows hidden, 0 visible, process alive,
                           3 JSON files untouched
B  Hide All -> Show All PASS 3 windows visible again; content, geometry
                           (60,60 / 92,92 / 124,124 at 330x300) and Pin
                           unchanged byte for byte
C  Hide All -> New Note PASS new note visible itself, the other notes stayed
                           hidden; Show All was not triggered
D  Autostart OFF -> ON PASS real HKCU Run value =
                           F:\study\note_app\src-tauri\target\debug\
                           sticky-harness.exe ; log
                           `autostart requested=true actual=true`
   Autostart ON -> OFF PASS key gone again; log
                           `autostart requested=false actual=false`;
                           left OFF afterwards, as required
E  fast typing -> Hide PASS typed SCENARIO_E_TOKEN_9876, Hide All, Tray Exit
   -> Exit -> restart       328 ms later; disk + UI = SCENARIO_E_TOKEN_9876
F  X delete -> Show All PASS deleted note gone from disk and stayed gone
                           across Hide All + Show All; 3 notes left, no
                           resurrection
G  delete all -> tray   PASS 0 notes and 0 JSON files, process and tray alive,
   -> New Note             menu intact, New Note created 1a0d887e9b0-0
Hidden exit flush      PASS  FINAL_HIDDEN_TOKEN typed, Hide All, Tray Exit:
                           369 ms; disk + UI = FINAL_HIDDEN_TOKEN
220x160                PASS  forced to the floor: inner size 220x160, typing
                           works, `+` and Pin are clickable, a real todo
                           checkbox click flips it to checked, the editor
                           scrolls vertically, and the toolbar is 29 px tall
                           (131 px of 160 left for the note)
```

Phase 4 (Harness Protocol), run against a live build with the push endpoint up.
The demo payloads were sent from temporary scripts outside the repo and were
never added to the project:

```
A  Empty registry      PASS  on startup: harnesses 0, active_tasks 0, and the
                             note window and tray were unaffected
B  First push          PASS  POST Harness A with task-1 running -> 1 harness,
                             1 active task, readable back through
                             list_active_harness_tasks
C  Update same harness PASS  re-POST harness-a with task-1 completed and
                             task-2 running -> still 1 harness (no duplicate),
                             active list holds only task-2
D  Two harnesses       PASS  harness-a + harness-b -> 2 harnesses, 2 active
                             tasks, both present with their own names
E  Invalid payload     PASS  9 payloads rejected: missing harness_id, unknown
                             status, invalid timestamp, empty name, updated_at
                             before started_at, empty title, duplicate task_id,
                             200-char harness_id and broken JSON - all HTTP 400
                             with a readable reason, app alive, registry still
                             reporting the same 2 valid harnesses
F  Restart             PASS  a pushed harness is gone after a restart:
                             harnesses 0, snapshots []. Harness runtime state is
                             ephemeral
G  Notes regression    PASS  with the API up: tray New Note, `+`, Markdown
                             heading/bold/code, a real todo checkbox click
                             round-tripping through Markdown, Pin, Hide All /
                             Show All, and Tray Exit (345 ms, notes intact)
   limits              PASS  300 KB body -> HTTP 413; 300 tasks -> HTTP 400;
                             GET /nope -> 404; POST /health and
                             DELETE /api/harness/snapshot -> 405
   bind failure        PASS  with 127.0.0.1:17899 held by another process the
                             app logged the failure and started anyway: note
                             window and all 8 tray items worked normally
   exposure            PASS  netstat shows exactly one LISTENING socket on
                             127.0.0.1:17899; the machine's LAN address
                             (192.168.0.104) could not reach it
```

Phase 5 (Harness Task Note), run against a live build. Harness state was pushed
from temporary scripts outside the repo; the stale run used a 4-second timeout
through the compile-time override so a real stale window could be observed
without waiting five minutes. That override was removed from the launcher
afterwards, so the shipping default is still 5 minutes:

```
Tray menu              PASS  9 rows: New Note / Harness Tasks / separator /
                             Show All Notes / Hide All Notes / separator /
                             Start with Windows / separator / Exit
A  Empty registry      PASS  with nothing reported, Tray -> Harness Tasks created
                             one window (label harness-tasks, 378x467) showing
                             the heading and "No running tasks"; no window was
                             created before the click
B  One running task    PASS  POST Harness A / Task A / running appeared within
                             1.5 s as "Harness A -> Task A -> Running", elapsed
                             08:19 computed from started_at
C  Re-push same id     PASS  task-a completed + task-b running arrived as one
                             harness with only Task B listed; no completed
                             history, still a single harness
D  Waiting + message   PASS  a waiting task rendered as "Waiting" with its
                             message shown at low visual weight
E  Two harnesses       PASS  Harness A (2 tasks) and Harness B (1 task) grouped
                             separately, tasks oldest-first, order stable
                             across refreshes
F  Stale exclusion     PASS  with a 4 s timeout: a running task passed the
                             timeout and left the window ("No running tasks")
                             while GET /api/harness/snapshots still returned it
                             byte for byte; a fresh POST brought it back. The
                             snapshot was never deleted or mutated
G  X then reopen       PASS  X hid the window (same HWND, still present), the
                             harness API stayed up (GET /health ok) and the
                             registry was intact; Tray -> Harness Tasks restored
                             the same window with its tasks listed, and no
                             second harness-tasks label ever appeared
H  Restart             PASS  after a restart the window returned at its saved
                             375,250 557x603 with Pin still on, the registry was
                             empty ("No running tasks"), and a new POST showed
                             the task again in 2.6 s
I  Show / Hide All     PASS  Harness Task Note + 2 notes: Hide All returned 3 and
                             left 0 visible windows with the tray alive and
                             GET /health still reporting 3 active tasks; Show
                             All returned all 3 windows with note files and
                             registry unchanged
J  Note regression     PASS  tray New Note, `+`, Markdown heading and
                             lists round-tripping to disk, a real todo checkbox
                             click flipping and persisting, Pin persisting,
                             resize persisting (525x475), Hide All / Show All,
                             and Tray Exit quitting cleanly
   listener permission PASS  before the capability fix the harness window logged
                             "event.listen not allowed on window harness-tasks";
                             after granting core:event:allow-listen the console
                             is clean and the focus refresh works
```

Re-run for Phase 6, which adds the adapters:

```
npm test                  PASS  85 tests (5 files)
npm run typecheck         PASS
npm run build             PASS  (same pre-existing chunk-size warning)
cargo test                PASS  95 tests (validation, registry, staleness,
                                adapters, manager config)
cargo check --all-targets PASS  (no warnings)
cargo build               PASS
```

Phase 6 (Harness Adapters), run against a live build. Four adapters were
configured for the run - two local JSON, one local HTTP against a temporary
loopback bridge, and one quiet source - plus a disabled adapter that must never
run. Fixtures and the bridge lived outside the repo in %TEMP%\sh-verify; the
shipping `harness-adapters.json` is `{ "adapters": [] }`:

```
A  Startup            PASS  4 start lines for 4 enabled adapters and no start
                            line for the disabled one; GET /health reported
                            harnesses: 4 with no POST from any of them
B  Stale, quiet prod  PASS  after > 300 s with every document unchanged,
                            list_live_active = ["http-harness"] while
                            list_active still held all 4: the three quiet
                            pull adapters aged out of the live view. No
                            snapshot was deleted - all 4 were still in the
                            registry, and the note showed only the live task
C  New content        PASS  rewriting one JSON document brought that harness
                            back into the live view within 3 s with its new
                            task, without a restart and without a POST
D  HTTP 500           PASS  the bridge returning 500 logged
                            "adapter http-bridge poll failed: ... HTTP 500"
                            and only that adapter; the other three kept
                            updating on their own ticks
E  HTTP recovery      PASS  the bridge returning 200 again and the adapter
                            recovered on its next tick with no restart
F  HTTP timeout       PASS  a bridge that accepted the connection and never
                            answered hit the 800 ms adapter timeout
                            (os error 10060), was logged, and left every other
                            adapter untouched
G  JSON file removed  PASS  deleting a source file produced repeated read
                            failures for that adapter only, and its last good
                            snapshot stayed in the registry
H  JSON recovery      PASS  restoring the file replaced the snapshot
                            automatically on the next tick
I  Restart refill     PASS  a restart with no push at all left the registry
                            empty and the adapters refilled it on their first
                            ticks
J  Regression         PASS  with adapters running: a new note saved Markdown to
                            disk, the Harness Task Note stayed live, Hide All
                            returned 3 and left 0 visible with the tray alive
                            and the adapters still polling, Show All returned
                            all 3, and Tray Exit quit in 3.85 s with no
                            adapter log line afterwards and both note files
                            intact (Phase 7 later measured this as Windows/
                            WebView2 teardown, not adapter cost - see Known
                            Issues)
K  Shipping state     PASS  with the fixtures removed and
                            `harness-adapters.json` back to
                            `{ "adapters": [] }`, startup logs "no harness
                            adapters configured" and health is
                            {"active_tasks":0,"harnesses":0,"status":"ok"}
```

```
Codex integration mode: Bridge
DeepSeek integration mode: Bridge
```

Re-run for Phase 7, which adds the Adapter Management window, its commands and
the per-adapter status surface:

```
npm test                  PASS  85 tests (5 files)
npm run typecheck         PASS
npm run build             PASS  (same pre-existing chunk-size warning)
cargo test                PASS  114 tests (validation, registry, staleness,
                                adapters, manager config, adapter window)
cargo check --all-targets PASS  (no warnings)
cargo build               PASS
```

Phase 7 (Adapter Management), run against a live build through the real UI over
the WebView2 CDP endpoint plus real tray clicks. Fixtures and the temporary
loopback bridge lived outside the repo in %TEMP%\sh-verify; the shipping
`harness-adapters.json` was restored to `{ "adapters": [] }\n` afterwards:

```
1  Add Local JSON      PASS  added through the window; status went ok and the
                            harness appeared in the Harness Tasks window
2  Disable             PASS  status off and the row stayed; the manager
                            reported running: 0, so it is a real stop and not a
                            filter over a still-polling adapter
3  Enable              PASS  recovered to ok on its first new poll, no restart
4  Edit path           PASS  one row after the edit (no duplicate), and the new
                            harness id was picked up from the new file
5  Add Local HTTP      PASS  source shown as
                            http://127.0.0.1:18001/api/harness/snapshot
6  Invalid configs     PASS  refused with a message and the file untouched:
                            "adapter 2: local-http needs a port",
                            "http_path must start with '/'", and a duplicate
                            name; the duplicate case initially overwrote the
                            existing adapter and was fixed to refuse instead
7  Delete              PASS  row gone and the file updated
8  Restart             PASS  adapters came back from the file with no UI help
9  One adapter fails   PASS  an HTTP 500 made that row error with the
                            producer's detail while the other stayed ok; both
                            last-good snapshots stayed in the registry, and the
                            failing one recovered to ok without a restart
10 Reload              PASS  a hand-edited file picked up a new adapter and a
                            disabled one without a restart
11 Regression          PASS  with adapters running: a normal note saved its
                            Markdown to disk, the Harness Task Note stayed live,
                            Hide All returned 3 and left 0 visible with the tray
                            alive, Show All returned all 3, and Tray Exit quit
                            with both note files intact
```

Exit latency (Phase 7 investigation; the Phase 6 note of 3.85 s was this
teardown, not an adapter cost):

```
Rust exit sequence     PASS  exit_app -> flush done -> app.exit(0) ->
                            ExitRequested -> stop_adapters -> Exit all completed
                            in ~34 ms after the tray click
Truly-gone timing      PASS  0 adapters ~2.0 s; 3 adapters ~1.9-2.0 s; an HTTP
                            adapter blocked mid-request ~2.1 s
Per-window teardown    PASS  notes only ~1.9-2.1 s; adding any second window
                            (adapter window or Harness Tasks) ~3.2-3.6 s, i.e.
                            ~1 s per WebView2 window
app.run() return       PASS  never printed while the process still ended with
                            exit code 0, so the delay is Windows/Tauri/WebView2
                            teardown after RunEvent::Exit
Decision               N/A   accepted as environmental; the Exit path and
                            EXIT_FLUSH_TIMEOUT = 1500 ms are unchanged
```

Re-run for Phase 8, which adds the bridge:

```
npm test                  PASS  127 tests (6 files)
npm run typecheck         PASS
npm run build             PASS  (same pre-existing chunk-size warning)
cargo test                PASS  114 tests (validation, registry, staleness,
                                adapters, manager config, adapter window)
cargo check --all-targets PASS  (no warnings)
cargo build               PASS
```

`npm test` covers `src/harness/bridge.test.ts` (42 tests) alongside the
Markdown, todo, link, elapsed and grouping suites. The bridge module is pure -
no I/O, no clock read, no `process.argv` - which is what lets the argument
parser, the session transitions and the snapshot construction all be tested
directly; the CLI file is the only part that touches the disk or the network,
and it does as little as possible.

Phase 8 (Harness Bridge), run against a live build through the real CLI, the
WebView2 CDP endpoint and real tray clicks. Every bridge run used `--state-dir`
under `%TEMP%\sh-verify`, fixtures and the loopback producer lived outside the
repo, and the shipping `harness-adapters.json` was restored to
`{ "adapters": [] }` afterwards:

```
1  start -> window    PASS  one row appeared: Codex / Refactor runtime /
                            Waiting / 00:12 / Running tests
2  update             PASS  --message and --title both changed the row in place
3  running <-> wait   PASS  both directions, same row, same id
4  done               PASS  live count 0 and the snapshot kept as completed
5  fail / cancel      PASS  reported failed and cancelled; both left the live
                            view while their snapshots stayed in the registry
6  Two harnesses      PASS  codex (running) and deepseek (waiting) at once,
                            liveCount 2, each with its own stable task id
7  App not running    PASS  exit 1 with "nothing is listening on
                            127.0.0.1:17899; start Sticky Harness first"; no
                            crash, no session file written
8  Stable id          PASS  the id stayed refactor-runtime across every update;
                            one harness stayed one row, never two
9  No current task    PASS  update and done after a finish both answered
                            "no current task for ..." with exit 1
10 Replace warning    PASS  start over a live task warned on stderr and
                            replaced it
11 Restart recovery   PASS  after restarting the app the registry was empty
                            (it is in-memory by design); running start again
                            restored the row
12 PS example         PASS  examples/run-task.ps1 -Work { ... } reported the
                            start, ran the work and reported completed
13 Bad input          PASS  an unreachable port, a malformed --endpoint, a
                            timeout from a socket that never answers (clear
                            error, exit 1), an unknown status and a --status on
                            done all failed cleanly with one readable line
14 --dry-run          PASS  printed the snapshot, sent nothing, left the state
                            dir untouched
15 HTTP adapter       PASS  added through the Adapter Management window
                            alongside the bridge; both sources showed in Harness
                            Tasks at the same time
16 Disable / enable   PASS  disabling stopped updates and enabling resumed them
                            with no restart
17 Isolation          PASS  killing the HTTP producer made only that row error
                            ("connection timed out") while the JSON adapter
                            stayed ok; restarting the producer recovered it
18 Invalid config     PASS  an empty path was refused with "adapter 2: path must
                            not be empty", the file on disk was untouched, the
                            other adapters kept running and the app did not
                            crash
19 Restart refill     PASS  both adapters refilled the registry on their first
                            ticks after a restart
20 Regression         PASS  with the bridge and an adapter running: a normal
                            note saved its Markdown to disk, the Harness Task
                            Note stayed live, Hide All left every window hidden
                            with the tray alive, Show All restored them, and
                            Tray Exit quit with the note files intact
21 Exit timing        PASS  one window with a JSON adapter and bridge snapshots
                            present: gone in 2039 ms; with the adapter window
                            and the Harness Task Note also open: 3843 ms and
                            3905 ms - i.e. the documented ~2 s teardown plus
                            ~1 s per extra WebView2 window, so the bridge adds
                            nothing to Exit
22 Shipping state     PASS  fixtures removed, harness-adapters.json back to
                            { "adapters": [] }, notes directory back to the
                            user's own note
```

```
Codex integration mode: Bridge
DeepSeek integration mode: Bridge
```

A `typecheck` failure that `vitest` could not catch was found during this
phase's quality gate: one test narrowed `parseArgs`'s `BridgeOptions | string`
return without checking, which `tsc` rejected even though the test passed. The
test now goes through small `parsed` / `parsedStart` helpers that assert the
shape instead of assuming it. `vitest` transpiles without type-checking, so
`npm run typecheck` is the gate that catches this class of error.

Re-run for Phase 9, after the release configuration, the bundled bridge and the
panic hook:

```
npm test                  PASS  127 tests (6 files)
npm run typecheck         PASS
npm run build             PASS  (same pre-existing chunk-size warning)
cargo test                PASS  117 tests (validation, registry, staleness,
                                adapters, manager config, adapter window,
                                harness window config, panic log, data dir name)
cargo check --all-targets PASS  (no warnings)
cargo build               PASS
npm run tauri build       PASS  MSI: src-tauri/target/release/bundle/msi/
                                Sticky Harness_0.1.0_x64_en-US.msi  (the 0.1.0
                                artifact; Phase 10 rebuilt it as 1.0.0)
                                (2,703,360 bytes, ~2.6 MB);
                                sticky-harness.exe = 5,396,992 bytes
```

Phase 9 (Release / Packaging), run against the real MSI installed per user into
`%LOCALAPPDATA%\Programs\Sticky Harness`, with a live data directory present
throughout. Installing, upgrading and uninstalling were real `msiexec` runs; the
UI steps were driven through the WebView2 CDP endpoint and real Win32 window
messages, and helper scripts lived outside the repo in `%TEMP%\sh-verify`:

```
 1 Build              PASS  npm run tauri build produced the MSI above, with the
                            exe and the bridge resources in the bundle
 2 Install (fresh)    PASS  msiexec /qn installs per user with no elevation;
                            program folder, Start Menu + desktop shortcuts and
                            the uninstall entry all appear
 3 Bundled bridge     PASS  node "<install>\bin\sticky-harness-bridge.mjs" ran
                            from the install dir; start/update/done reported a
                            task that appeared in Harness Tasks
 4 First run empty    PASS  with no prior data: one blank note, Harness Tasks
                            "No running tasks", Harness Adapters empty, no
                            sample data and no development path anywhere
 5 Smoke: note        PASS  New Note (tray and in-note +), a Markdown heading and
                            a - [ ] todo rendered and persisted, Pin toggled both
                            ways through the real button and written to disk,
                            geometry round-tripped, and closing a note window
                            removed its file
 6 Smoke: harness     PASS  a Local JSON adapter added through the window showed
                            its task, and bridge + adapter showed together
 7 Hide / Show All    PASS  every window IsWindowVisible=False with the tray
                            alive; Show All restored all of them
 8 Start with Windows PASS  ON wrote the Run entry and OFF removed it; left OFF
 9 Tray Exit          PASS  measured from the Exit click to the process being
                            gone: 1,370 ms with two windows, 3,415 ms with a
                            third window open - the documented ~2 s teardown plus
                            ~1 s per window, unchanged by the adapters; notes kept
10 Upgrade            PASS  a new MSI installed over the old one: new exe in
                            place, note, adapters and window state unchanged
11 Uninstall          PASS  program folder, both shortcuts and the registry
                            entry removed; %APPDATA%\com.stickyharness.desktop
                            byte-identical afterwards (sha256 per file);
                            autostart not resurrected
12 Reinstall          PASS  installed again: correct layout, the note and its
                            content restored, and the saved adapter configuration
                            read back (its fixture task reappeared)
13 Repo cleanup      PASS  73 tracked files, no fixtures, CDP/UIA helpers,
                            logs, snapshots or secrets; no absolute dev paths
```

One limit is recorded rather than papered over: two `0xc0000409` hard exits were
observed during this phase and could not be reproduced in 22+ controlled attempts
afterwards. See Known Issues - the panic hook exists so a recurrence leaves a
readable location.

Re-run for Phase 10, after unifying the version at 1.0.0. The source is unchanged
from Phase 9 apart from the five version fields, so the counts are the same:

```
npm test                  PASS  127 tests (6 files)
npm run typecheck         PASS
npm run build             PASS  (same pre-existing chunk-size warning)
cargo test                PASS  117 tests
cargo check --all-targets PASS  (no warnings)
cargo build               PASS
npm run tauri build       PASS  MSI: src-tauri/target/release/bundle/msi/
                                Sticky Harness_1.0.0_x64_en-US.msi
                                (2,703,360 bytes, ~2.6 MB);
                                sticky-harness.exe = 5,396,992 bytes
```

Phase 10 (Final QA / v1.0), run on the MSI installed per user into
`%LOCALAPPDATA%\Programs\Sticky Harness`, with a real data directory present
throughout and, for the upgrade leg, the previous 0.1.0 build already installed.
Installing and upgrading were real `msiexec` runs; the UI steps were driven
through the WebView2 CDP endpoint and real Win32 window messages, and helper
scripts lived outside the repo in `%TEMP%\sh-verify`:

```
 1 Version sources   PASS  tauri.conf.json, Cargo.toml, the sticky-harness entry
                            in Cargo.lock, package.json and both package-lock
                            entries all read 1.0.0; identifier and AppData path
                            unchanged; only unrelated crates still say 0.1.0
 2 Upgrade 0.1->1.0  PASS  msiexec exit 0 over the installed 0.1.0 build; one
                            uninstall entry now at 1.0.0; new exe in place; every
                            file under %APPDATA%\com.stickyharness.desktop
                            sha256-identical before and after
 3 First run / notes PASS  New Note from the tray and from the in-note +; a
                            Markdown heading and a - [ ] todo stored and rendered;
                            Pin toggled both ways and read back from disk;
                            geometry written and restored; close deletes the note
 4 Harness Tasks     PASS  empty on fresh data; live rows for every source
 5 Adapters          PASS  a Local JSON and a Local HTTP adapter live together,
                            both ok, both tasks visible at once
 6 Bridge            PASS  the installed bridge drove Codex and DeepSeek through
                            start / update / running<->waiting / done / fail /
                            cancel; four sources visible simultaneously
 7 Bad input         PASS  an unknown option exited non-zero with one readable
                            line; a refused adapter edit (empty path, duplicate
                            name) changed nothing and kept the app running
 8 Corrupt config    PASS  a malformed harness-adapters.json was reported with
                            its parse reason, saved nothing, and the app stayed
                            up with no panic.log
 9 Hide / Show All   PASS  every window IsWindowVisible=False with the tray
                            alive; Show All restored all of them
10 Start with Win    PASS  ON wrote the Run entry and OFF removed it; left OFF
11 Restart persist   PASS  notes, content, geometry and adapters all restored
12 Tray Exit         PASS  exit_app -> process gone in 20-76 ms across runs; the
                            remaining seconds are Windows/WebView2 teardown
13 Hard-exit stress  PASS  not reproduced: 4 launch/churn/exit cycles (8 pushes
                            each), 12 create/close cycles and 30 snapshot POSTs;
                            no panic.log; WER count stayed at 3
14 Repo cleanliness  PASS  73 tracked files, no fixtures, CDP/UIA helpers, logs,
                            scratch scripts or secrets; MSI file table holds only
                            the exe, its dll, the bridge and its one module
```

## 8. Known Issues

- **Capabilities the frontend needs must be granted explicitly.**
  `core:window:allow-destroy` is what lets a closing note window actually
  finish closing; without it the X button deleted the JSON and left the window
  on screen. `core:window:allow-set-size` is granted for the same reason, so
  the note can be resized to its documented 220x160 floor. Phase 5 hit the same
  trap from the other side: the default capability only listed `note-*`, so
  `event.listen` was denied for `harness-tasks` and the whole poll effect
  aborted before it ever loaded a task. The window looked permanently empty
  while the registry held the data. `core:window:default` contains neither
  permission, and a new window label must be added to the capability before any
  of its APIs work. Phase 7 is the third time: `harness-adapters` had to be
  added to `windows` or the window's commands would have been denied.
- **An `os error 32` read race was observed once during runtime verification,**
  but repeated `create -> immediate get_note` checks could not reproduce it.
  Fifteen serial runs (6 + 10) of create-then-read all succeeded. Persistence
  was deliberately left unchanged rather than restructured around a race that
  no longer reproduces.
- **`fs::rename` can be refused on this machine.** Inside the AppData tree the
  rename returns `ERROR_NOT_SAME_DEVICE` (`os error 17`) even though both files
  are in the same directory, so `write_record` falls back to copying over the
  destination and logs a warning. This is the same environment quirk that
  breaks the NSIS bundler. Saves are correct either way; the atomic rename path
  is simply unavailable here and will be used normally elsewhere.
- **`npm run tauri build` cannot finish the NSIS bundle** with
  `failed to bundle project: 系统无法将文件移到不同的磁盘驱动器。 (os error 17)`
  while extracting its own toolchain. Verified environmental, not a project
  defect: a pristine `create-tauri-app` scaffold fails identically on this
  machine. `--bundles msi` succeeds.
- **This machine MSVC is not registered with `vswhere`**, so plain
  `cargo build` fails with a missing `link.exe`. Call
  `F:\software\VisualStudio\VC\Auxiliary\Build\vcvars64.bat` first. Environment
  quirk, not a project defect.
- **The window-geometry debounce is last-write-wins.** Two rapid moves can let a
  slightly older read-modify-write win, which is invisible in practice because
  the next move rewrites it. Revisit only if geometry drift is ever reported.
- **The harness registry is intentionally not persisted and not pruned.** State
  is dropped on exit, so the Harness Task Note opens empty after a restart until
  a harness reports again. Staleness now hides a task from the note, but the
  snapshot is never deleted or mutated: the decision what to *do* with a stale
  harness beyond hiding it (prune, mark it, ask it) still needs a task of its
  own.
- **The stale timeout can be overridden at compile time.** `protocol.rs` reads
  `STICKY_HARNESS_STALE_TIMEOUT_MS` with `option_env!` and falls back to the
  5-minute default, so a real stale window can be observed in a few seconds
  during verification without changing shipping behaviour. It defaults to the
  documented value when unset, which is the only configuration the app ships.
- **The Harness Task Note refreshes on a timer, not on an event.** Hidden
  windows keep polling once a second, which costs almost nothing and keeps the
  code simple, and picking the window up refreshes it immediately. A hidden
  window can therefore be up to one second behind when it reappears.
- **The task list shows whatever the registry has, with no history.** A task
  that completes simply disappears on the next poll; there is no completed list
  and no notification, which is deliberate for a status surface.
- **Elapsed time is computed from `started_at` in the UI.** A producer that
  reports a wrong or future `started_at` will show a wrong duration; negative
  values are clamped to `00:00`. This is the one place a producer clock is
  still trusted, and it is cosmetic only: staleness and the live view no
  longer read producer time at all.
- **The push endpoint has no authentication.** It is bound to loopback only and
  sends no CORS header, so a local process can post harness state and a web page
  cannot read it cross-origin. That is the intended boundary for local IPC; it
  is not a security model for untrusted local code.
- **`HARNESS_API_PORT` is a fixed constant (17899).** If it is taken the app
  logs the failure and runs without the harness API. There is no fallback port
  and no UI to change it: the Adapter Management window edits adapters, not the
  app's own push listener, and the port is a constant by design.
- **The HTTP parser is deliberately minimal.** It handles a single request per
  connection with `Connection: close` and reads only `Content-Length`. A
  `Transfer-Encoding: chunked` body is not supported, because a local snapshot
  upload does not need it. Revisit only if a real producer sends chunked.
- Automated UI verification used temporary CDP / UI-Automation helper scripts
  that live in %TEMP%\sh-verify and are not part of the repo; re-create them
  if runtime behaviour must be re-tested.

- **Staleness is now "last new evidence", so a producer that reports the same
  content for a long time goes stale even while it is still reachable.** That is
  the intended rule: a re-read of an unchanged document says nothing about
  whether the work is alive, and depending on the producer's clock to say it
  would put a clock outside the app in charge of the app's own liveness. The
  cost is that an adapter whose producer reports only coarse changes - say, one
  status write per hour - will be hidden between writes. The fix belongs on the
  producing side (bump `updated_at` in the document) or in a future
  per-snapshot heartbeat field, not in loosening the rule.
- **Push still counts as a heartbeat and Pull no longer does.** A repeated
  `POST` of byte-identical content refreshes the snapshot, because receiving a
  report is itself the event; a repeated *read* of byte-identical content does
  not. This asymmetry is deliberate and is the only behavioural difference left
  between the two paths.
- **`differs_from` compares content, not arrival.** It looks at
  `harness_id`, `harness_name`, `updated_at` and the task list, and
  deliberately ignores `source`. A producer that rewrites the same JSON with a
  new `updated_at` every tick therefore stays permanently live; only the
  producer can be blamed for that, and a future phase may want a monotonic
  producer sequence number instead.
- **Adapter polling is a fixed interval per adapter, with no backoff.** A
  failing adapter retries every `poll_interval_millis` (5 s by default,
  250 ms floor), which is fine for local files and loopback HTTP and would be
  wrong for anything remote. Remote is out of scope by design.
- **`local-http` accepts only a `Content-Length` body.** A chunked response is
  not supported, for the same reason the push endpoint does not parse chunked
  requests: a local snapshot does not need it.
- **`harness-adapters.json` is still the contract, and the window is another
  writer of it.** An unknown key, a duplicate name, a bad kind or an
  out-of-range value is logged and the whole file is rejected at startup, so the
  app starts with no adapters rather than a partial set. The Adapter Management
  window cannot produce such a file: every edit re-validates the whole resulting
  configuration in Rust before writing, and a rejected edit leaves the previous
  file on disk. The staleness of this line at startup is still the file being
  hand-edited outside the app - use Reload, or restart, to pick that up.
- **Adapter status is in-memory and last-write-only.** `AdapterStatus` keeps one
  outcome per adapter and no history, and it is rebuilt empty on every
  configuration change, so a reload or an edit shows `waiting` until the first
  new poll. It is deliberately not a log: adding per-poll history would turn a
  settings window into the task-log surface this project keeps refusing.
- **A configuration change stops and restarts every adapter.** Add, edit,
  delete, enable, disable and reload all replace the whole running set through
  `apply_adapter_config`, even when only one adapter changed. That is what keeps
  one code path for validation and isolation; the cost is one dropped poll for
  the untouched adapters, and their last-good snapshots stay in the registry.
- **An adapter can only ever add harness state, never inspect the app.** It
  writes into `HarnessRegistry` through the same validated `HarnessSnapshot`
  contract as the push endpoint, so an adapter cannot create a note, touch
  `notes/`, or send anything anywhere. `local-json` never executes what it
  reads, and `local-http` only ever connects to `127.0.0.1`.
- **Adapter workers sleep in 50 ms slices to make Exit quick.** That is the
  only thing keeping a 5-minute poll interval from delaying the tray; it costs
  a wakeup every 50 ms per adapter, which is irrelevant at this scale but should
  be revisited if adapters ever became numerous.
- **Tray Exit spends about 2 s in Windows/Tauri/WebView2 teardown, and that is
  environmental, not this app's exit path.** Measured with
  `GetExitCodeProcess` polling rather than window disappearance, the whole Rust
  exit sequence - `exit_app`, the note flush, `app.exit(0)`, `ExitRequested`,
  `stop_adapters`, `Exit` - finishes about 34 ms after the tray click.
  Time-to-gone is then ~2.0 s with no adapters, ~1.9-2.0 s with three, and
  ~2.1 s with an HTTP adapter blocked mid-request, so adapters do not contribute.
  A second window costs about another second (~3.2-3.6 s with the adapter window
  or the Harness Task Note open), which is where the Phase 6 figure of 3.85 s
  came from. A line placed after `app.run()` never printed while the process
  still ended with exit code 0, so `app.run()` does not return normally: the
  process is terminated during teardown, after `RunEvent::Exit`. A deliberately
  minimal Tauri 2 app with one window behaves the same on this machine.
- **`EXIT_FLUSH_TIMEOUT` stays at 1500 ms.** The exit path was deliberately left
  unchanged: 1.5 s is the budget for the last unsaved keystroke, and trading it
  for a smaller teardown number would risk losing input to save time the app
  does not control. Do not shorten it to chase the teardown.
- **A bridge task stays visible for about five minutes after its last report.**
  Push counts as a heartbeat simply because it arrived, so a repeated identical
  `POST` still refreshes `received_at`. A `start` with no later call therefore
  ages out on the normal stale timeout rather than instantly, and a long task
  must call `update` periodically to stay honestly live. This is the existing
  push rule, not a bridge defect: the bridge is a push producer like any other,
  and the fix for a producer that cannot speak often belongs in the protocol (a
  per-snapshot heartbeat) rather than in one client. Do not special-case the
  bridge in the registry.
- **The bridge keeps one session per state directory.** `update`, `done`,
  `fail` and `cancel` act on whatever `start` last wrote, so two tasks run from
  the same state directory would overwrite each other's session. Running two
  harnesses at once means passing a different `--state-dir` (or
  `STICKY_HARNESS_BRIDGE_DIR`) per task - which is exactly what the two-harness
  verification did. Per-harness sessions keyed by name would be a reasonable
  later change; nothing today depends on one directory holding more than one.
- **A bridge session file that does not parse is treated as absent.** The bridge
  will not reconstruct a task id from a half-written or hand-edited session, so
  the answer is "no current task, run start" rather than a guess that could fork
  a second row. If the app restarted, the registry is empty anyway and `start`
  is the correct next call.
- **The bridge is a Node script, so it needs Node 20 or newer** (developed on
  24, which is what the `bin/` CLI's direct `.ts` import relies on for type
  stripping). This is a deliberate trade: no second toolchain or build artifact
  for a reporter, at the cost of a bridge on a machine that has the repo but not
  Node. The app itself is unaffected either way.
- **The bridge writes to the user profile, not to `<AppData>`.**
  `\.sticky-harness\bridge-session.json` belongs to the bridge rather than the
  app, keeps only five small fields, and is deleted when a task finishes. It
  holds no logs, no prompts, no conversation and no code, and `--state-dir`
  moves it anywhere.
- **`--timeout` is a hung-app guard, not a slow-app budget.** It defaults to
  2 s and destroys the request when it fires, so a snapshot is either accepted
  or fails with a readable line; there is no retry and no queue behind it. A
  harness that must not lose a report should call the bridge again, which is the
  same idempotent `harness_id` replacement any producer gets.
- **A rare hard exit (`0xc0000409`) is documented but not explained.** Windows
  Error Reporting recorded three `BEX64` events with `ExceptionData 7`
  (`FAST_FAIL_FATAL_APP_EXIT`) on 2026-09-27, at 13:42, 14:05 and 14:38. All three
  fault in `sticky-harness.exe` at the same offset `0x741a5`, and all three carry
  the same module timestamp, `0x6ab8aba1` - a release build from about 13:37,
  made *before* the panic hook existed. The two later builds (`0x6ab8c0ec`, 15:08,
  and `0x6ab8dc64`, 17:05, which is the 1.0.0 binary) have each now been exercised
  without a single new event.
  `FAST_FAIL_FATAL_APP_EXIT` is the abort a `panic = "abort"` release performs,
  and no panic message or dump was ever captured for it. Across two phases it has
  resisted 22+ controlled reproductions in Phase 9 (30 rapid tray New Notes,
  adapter-window/new-note sequences, four first-run launches on fresh data, a
  four-minute idle with 47 notes, 24 create/close cycles, a port-conflict second
  instance) and, in Phase 10, four launch -> 8-push harness churn -> exit cycles on
  the 1.0.0 build, 12 more create/close cycles and 30 snapshot POSTs - all clean,
  with the WER count flat at three. It is neither fixed nor explained, and it is
  not claimed to be: `lib.rs` installs a panic hook that writes
  `<AppData>/panic.log` (timestamp, location, message) before the abort, so a
  recurrence leaves a source location instead of a guess - read that file first.
  `panic = "abort"` is deliberately unchanged, because unwinding across the FFI
  boundary to Tauri/WebView2 is undefined behaviour.
- **The MSI's ProductCode changes on every build.** Upgrading is "install the new
  MSI over the old one", which the MSI's shared `UpgradeCode` and higher version
  allow, and it was verified against live data. Uninstalling by script needs the
  *current* ProductCode, though, because its key is that per-build GUID - read it
  from the uninstall registry key rather than from a hardcoded value. User data is
  never part of either operation.
- **NSIS is deliberately not built.** `bundle.targets` is `["msi"]`: the NSIS
  bundler cannot extract its own toolchain on this machine (`os error 17`, the
  same environment quirk as the `fs::rename` failure above), which a pristine
  `create-tauri-app` scaffold reproduces. A machine without that quirk can build
  NSIS with `--bundles nsis` and no code change.
- **The MSI is unsigned.** No code-signing certificate is wired into the build, so
  Windows may warn on first run and the publisher reads as unknown. Signing is a
  distribution decision with its own key management and was out of scope here.
- **The installed bridge needs Node on `PATH`.** The MSI bundles the bridge script
  and the `.ts` module it imports, but not a Node runtime, so the installed
  `sticky-harness-bridge` needs Node 23.6+ (Node 22.6-23.5 with
  `--experimental-strip-types`) just as the repository copy does. The app itself
  does not use Node at all.

## 9. Next Step

Maintenance / future improvements

v1.0.0 is done: the version is unified, the installer is built and installed, the
whole product was re-run on it, and the upgrade from 0.1.0 was verified against
live data. See 4a for what shipped and §7 for the run-through. There is no
Phase 11; the product surface is frozen and what follows is maintenance.

Known work that is *not* scheduled, in rough priority order:

- **Sign the MSI and add an update story.** The release is unsigned and has no
  auto-update. Both need decisions this project cannot make on its own - a
  certificate and its key handling, and a feed plus a threat model - so they are
  deliberately not improvised here.
- **Watch for a recurrence of `0xc0000409`.** It is documented, not explained
  (Known Issues). If it appears again, read `<AppData>/panic.log` first; the hook
  exists so the next occurrence can be located rather than guessed at. Do not
  "fix" it on the strength of a failed reproduction.
- **Product polish carried over from the early phases:** images, themes, search and
  a settings window. The adapter window and the bridge's `--state-dir` default are
  surfaces a settings window should eventually absorb rather than sit beside.
- **The per-producer heartbeat question.** Staleness is deliberately "last new
  evidence", so a producer that reports rarely can be hidden between reports. The
  honest fix is a producer-side `updated_at` bump or a protocol heartbeat field,
  not loosening the rule; see Known Issues.
- **A macOS/Linux port.** The product decisions ask for a non-Windows-only
  architecture, and the seams (paths, tray, autostart, window lifecycle) are
  isolated enough to make that a real option later. Nothing here is scheduled.

Still out of scope, and still worth refusing:

- No task history, log or tool-call viewer, no terminal, no notifications, no
  SQLite, no cloud, no authentication, no WebSocket or SSE.
- No remote network. `local-http` means loopback, and the bridge posts to
  loopback, and both should keep meaning that.
- No Codex or DeepSeek control, no prompt sending, and no parsing of a
  harness's private state. The app reports; it does not drive or snoop.
- No second bridge, no SDK and no per-vendor adapter without a stable
  documented source.
## 10. Latest Commit

Remote: `https://github.com/1148514800/sticky-harness` (private, default branch
`main`). Local commits only; nothing has been pushed.

```
2c8c872 feat: edit notes as markdown with todos         (Phase 2)
c1c1f85 docs: record Phase 2 commit in handoff
8d83a70 fix: complete markdown note runtime behavior    (Phase 2 closeout)
c2ef7e7 feat: improve desktop note experience           (Phase 3)
06dbf5b docs: record the Phase 3 commit in the handoff
eeb5353 feat: add local harness protocol                (Phase 4)
cd92869 docs: record the Phase 4 commit in the handoff
7fa106b feat: add harness task note                     (Phase 5)
b70e5a6 docs: record the Phase 5 commit in the handoff
756386e feat: add harness adapters                      (Phase 6)
393aef5 docs: record the Phase 6 commit in the handoff
f607205 docs: record the Phase 6 docs commit hash
8fc785c feat: add adapter management              (Phase 7)
d815c38 feat: add harness bridge                     (Phase 8)
85bde2c chore: prepare desktop release             (Phase 9)
0b29754 chore: prepare v1.0 release               (Phase 10, v1.0.0)
```

Every commit is local. Nothing has been pushed. Phase 8 adds the harness bridge:
`src/harness/bridge.ts` (the pure argument parser, session transitions and
snapshot construction), `src/harness/bridge.test.ts` (42 tests),
`bin/sticky-harness-bridge.mjs` (the CLI: session file plus one POST to the
existing endpoint), `examples/run-task.ps1` and `examples/run-task.sh` (report,
run one command, report the outcome) and the `bridge` script in `package.json`.
No Rust file, no capability and no existing commit was touched, and nothing was
rewritten or squashed. The one test-only change outside the new files is the
`parsed` / `parsedStart` helpers in `bridge.test.ts`, added because `tsc`
rejected a union narrowing that `vitest` had not caught.

Phase 7 added the Adapter Management window and its commands:
`harness/adapters_window.rs` (the window, its commands and the
validate-then-save-then-apply rule), `AdaptersWindow.tsx` plus its styles and
command wrappers, the `AdapterStatus` / `AdapterOutcome` surface and
`save_config` in `harness/manager.rs`, `apply_adapter_config` in
`harness/mod.rs`, the tray item in `tray.rs`, and the `harness-adapters`
capability. `EXIT_FLUSH_TIMEOUT` and the exit path are untouched.

Phase 9 prepared the desktop release. `src-tauri/tauri.conf.json` switched
`bundle.targets` to `["msi"]` and added two `bundle.resources` entries that copy
`bin/sticky-harness-bridge.mjs` and `src/harness/bridge.ts` next to the
executable, so the installed bridge runs without the repository.
`src-tauri/src/lib.rs` gained the panic hook plus its two tests, and
`src-tauri/src/paths.rs` gained `app_data_dir_name` and the test that keeps it
equal to the bundle identifier, so the hook's log path still has one owner. The
application name, identifier, executable name and icon are unchanged from earlier
phases, and no data format, no capability and no exit-path code changed. See 4f
and §7 for what was verified. It was committed as `85bde2c chore: prepare
desktop release`.

Phase 10 released v1.0.0 and added no feature. `package.json`, `package-lock.json`,
`src-tauri/Cargo.toml`, the `sticky-harness` entry in `src-tauri/Cargo.lock` and
`src-tauri/tauri.conf.json` were moved from `0.1.0` to `1.0.0`; `README.md` gained
the release notes and the version/roadmap updates, and `AI_HANDOFF.md` gained 4a,
the Phase 10 verification block in §7, the corrected `0xc0000409` record in §8 and
the Maintenance next step in §9. No Rust source, no capability, no exit-path code
and no data format changed, and nothing was pushed. Committed as `0b29754 chore:
prepare v1.0 release`.
