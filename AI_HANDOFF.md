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

Phase 6 — Harness Adapters

Status: Completed

Done:

- Phases 1 to 5: sticky notes, Markdown editing, desktop experience, the local
  harness protocol and the Harness Task Note. See 4b and 4c below.
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

Not done (intentionally, do not start without a new task):

- Settings UI, adapter management UI, and any adapter editing outside the JSON
  file. A malformed or missing configuration is logged, not surfaced in UI.
- Codex or DeepSeek control, prompt sending, task history, logs, tool calls, a
  terminal view, search, notifications, SQLite, cloud, authentication,
  WebSocket, SSE, and any remote network access. Local HTTP means loopback only.

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

## 5. Important Files

```
src/App.tsx                           Picks the view from this window label
src/windows/NoteWindow.tsx            One note: + button, Pin, autosave
src/windows/HarnessTaskWindow.tsx     Read-only task view, polls once a second
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
src/styles.css                        Minimal note styling

src-tauri/src/lib.rs                  Setup, command registration, exit veto
src-tauri/src/notes.rs                Note identity, persistence, window lifecycle
src-tauri/src/tray.rs                 Tray icon, menu, autostart checkbox
src-tauri/src/paths.rs                Sole owner of the local data directory
src-tauri/src/harness_window.rs       The Harness Task Note window and its state
src-tauri/src/harness/mod.rs          Harness wiring, Tauri commands, init
src-tauri/src/harness/protocol.rs     The protocol model, status enum, constants
src-tauri/src/harness/registry.rs     In-memory snapshots and active filtering
src-tauri/src/harness/server.rs       Loopback-only push endpoint
src-tauri/src/harness/adapter.rs      HarnessAdapter trait + LocalJson/LocalHttp
src-tauri/src/harness/manager.rs      AdapterManager: config, workers, isolation
src-tauri/src/harness/tests.rs        Core tests (validation, registry, stale)
src-tauri/capabilities/default.json   Core permissions for note-* and harness-tasks
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
- The Harness Task Note opens from Tray -> Harness Tasks, shows only live active
  tasks grouped by harness, and refreshes once a second. Closing it with X
  hides it; the tray item focuses the same window again rather than creating a
  second one.
- Show All and Hide All cover the Harness Task Note too. Hiding it deletes
  nothing and changes no registry state; showing it never creates it if it was
  never opened.
- Tray Exit quits the process and keeps every note file, including notes that
  were hidden.
- A local harness can POST its current state to
  `http://127.0.0.1:17899/api/harness/snapshot` and the app remembers it in
  memory; `GET /api/harness/snapshots` reads it back. Nothing about this is
  persisted, and nothing is shown in the UI yet.
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
                            intact
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
  of its APIs work.
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
  and no settings screen yet, by design.
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
- **`harness-adapters.json` is hand-edited and has no UI.** An unknown key, a
  duplicate name, a bad kind or an out-of-range value is logged and the whole
  file is rejected, so the app starts with no adapters rather than a partial
  set. There is no settings screen, no validation feedback in the UI and no
  reload without a restart - extending the running configuration is Phase 7's
  problem.
- **An adapter can only ever add harness state, never inspect the app.** It
  writes into `HarnessRegistry` through the same validated `HarnessSnapshot`
  contract as the push endpoint, so an adapter cannot create a note, touch
  `notes/`, or send anything anywhere. `local-json` never executes what it
  reads, and `local-http` only ever connects to `127.0.0.1`.
- **Adapter workers sleep in 50 ms slices to make Exit quick.** That is the
  only thing keeping a 5-minute poll interval from delaying the tray; it costs
  a wakeup every 50 ms per adapter, which is irrelevant at this scale but should
  be revisited if adapters ever became numerous.

## 9. Next Step

Next: Phase 7 — Adapter Management / Product Polish

Phase 6 is done: harness state is produced by real adapters, not by hand, and
the stale rule is finally honest about what it measures.

What Phase 6 deliberately left alone, and what Phase 7 is for:

- Adapters are configured by hand-editing `<AppData>/harness-adapters.json`.
  There is no UI, no validation feedback and no reload without a restart. A
  Phase 7 can add a management surface and a live reload while keeping the file
  format as the contract.
- Nothing about `HarnessAdapter` needs redesigning to do that. An adapter is
  still one trait, two implementations, one manager, one registry. Do not add a
  second abstraction, a plugin loader, a scripting host or per-adapter SDKs.
- The remaining product polish from earlier phases is still open: images,
  themes, search and a settings window. The adapter management surface is the
  natural place to put the harness settings, and the two should not become two
  separate screens.

Still out of scope, and still worth refusing:

- No task history, log or tool-call viewer, no terminal, no notifications, no
  SQLite, no cloud, no authentication, no WebSocket or SSE.
- No remote network. `local-http` means loopback, and it should keep meaning
  loopback.
- Still no Direct Codex or DeepSeek adapter. The conclusion stands until one of
  them publishes a stable, documented, read-only "currently running" source;
  until then Bridge is the honest answer, and a fake Direct adapter would be a
  regression dressed up as a feature.

## 10. Latest Commit

Remote: `https://github.com/1148514800/sticky-harness` (private, default branch
`main`). Local commits only; nothing has been pushed.

```
cd92869 docs: record the Phase 4 commit in the handoff
eeb5353 feat: add local harness protocol                (Phase 4)
06dbf5b docs: record the Phase 3 commit in the handoff
c2ef7e7 feat: improve desktop note experience           (Phase 3)
8d83a70 fix: complete markdown note runtime behavior    (Phase 2 closeout)
c1c1f85 docs: record Phase 2 commit in handoff
2c8c872 feat: edit notes as markdown with todos         (Phase 2)

7fa106b feat: add harness task note                   (Phase 5)
b70e5a6 docs: record the Phase 5 commit in the handoff
```

Every commit is local. Nothing has been pushed. Phase 6 adds the adapters:
`harness/manager.rs` (config, workers, failure isolation, shutdown),
`harness/adapter.rs` (the `HarnessAdapter` trait plus `LocalJsonAdapter` and
`LocalHttpAdapter`), the config path in `paths.rs`, `start_adapters` and
`stop_adapters` in `harness/mod.rs`, the Exit wiring in `lib.rs`, and the
clock-independent stale rule in `protocol.rs` / `registry.rs`. No existing
commit was rewritten or squashed.

