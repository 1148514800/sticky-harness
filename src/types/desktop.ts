/**
 * Shared desktop-layer types.
 *
 * These mirror the Rust structs in `src-tauri/src/notes.rs`. Rust owns note
 * identity and persistence; the frontend only reads this shape and renders it.
 */

/** Every note window label starts with this prefix. */
export const NOTE_LABEL_PREFIX = "note-";

/** Saved window geometry, in physical pixels. Absent fields mean "not known". */
export interface NoteWindowState {
  x?: number | null;
  y?: number | null;
  width?: number | null;
  height?: number | null;
  always_on_top: boolean;
}

/** One sticky note as stored by Rust. */
export interface NoteRecord {
  /** Stable for the lifetime of the note; also the JSON file stem. */
  id: string;
  content: string;
  /** Unix epoch milliseconds. */
  created_at: number;
  /** Unix epoch milliseconds. */
  updated_at: number;
  window: NoteWindowState;
}

/**
 * Extract the note id from a window label, or `null` when the label is not a
 * note window label.
 */
export function noteIdForLabel(label: string): string | null {
  if (!label.startsWith(NOTE_LABEL_PREFIX)) {
    return null;
  }

  const id = label.slice(NOTE_LABEL_PREFIX.length);
  return id.length > 0 ? id : null;
}
