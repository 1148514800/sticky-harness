import { useCallback, useEffect, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { HarnessTaskList } from "../components/HarnessTaskList";
import {
  getHarnessWindowStatus,
  listLiveActiveHarnessTasks,
  setHarnessWindowPinned,
} from "../services/desktop";
import type { ActiveHarnessTask } from "../types/desktop";
import { logError } from "../utils/logger";

/** How often the note asks Rust what is running. */
const POLL_INTERVAL_MS = 1000;
/** How often the elapsed times are redrawn between polls. */
const TICK_INTERVAL_MS = 1000;

/**
 * The Harness Task Note.
 *
 * A read-only view of `HarnessRegistry`: it polls the live active tasks and
 * renders them. It is not a normal note - no editor, no Markdown, no note file -
 * and closing it hides the window rather than discarding anything, because
 * nothing here is user content.
 *
 * Polling is deliberate. The registry lives in this same process, so a
 * WebSocket or SSE channel would be machinery serving an event that already
 * happens locally; one command a second is enough for a status surface.
 */
export function HarnessTaskWindow() {
  const [tasks, setTasks] = useState<ActiveHarnessTask[]>([]);
  const [pinned, setPinned] = useState(false);
  const [loaded, setLoaded] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [now, setNow] = useState(() => Date.now());

  /** Set while a poll is in flight, so a slow one cannot stack up. */
  const pollingRef = useRef(false);

  const refresh = useCallback(async () => {
    if (pollingRef.current) return;
    pollingRef.current = true;

    try {
      const next = await listLiveActiveHarnessTasks();
      setTasks(next);
      setError(null);
      setLoaded(true);
    } catch (cause) {
      logError("could not refresh harness tasks", cause);
      setError("Harness tasks could not be loaded.");
    } finally {
      pollingRef.current = false;
    }
  }, []);

  // The first read happens immediately, so the window is never briefly wrong.
  useEffect(() => {
    void refresh();
  }, [refresh]);

  useEffect(() => {
    const timer = window.setInterval(() => {
      void refresh();
    }, POLL_INTERVAL_MS);

    return () => window.clearInterval(timer);
  }, [refresh]);

  // Redraw the elapsed times once a second. This is a local clock tick, not a
  // second IPC round trip: durations are computed from `started_at`.
  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), TICK_INTERVAL_MS);
    return () => window.clearInterval(timer);
  }, []);

  // Picking the window back up should show current state at once, rather than
  // whatever was on screen when it was hidden.
  useEffect(() => {
    let stop: (() => void) | null = null;
    let cancelled = false;

    getCurrentWindow()
      .onFocusChanged(({ payload: focused }) => {
        if (focused) void refresh();
      })
      .then((unlisten) => {
        if (cancelled) {
          unlisten();
          return;
        }
        stop = unlisten;
      })
      .catch((cause: unknown) => logError("could not watch harness window focus", cause));

    return () => {
      cancelled = true;
      stop?.();
    };
  }, [refresh]);

  useEffect(() => {
    let cancelled = false;

    getHarnessWindowStatus()
      .then((status) => {
        if (!cancelled) setPinned(status.pinned);
      })
      .catch((cause: unknown) => logError("could not read the harness window state", cause));

    return () => {
      cancelled = true;
    };
  }, []);

  const handleTogglePin = useCallback(() => {
    const next = !pinned;
    setPinned(next);
    setHarnessWindowPinned(next).catch((cause: unknown) => {
      logError("could not change always-on-top for the harness window", cause);
      setPinned(!next);
    });
  }, [pinned]);

  return (
    <div className="note harness">
      <div className="note__bar">
        <span className="harness__heading">Harness Tasks</span>

        <button
          type="button"
          className={pinned ? "note__button note__button--active" : "note__button"}
          onClick={handleTogglePin}
          title={pinned ? "Unpin (always on top)" : "Pin (always on top)"}
          aria-pressed={pinned}
        >
          Pin
        </button>
      </div>

      {error ? (
        <p className="note__error">{error}</p>
      ) : loaded ? (
        <HarnessTaskList tasks={tasks} now={now} />
      ) : null}
    </div>
  );
}
