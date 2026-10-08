import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { NoteEditor } from "../components/NoteEditor";
import {
  EXIT_REQUESTED_EVENT,
  confirmExitFlush,
  createNote,
  openAdaptersWindow,
  openHarnessTasksWindow,
  getNote,
  saveNoteContent,
  setNotePinned,
} from "../services/desktop";
import { logError } from "../utils/logger";

/** Quiet period after the last edit before Markdown is written to disk. */
const SAVE_DEBOUNCE_MS = 300;

interface NoteWindowProps {
  /** Stable note id, derived from this window's own label. */
  noteId: string;
}

type SaveState = "idle" | "error";

/**
 * One sticky note.
 *
 * Window-level concerns only: loading the note, the `+` / `Pin` bar, and
 * autosaving the Markdown the editor produces. Markdown handling itself lives
 * in `editor/markdown.ts` and `components/NoteEditor.tsx`.
 */
export function NoteWindow({ noteId }: NoteWindowProps) {
  const [content, setContent] = useState("");
  const [pinned, setPinned] = useState(false);
  const [loaded, setLoaded] = useState(false);
  const [saveState, setSaveState] = useState<SaveState>("idle");
  const [error, setError] = useState<string | null>(null);

  const saveTimerRef = useRef<number | null>(null);
  /** Save currently on its way to Rust, so a quit can wait for it. */
  const inFlightRef = useRef<Promise<void> | null>(null);
  /** Newest Markdown not yet handed to Rust, or `null` when all is saved. */
  const pendingRef = useRef<string | null>(null);
  /** Set once this window is closing, so no save may follow the delete. */
  const closingRef = useRef(false);

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

  const write = useCallback(
    async (markdown: string) => {
      const run = (async () => {
        try {
          await saveNoteContent(noteId, markdown);
          setSaveState("idle");
        } catch (cause) {
          logError(`could not save note ${noteId}`, cause);
          setSaveState("error");
          // Keep the text pending so a later flush can still write it.
          pendingRef.current = markdown;
        }
      })();

      inFlightRef.current = run;
      try {
        await run;
      } finally {
        if (inFlightRef.current === run) {
          inFlightRef.current = null;
        }
      }
    },
    [noteId],
  );

  /**
   * Write everything owed to disk right now.
   *
   * Returns a promise so the exit handshake can wait for the write to land
   * before the process is allowed to quit.
   */
  const flushPendingSave = useCallback(async () => {
    if (saveTimerRef.current !== null) {
      window.clearTimeout(saveTimerRef.current);
      saveTimerRef.current = null;
    }

    // An edit made less than one debounce ago may already have started saving;
    // quitting must not outrun it.
    while (inFlightRef.current !== null) {
      await inFlightRef.current;
    }

    // A closing note must never write. This is what previously let an unmount
    // flush recreate a JSON file that Rust had just deleted.
    if (closingRef.current) {
      return;
    }

    const pending = pendingRef.current;
    if (pending === null) {
      return;
    }

    pendingRef.current = null;
    await write(pending);
  }, [write]);

  const handleEditorChange = useCallback(
    (markdown: string) => {
      pendingRef.current = markdown;

      if (saveTimerRef.current !== null) {
        window.clearTimeout(saveTimerRef.current);
      }

      saveTimerRef.current = window.setTimeout(() => {
        saveTimerRef.current = null;
        void flushPendingSave();
      }, SAVE_DEBOUNCE_MS);
    },
    [flushPendingSave],
  );

  // Stop saving the moment the window begins to close, so the delete that
  // follows can never be undone by a trailing write.
  useEffect(() => {
    let stop: (() => void) | null = null;
    let cancelled = false;

    // Effects re-run (React StrictMode does so deliberately in development), so
    // mounting a fresh watcher must clear the flag again. Without this the
    // cleanup below would leave the note permanently unable to save.
    closingRef.current = false;

    getCurrentWindow()
      .onCloseRequested(() => {
        closingRef.current = true;
      })
      .then((unlisten) => {
        if (cancelled) {
          unlisten();
          return;
        }
        stop = unlisten;
      })
      .catch((cause: unknown) => {
        logError(`could not watch the close of note ${noteId}`, cause);
      });

    return () => {
      cancelled = true;
      closingRef.current = true;
      stop?.();
    };
  }, [noteId]);

  // The app is quitting: save the newest text, then tell Rust this window is
  // done so the quit can go ahead.
  useEffect(() => {
    const unlisten = listen(EXIT_REQUESTED_EVENT, () => {
      void (async () => {
        try {
          await flushPendingSave();
        } catch (cause) {
          logError(`could not flush note ${noteId} before exit`, cause);
        } finally {
          try {
            await confirmExitFlush(noteId);
          } catch (cause) {
            logError(`could not confirm the exit flush for ${noteId}`, cause);
          }
        }
      })();
    });

    return () => {
      void unlisten.then((stop) => stop());
    };
  }, [flushPendingSave, noteId]);

  const [menuOpen, setMenuOpen] = useState(false);
  const [menuVisible, setMenuVisible] = useState(false);

  const closeMenu = useCallback(() => setMenuOpen(false), []);

  useEffect(() => {
    if (!menuOpen) {
      setMenuVisible(false);
      return;
    }
    setMenuVisible(true);
    const unlisten = getCurrentWindow().onCloseRequested(closeMenu);
    return () => {
      void unlisten.then((stop) => stop());
    };
  }, [closeMenu, menuOpen]);

  const toggleMenu = useCallback(() => {
    setMenuOpen((open) => !open);
  }, []);

  const handleCreateNote = useCallback(() => {
    createNote().catch((cause: unknown) => logError("could not create a note", cause));
    setMenuOpen(false);
  }, []);

  const handleOpenHarnessTasks = useCallback(() => {
    openHarnessTasksWindow().catch((cause: unknown) => logError("could not open harness tasks", cause));
    setMenuOpen(false);
  }, []);

  const handleOpenAdapters = useCallback(() => {
    openAdaptersWindow().catch((cause: unknown) => logError("could not open harness adapters", cause));
    setMenuOpen(false);
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
        <div className="note__add">
          <button
            type="button"
            className="note__button"
            onClick={toggleMenu}
            title="新建"
            aria-haspopup="menu"
            aria-expanded={menuOpen}
          >
            +
          </button>
          {menuVisible && (
            <div className="note__menu" role="menu">
              <button type="button" role="menuitem" onClick={handleCreateNote}>
                普通便签
              </button>
              <button type="button" role="menuitem" onClick={handleOpenHarnessTasks}>
                任务面板
              </button>
              <button type="button" role="menuitem" onClick={handleOpenAdapters}>
                适配器管理
              </button>
            </div>
          )}
        </div>

        <span className="note__status">{saveState === "error" ? "未保存" : ""}</span>

        <button
          type="button"
          className={pinned ? "note__button note__button--active" : "note__button"}
          onClick={handleTogglePin}
          title={pinned ? "取消置顶" : "置顶"}
          aria-pressed={pinned}
        >
          置顶
        </button>
      </div>

      {error ? (
        <p className="note__error">{error}</p>
      ) : loaded ? (
        <NoteEditor value={content} onChange={handleEditorChange} />
      ) : null}
    </div>
  );
}
