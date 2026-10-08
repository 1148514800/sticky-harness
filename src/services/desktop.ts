import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import type {
  ActiveHarnessTask,
  AdapterInput,
  AdaptersView,
  HarnessWindowStatus,
  NoteRecord,
} from "../types/desktop";

/**
 * Thin wrapper around the Rust note commands.
 *
 * Note identity, persistence, window lifecycle, always-on-top and the tray all
 * live in Rust; React only asks for them. Every call converts a rejected IPC
 * promise into a readable Error so UI code can report a problem instead of
 * crashing.
 */

/**
 * Emitted by Rust when the tray asked to quit. Every note window answers by
 * saving its last edit and confirming, which is what lets a quit wait for the
 * final keystrokes instead of dropping them.
 */
export const EXIT_REQUESTED_EVENT = "sticky-harness://exit-requested";

async function callCommand<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (error) {
    const reason = typeof error === "string" ? error : String(error);
    throw new Error(`${command} failed: ${reason}`);
  }
}

/** Create a new note and its window. Used by the in-note `+` button. */
export async function createNote(): Promise<NoteRecord> {
  return callCommand<NoteRecord>("new_note");
}

/** Load one note by its stable id. */
export async function getNote(id: string): Promise<NoteRecord> {
  return callCommand<NoteRecord>("get_note", { id });
}

/** Persist edited text. Rust refreshes `updated_at`. */
export async function saveNoteContent(id: string, content: string): Promise<void> {
  return callCommand<void>("save_note_content", { id, content });
}

/** Tell Rust this note finished its final save while the app is quitting. */
export async function confirmExitFlush(id: string): Promise<void> {
  return callCommand<void>("confirm_exit_flush", { id });
}

/** Open the Harness Tasks window, creating it if needed. */
export async function openHarnessTasksWindow(): Promise<boolean> {
  return callCommand<boolean>("open_harness_tasks_window");
}
/** Turn always-on-top on or off for one note, in the OS and on disk. */
export async function setNotePinned(id: string, pinned: boolean): Promise<void> {
  return callCommand<void>("set_note_pinned", { id, pinned });
}

/**
 * Open a web or mail link in the default app.
 *
 * The note window never navigates. Callers must already have refused schemes
 * other than http, https and mailto.
 */
export async function openExternalUrl(url: string): Promise<void> {
  try {
    await openUrl(url);
  } catch (error) {
    const reason = typeof error === "string" ? error : String(error);
    throw new Error(`open_url failed: ${reason}`);
  }
}

/**
 * Every active task from a harness that is still reporting.
 *
 * The Rust registry applies both rules - the task is active and its harness is
 * not stale - so the UI never inspects snapshots or decides what stale means.
 */
export async function listLiveActiveHarnessTasks(): Promise<ActiveHarnessTask[]> {
  return callCommand<ActiveHarnessTask[]>("list_live_active_harness_tasks");
}

/**
 * The configured adapters, joined with their last health reading.
 *
 * The window polls this once a second. Every mutation below returns the same
 * shape, so the caller never has to re-read after a change.
 */
export async function listAdapters(): Promise<AdaptersView> {
  return callCommand<AdaptersView>("list_adapters");
}

/**
 * Add an adapter, or replace the one with the same name.
 *
 * Rust validates the whole resulting configuration before writing anything, so
 * a rejected edit throws and leaves the file untouched.
 */
export async function saveAdapter(
  entry: AdapterInput,
  previousName?: string | null,
): Promise<AdaptersView> {
  return callCommand<AdaptersView>("save_adapter", {
    entry,
    previousName: previousName ?? null,
  });
}

/** Turn one adapter on or off. */
export async function setAdapterEnabled(name: string, enabled: boolean): Promise<AdaptersView> {
  return callCommand<AdaptersView>("set_adapter_enabled", { name, enabled });
}

/** Remove one adapter. */
export async function deleteAdapter(name: string): Promise<AdaptersView> {
  return callCommand<AdaptersView>("delete_adapter", { name });
}

/**
 * Re-read `harness-adapters.json` and start from it.
 *
 * The escape hatch for hand-editing: the file is the contract, so a change made
 * in an editor must not require a restart.
 */
export async function reloadAdapters(): Promise<AdaptersView> {
  return callCommand<AdaptersView>("reload_adapters");
}

/** Open the Harness Adapters management window, creating it if needed. */
export async function openAdaptersWindow(): Promise<boolean> {
  return callCommand<boolean>("open_adapters_window");
}
/** Whether the Harness Task Note exists, is visible, and is pinned. */
export async function getHarnessWindowStatus(): Promise<HarnessWindowStatus> {
  return callCommand<HarnessWindowStatus>("harness_window_status");
}

/** Pin or unpin the Harness Task Note, stored in its own window config. */
export async function setHarnessWindowPinned(pinned: boolean): Promise<void> {
  return callCommand<void>("set_harness_window_pinned", { pinned });
}
