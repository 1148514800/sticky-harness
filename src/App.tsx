import { getCurrentWindow } from "@tauri-apps/api/window";
import { MainWindow } from "./windows/MainWindow";
import { TestNoteWindow } from "./windows/TestNoteWindow";
import { windowRoleForLabel } from "./types/desktop";

/**
 * Root component for a single webview.
 *
 * Every Tauri window mounts its own React root, and the view is chosen from
 * that window's own label. Nothing here is global app state, so one window
 * closing can never affect another window's UI.
 */
function App() {
  let label = "main";

  try {
    label = getCurrentWindow().label;
  } catch (cause) {
    console.error("[sticky-harness] could not read the window label; falling back to main", cause);
  }

  return windowRoleForLabel(label) === "main" ? (
    <MainWindow />
  ) : (
    <TestNoteWindow label={label} />
  );
}

export default App;
