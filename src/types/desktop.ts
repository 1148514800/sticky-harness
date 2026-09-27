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

/** The window label of the single Harness Task Note. */
export const HARNESS_TASK_LABEL = "harness-tasks";

/**
 * The status of one harness task.
 *
 * Mirrors the Rust enum, which serialises in snake_case. Only `running` and
 * `waiting` can reach this UI, because the live view the note reads excludes
 * everything else.
 */
export type HarnessTaskStatus =
  | "running"
  | "waiting"
  | "failed"
  | "completed"
  | "cancelled"
  | "unknown";

/**
 * One active harness task, flattened with the harness it belongs to.
 *
 * This mirrors `ActiveHarnessTask` in Rust and is deliberately not the whole
 * snapshot: the note shows who is running and what they are doing, and nothing
 * else from the protocol.
 */
export interface ActiveHarnessTask {
  harness_id: string;
  harness_name: string;
  task_id: string;
  title: string;
  status: HarnessTaskStatus;
  /** Unix milliseconds; elapsed time is computed from this, never stored. */
  started_at: number;
  /** Unix milliseconds. */
  updated_at: number;
  message?: string | null;
}

/** Whether this window exists and is on screen, as Rust sees it. */
export interface HarnessWindowStatus {
  open: boolean;
  visible: boolean;
  pinned: boolean;
}

/** The window label of the Adapter Management window. */
export const ADAPTERS_LABEL = "harness-adapters";

/** Which transport an adapter reads through. Mirrors the Rust enum. */
export type AdapterKind = "local-json" | "local-http";

/**
 * One adapter as the management window shows it.
 *
 * Mirrors `AdapterView` in Rust: the configuration plus the last health
 * reading. The window never receives the raw file format, so it cannot
 * round-trip a field it did not display.
 */
export interface AdapterView {
  name: string;
  kind: AdapterKind;
  enabled: boolean;
  /** A file path, or the loopback URL for an HTTP adapter. */
  source: string;
  /** `ok`, `error`, `rejected`, `waiting`, or `off` while disabled. */
  status: string;
  detail?: string | null;
  checked_at?: number | null;
  succeeded_at?: number | null;
  poll_interval_millis?: number | null;
  timeout_millis?: number | null;
  http_path?: string | null;
  port?: number | null;
  path?: string | null;
}

/** Everything the adapter window renders in one poll. */
export interface AdaptersView {
  adapters: AdapterView[];
  /** How many adapters are actually running. */
  running: number;
  /**
   * Why the file on disk could not be used, if it could not.
   *
   * A file that does not parse starts the app with no adapters, which is
   * correct but looks the same as "no adapters configured" unless we say so.
   */
  problem?: string | null;
}

/** The editable half of an adapter, as the form sends it. */
export interface AdapterInput {
  name: string;
  kind: AdapterKind;
  enabled: boolean;
  path?: string | null;
  port?: number | null;
  http_path?: string | null;
  timeout_millis?: number | null;
  poll_interval_millis?: number | null;
}

/** Whether a window label belongs to the adapter management window. */
export function isAdaptersLabel(label: string): boolean {
  return label === ADAPTERS_LABEL;
}

/** Whether a window label belongs to the Harness Task Note. */
export function isHarnessTaskLabel(label: string): boolean {
  return label === HARNESS_TASK_LABEL;
}
