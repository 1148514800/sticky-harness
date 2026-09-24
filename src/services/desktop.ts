import { invoke } from "@tauri-apps/api/core";
import type { NoteRecord } from "../types/desktop";

/**
 * Thin wrapper around the Rust note commands.
 *
 * Note identity, persistence, window lifecycle, always-on-top and the tray all
 * live in Rust; React only asks for them. Every call converts a rejected IPC
 * promise into a readable Error so UI code can report a problem instead of
 * crashing.
 */

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

/** Turn always-on-top on or off for one note, in the OS and on disk. */
export async function setNotePinned(id: string, pinned: boolean): Promise<void> {
  return callCommand<void>("set_note_pinned", { id, pinned });
}
