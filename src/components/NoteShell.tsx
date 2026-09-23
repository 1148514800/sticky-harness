import type { ReactNode } from "react";

interface NoteShellProps {
  title: string;
  children: ReactNode;
}

/**
 * Phase 0 visual shell.
 *
 * Intentionally plain: a light surface, rounded corners and a small title.
 * Phase 3 owns real visual design; this only needs to look tidy enough to
 * confirm a window opened.
 */
export function NoteShell({ title, children }: NoteShellProps) {
  return (
    <section className="note-shell">
      <header className="note-shell__header">
        <h1 className="note-shell__title">{title}</h1>
      </header>
      <div className="note-shell__body">{children}</div>
    </section>
  );
}
