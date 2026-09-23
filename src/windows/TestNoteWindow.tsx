import { NoteShell } from "../components/NoteShell";

interface TestNoteWindowProps {
  label: string;
}

/**
 * Content for dynamically created windows.
 *
 * Each window renders standalone from its own label, so closing the main
 * window (or any other window) cannot break the remaining ones.
 */
export function TestNoteWindow({ label }: TestNoteWindowProps) {
  return (
    <NoteShell title="便签">
      <p className="note-shell__phase">Phase 0</p>
      <p className="note-shell__status">This is an independent test window.</p>
      <p className="note-shell__label">{label}</p>
    </NoteShell>
  );
}
