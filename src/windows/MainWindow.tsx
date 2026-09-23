import { useEffect, useState } from "react";
import { DevToolbar } from "../components/DevToolbar";
import { NoteShell } from "../components/NoteShell";
import { getAppPathInfo } from "../services/desktop";
import { logError } from "../utils/logger";

/**
 * Main test window.
 *
 * Proves the shell boots and that Rust can be asked for the app data
 * directory. It holds no note state and is not the owner of other windows.
 */
export function MainWindow() {
  const [appDataDir, setAppDataDir] = useState<string | null>(null);
  const [appDataError, setAppDataError] = useState<string | null>(null);

  useEffect(() => {
    let isActive = true;

    getAppPathInfo()
      .then((info) => {
        if (!isActive) {
          return;
        }
        setAppDataDir(info.appDataDir);
        console.info(`[sticky-harness] app data dir: ${info.appDataDir}`);
      })
      .catch((cause: unknown) => {
        if (!isActive) {
          return;
        }
        logError("could not resolve the app data directory", cause);
        setAppDataError(cause instanceof Error ? cause.message : String(cause));
      });

    return () => {
      isActive = false;
    };
  }, []);

  return (
    <NoteShell title="便签">
      <p className="note-shell__phase">Phase 0</p>
      <p className="note-shell__status">Desktop app is running.</p>

      <DevToolbar />

      <dl className="path-info">
        <dt>App Data</dt>
        <dd>{appDataDir ?? (appDataError ? `Unavailable (${appDataError})` : "Resolving...")}</dd>
      </dl>
    </NoteShell>
  );
}
