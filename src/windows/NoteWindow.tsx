import { useCallback, useEffect, useRef, useState } from "react";
import { createNote, getNote, saveNoteContent, setNotePinned } from "../services/desktop";
import { logError } from "../utils/logger";

/** Quiet period after the last keystroke before text is written to disk. */
const SAVE_DEBOUNCE_MS = 300;

interface NoteWindowProps {
  /** Stable note id, derived from this window's own label. */
  noteId: string;
}

type SaveState = "idle" | "error";

/**
 * One sticky note.
 *
 * Plain text only for Phase 1: a `+` button, a `Pin` toggle and a textarea
 * that saves itself. No save button, no markdown, no todos.
 */
export function NoteWindow({ noteId }: NoteWindowProps) {
  const [content, setContent] = useState("");
  const [pinned, setPinned] = useState(false);
  const [loaded, setLoaded] = useState(false);
  const [saveState, setSaveState] = useState<SaveState>("idle");
  const [error, setError] = useState<string | null>(null);

  const saveTimerRef = useRef<number | null>(null);
  /** Latest text, so an unmount or a failed save can retry the current value. */
  const pendingRef = useRef<string | null>(null);

  useEffect(() => {
    let cancelled = false;

    getNote(noteId)
      .then((note) => {
        if (cancelled) return;
        setContent(note.content);
        setPinned(note.window.always_on_top);
        setLoaded(true);
      })
      .catch((cause: unknown) => {
        if (cancelled) return;
        logError(`could not load note ${noteId}`, cause);
        setError("This note could not be loaded.");
      });

    return () => {
      cancelled = true;
    };
  }, [noteId]);

  const flushPendingSave = useCallback(() => {
    if (saveTimerRef.current !== null) {
      window.clearTimeout(saveTimerRef.current);
      saveTimerRef.current = null;
    }

    const pending = pendingRef.current;
    if (pending === null) {
      return;
    }

    pendingRef.current = null;
    saveNoteContent(noteId, pending)
      .then(() => setSaveState("idle"))
      .catch((cause: unknown) => {
        logError(`could not save note ${noteId}`, cause);
        setSaveState("error");
      });
  }, [noteId]);

  // Never lose the last keystrokes when the window goes away mid-debounce.
  useEffect(() => flushPendingSave, [flushPendingSave]);

  const handleChange = useCallback(
    (value: string) => {
      setContent(value);
      pendingRef.current = value;

      if (saveTimerRef.current !== null) {
        window.clearTimeout(saveTimerRef.current);
      }

      saveTimerRef.current = window.setTimeout(() => {
        saveTimerRef.current = null;
        flushPendingSave();
      }, SAVE_DEBOUNCE_MS);
    },
    [flushPendingSave],
  );

  const handleCreateNote = useCallback(() => {
    createNote().catch((cause: unknown) => logError("could not create a note", cause));
  }, []);

  const handleTogglePin = useCallback(() => {
    const next = !pinned;
    setPinned(next);
    setNotePinned(noteId, next).catch((cause: unknown) => {
      logError(`could not change always-on-top for ${noteId}`, cause);
      setPinned(!next);
    });
  }, [noteId, pinned]);

  return (
    <div className="note">
      <div className="note__bar">
        <button type="button" className="note__button" onClick={handleCreateNote} title="New note">
          +
        </button>

        <span className="note__status">{saveState === "error" ? "Not saved" : ""}</span>

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
      ) : (
        <textarea
          className="note__editor"
          value={content}
          onChange={(event) => handleChange(event.target.value)}
          placeholder="Type here…"
          spellCheck={false}
          autoFocus
          disabled={!loaded}
        />
      )}
    </div>
  );
}
