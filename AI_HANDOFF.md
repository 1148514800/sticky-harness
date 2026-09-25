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

Phase 4 — Harness Protocol

Status: Completed

Done:

- Phase 1 sticky notes, Phase 2 Markdown editing, Phase 3 desktop experience.
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
- Staleness is computed, never assumed: a snapshot is stale when
  `now - max(producer.updated_at, received_at)` passes the timeout. Nothing is
  deleted on staleness in this phase.
- The registry is deliberately not persisted. Harness state describes live
  processes, so an empty registry after a restart is the truth.

Not done (intentionally, do not start without a new task):

- Any real harness integration. No Codex, no DeepSeek, no adapter for either.
- The Harness Task Note UI (that is Phase 5).
- Task history, SQLite, log or tool-call viewers, settings screen,
  authentication, cloud, WebSocket and SSE. Snapshots are enough.

## 5. Important Files

```
src/App.tsx                           Picks the view from this window label
src/windows/NoteWindow.tsx            One note: + button, Pin, autosave
src/components/NoteEditor.tsx         WYSIWYG Markdown editor for one note
src/editor/markdown.ts                 Markdown <-> editor document conversion
src/editor/todoInput.ts                Turns a typed `- [ ] ` into a checkbox
src/editor/links.ts                    Which clicks open a link, and which URLs
src/services/desktop.ts                Typed wrappers over the Rust note commands
src/types/desktop.ts                  NoteRecord/NoteWindowState + label parsing
src/utils/logger.ts                   Console logging helpers
src/styles.css                        Minimal note styling

src-tauri/src/lib.rs                  Setup, command registration, exit veto
src-tauri/src/notes.rs                Note identity, persistence, window lifecycle
src-tauri/src/tray.rs                 Tray icon, menu, autostart checkbox
src-tauri/src/paths.rs                Sole owner of the local data directory
src-tauri/src/harness/mod.rs          Harness wiring, Tauri commands, init
src-tauri/src/harness/protocol.rs     The protocol model, status enum, constants
src-tauri/src/harness/registry.rs     In-memory snapshots and active filtering
src-tauri/src/harness/server.rs       Loopback-only push endpoint
src-tauri/src/harness/adapter.rs      Pull seam + reference local-JSON adapter
src-tauri/src/harness/tests.rs        Core tests (validation, registry, stale)
src-tauri/capabilities/default.json   Core permissions for note-* windows
src-tauri/tauri.conf.json             No startup window; notes created by Rust
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
- Tray Exit quits the process and keeps every note file, including notes that
  were hidden.
- A local harness can POST its current state to
  `http://127.0.0.1:17899/api/harness/snapshot` and the app remembers it in
  memory; `GET /api/harness/snapshots` reads it back. Nothing about this is
  persisted, and nothing is shown in the UI yet.
- If the harness port is already taken, the app logs the failure and starts
  anyway: notes and the tray are unaffected.
- A corrupt or hand-edited note file is logged and skipped without stopping the
  other notes from loading.

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

## 8. Known Issues

- **Capabilities the frontend needs must be granted explicitly.**
  `core:window:allow-destroy` is what lets a closing note window actually
  finish closing; without it the X button deleted the JSON and left the window
  on screen. `core:window:allow-set-size` is granted for the same reason, so
  the note can be resized to its documented 220x160 floor. `core:window:default`
  contains neither, which is worth remembering before adding the next window
  API call.
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
  is dropped on exit, so a note opened after a restart shows nothing until a
  harness reports again. Staleness is computed but never acted on, so a harness
  that dies mid-task keeps showing its task as active until it reports or the
  app restarts. Both are deliberate for this phase; deciding what to *do* about
  a stale harness needs a task of its own.
- **The push endpoint has no authentication.** It is bound to loopback only and
  sends no CORS header, so a local process can post harness state and a web page
  cannot read it cross-origin. That is the intended boundary for local IPC; it
  is not a security model for untrusted local code.
- **`HARNESS_API_PORT` is a fixed constant (17899).** If it is taken the app
  logs the failure and runs without the harness API. There is no fallback port
  and no settings screen yet, by design.
- **The HTTP parser is deliberately minimal.** It handles a single request per
  connection with `Connection: close` and reads only `Content-Length`. A
  `Transfer-Encoding: chunked` body is not supported, because a local snapshot
  upload does not need it. Revisit only if a real producer sends chunked.
- Automated UI verification used temporary CDP / UI-Automation helper scripts
  that live in %TEMP%\sh-verify and are not part of the repo; re-create them
  if runtime behaviour must be re-tested.

## 9. Next Step

Next: Phase 5 — Harness Task Note

Direction only, do not start without a new task. Phase 4 built the protocol and
the state; Phase 5 is the note that shows it, and nothing here should be
implemented early.

The contract Phase 5 should be built on already exists:

- Read state through `list_active_harness_tasks` (Tauri command) or
  `HarnessRegistry::list_active_tasks`. Do not scan or filter the registry from
  React, and do not re-implement the active rule: ask
  `HarnessStatus::is_active()`.
- `ActiveHarnessTask` is the view model: harness id and name, task id and title,
  status, started_at, updated_at and an optional short message. Keep it to that.
- Compute elapsed time in the UI from `now - started_at`; the protocol never
  carries a rendered duration.
- The Harness Task Note stays a separate concept from a normal sticky note. It
  is not a note file and must not go through the note JSON format.

Constraints that already hold and must survive Phase 5:

- `notes.rs` stays the only owner of note persistence, and `create_and_open`
  stays the only way a note window comes into being.
- No task history, no SQLite, no log or tool-call viewer, no settings screen.
- Everything stays local: no account, no sync, no server.

## 10. Latest Commit

Remote: `https://github.com/1148514800/sticky-harness` (private, default branch
`main`). Local commits only; nothing has been pushed.

```
c2ef7e7 feat: improve desktop note experience            (Phase 3)
8d83a70 fix: complete markdown note runtime behavior   (Phase 2 closeout)
c1c1f85 docs: record Phase 2 commit in handoff
2c8c872 feat: edit notes as markdown with todos        (Phase 2)
1b43292 docs: record Phase 1 commit in handoff
```

Every commit is local. Nothing has been pushed, and the Phase 3 work adds:
the tray menu with Show All Notes / Hide All Notes, the official autostart
plugin behind Start with Windows, the 220x160 minimum size, and the light note
styling pass.
