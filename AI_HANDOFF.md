# AI Handoff

This is the concise handoff for continuing work on Sticky Harness. If this file
disagrees with the code, trust the code and update this file.

## Project Goal

A Windows-first desktop app for minimal floating sticky notes, plus a local
Harness Tasks panel that shows currently running AI harness tasks. Everything
stays local: no account, no sync, no cloud server.

## Core Decisions

- Tauri 2 + React + TypeScript + Vite; Rust owns persistence, windows, tray and OS integration.
- Users get multiple independent floating Markdown notes.
- Normal notes and the Harness Tasks / Harness Adapters windows are separate concepts.
- One note is one JSON file, one window and one stable id; no SQLite.
- Closing a normal note deletes it without confirmation; tray `Exit` must never delete notes.
- All harness data is local and loopback-only.
- No cloud, authentication, remote networking, notifications, task history or terminal viewer.

## Important Files

- `src/windows/NoteWindow.tsx` — note window UI and the `+` window-type menu.
- `src/components/NoteEditor.tsx` and `src/editor/markdown.ts` — Markdown editing.
- `src/services/desktop.ts` — typed wrappers around Tauri commands.
- `src-tauri/src/notes.rs` — note persistence, restore, close/delete and exit behavior.
- `src-tauri/src/tray.rs` — Chinese tray menu and tray event handling.
- `src-tauri/src/harness_window.rs` — Harness Tasks window and manual-open command.
- `src-tauri/src/harness/adapters_window.rs` — Harness Adapters management window.
- `src-tauri/src/harness/{protocol.rs,registry.rs,server.rs,adapter.rs,manager.rs}` — harness protocol, registry, loopback push and local pull adapters.
- `bin/sticky-harness-bridge.mjs` and `src/harness/bridge.ts` — local bridge CLI and pure bridge logic.
- `APP_USAGE.md` — current Chinese user guide.

## Current State

- Installed app version: `1.0.0`.
- Desktop shortcut: `C:\Users\godblack\Desktop\Sticky Harness.lnk`.
- Installed executable: `%LOCALAPPDATA%\Programs\Sticky Harness\sticky-harness.exe`.
- User data: `%APPDATA%\com.stickyharness.desktop`.
- The `+` button offers: normal note, Harness Tasks and Harness Adapters.
- The tray menu is Chinese: 新建便签, 任务面板, 适配器管理, 显示全部窗口, 隐藏全部窗口, 随 Windows 启动, 退出.
- Startup intentionally restores only note windows. Harness Tasks is manually opened and must not be restored automatically.
- MSI install may recreate a duplicate shortcut in `C:\Users\Public\Desktop`; delete that duplicate and keep the user-desktop shortcut.
- The latest installed build was rebuilt from the current working tree on 2026-10-08.

## Local Environment

- MSVC is installed at `F:\software\VisualStudio`; a plain PowerShell does not find `cl.exe`.
- Build the Windows app with:

```powershell
cmd /c "call F:\software\VisualStudio\VC\Auxiliary\Build\vcvars64.bat && cd /d F:\study\note_app && npm run tauri build"
```

- Frontend-only checks:

```powershell
npm run typecheck
npm test
npm run build
```

- Rust checks require the same MSVC environment:

```powershell
cargo test
cargo check --all-targets
```

## Verification Snapshot

On 2026-10-08 the current working-tree changes were verified with:

- `npm run typecheck` — passed.
- `npm test` — 127 tests passed across 6 files.
- `npm run build` — passed; only the pre-existing chunk-size warning.
- `npm run tauri build` — passed in the MSVC environment.
- Installed app restart test: only one window opened, titled `便签`.

## Current Working Tree

Uncommitted changes include:

- Chinese tray menu.
- `+` button type selector for normal note, Harness Tasks and Harness Adapters.
- Manual-only Harness Tasks opening; automatic restore removed from startup.
- `APP_USAGE.md` user guide.

Check exact files with `git status --short` before editing.

## Next Step

1. Review `git status --short` and the code before making assumptions.
2. Keep the current UX decisions above; do not reintroduce automatic Harness Tasks restoration.
3. Preserve user data during installer work; never delete `%APPDATA%\com.stickyharness.desktop` during tests.
4. Run the verification commands in the listed environments.
5. Update this file only when current state, architecture or handoff-critical behavior changes.
