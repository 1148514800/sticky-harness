/**
 * Shared desktop-layer types.
 *
 * Phase 0 only distinguishes the main window from temporary test windows so
 * that each webview can render the right view. Phase 1 will replace this with
 * a real note/window identity model.
 */

export const MAIN_WINDOW_LABEL = "main";

export type WindowRole = "main" | "test-note";

export interface CreatedWindow {
  /** Unique Tauri window label, e.g. `test-note-1`. */
  label: string;
  /** Which view this window should render. */
  role: WindowRole;
}

export interface AppPathInfo {
  /** Absolute path to the OS-specific per-app data directory. */
  appDataDir: string;
}

export function windowRoleForLabel(label: string): WindowRole {
  return label === MAIN_WINDOW_LABEL ? "main" : "test-note";
}
