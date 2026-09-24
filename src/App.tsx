import { getCurrentWindow } from "@tauri-apps/api/window";
import { noteIdForLabel } from "./types/desktop";
import { NoteWindow } from "./windows/NoteWindow";

/**
 * Root component for a single webview.
 *
 * Every note is its own Tauri window, and each mounts its own React root. The
 * view is chosen from that window's own label, so there is no shared root
 * window and closing one note can never affect another.
 */
function App() {
  let label = "";

  try {
    label = getCurrentWindow().label;
  } catch (cause) {
    console.error("[sticky-harness] could not read the window label", cause);
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
