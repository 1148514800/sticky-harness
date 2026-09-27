import { getCurrentWindow } from "@tauri-apps/api/window";
import { isAdaptersLabel, isHarnessTaskLabel, noteIdForLabel } from "./types/desktop";
import { AdaptersWindow } from "./windows/AdaptersWindow";
import { HarnessTaskWindow } from "./windows/HarnessTaskWindow";
import { NoteWindow } from "./windows/NoteWindow";

/**
 * Root component for a single webview.
 *
 * Every window is its own Tauri window, and each mounts its own React root. The
 * view is chosen from that window's own label, so there is no shared root window
 * and closing one window can never affect another.
 *
 * There are three kinds of window, and the label says which:
 *
 * - `note-<id>`        a normal Markdown note the user owns
 * - `harness-tasks`    the single read-only harness status note
 * - `harness-adapters` the single adapter configuration window
 */
function App() {
  let label = "";

  try {
    label = getCurrentWindow().label;
  } catch (cause) {
    console.error("[sticky-harness] could not read the window label", cause);
  }

  if (isHarnessTaskLabel(label)) {
    return <HarnessTaskWindow />;
  }

  if (isAdaptersLabel(label)) {
    return <AdaptersWindow />;
  }

  const noteId = noteIdForLabel(label);

  if (noteId === null) {
    return (
      <p className="note__error">
        This window is not a note window. Create one from the tray menu.
      </p>
    );
  }

  return <NoteWindow noteId={noteId} />;
}

export default App;
