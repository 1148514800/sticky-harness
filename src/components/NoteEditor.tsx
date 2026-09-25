import { useRef } from "react";
import type { JSONContent } from "@tiptap/core";
import { EditorContent, useEditor } from "@tiptap/react";
import { EDITOR_EXTENSIONS, docToMarkdown, markdownToDoc } from "../editor/markdown";
import { isLinkOpenGesture, openableLinkUrl } from "../editor/links";
import { openExternalUrl } from "../services/desktop";
import { logError } from "../utils/logger";

interface NoteEditorProps {
  /** Markdown loaded from disk. Applied once, when this editor is created. */
  value: string;
  /** Emitted with freshly serialised Markdown on every user edit. */
  onChange: (markdown: string) => void;
}

/**
 * The WYSIWYG note editor.
 *
 * There is no edit/preview switch: the document is always directly editable and
 * Markdown input rules convert syntax as it is typed.
 *
 * Markdown stays the storage format. The editor owns a rich document and this
 * component translates at the boundary, so autosave keeps working on the same
 * Markdown string that Phase 1 wrote.
 *
 * Mount this only after the note has loaded. The initial document is taken
 * once, so a later parent render cannot put the caret back to the start.
 */
export function NoteEditor({ value, onChange }: NoteEditorProps) {
  const onChangeRef = useRef(onChange);
  onChangeRef.current = onChange;

  const editor = useEditor({
    extensions: EDITOR_EXTENSIONS,
    content: markdownToDoc(value) as JSONContent,
    autofocus: "start",
    immediatelyRender: true,
    editorProps: {
      attributes: {
        class: "note__content",
        spellcheck: "false",
      },
      handleDOMEvents: {
        click: (view, event) => {
          const href = hrefFromClick(event.target, view.dom);
          if (href === null) return false;

          // An <a href> inside the webview would otherwise navigate this note
          // away. Editing stays in the note; opening is an explicit gesture.
          event.preventDefault();
          if (!isLinkOpenGesture(event)) return false;

          const url = openableLinkUrl(href);
          if (url !== null) {
            openExternalUrl(url).catch((cause: unknown) => {
              logError(`could not open ${url}`, cause);
            });
          }
          return true;
        },
      },
    },
    onUpdate: ({ editor: instance }) => {
      onChangeRef.current(docToMarkdown(instance.getJSON()));
    },
  });

  return <EditorContent editor={editor} className="note__editor-host" />;
}

/** The link under a click, or null when the click is ordinary note text. */
function hrefFromClick(target: EventTarget | null, root: HTMLElement): string | null {
  if (!(target instanceof Node) || !root.contains(target)) return null;

  const element = target instanceof Element ? target : target.parentElement;
  const anchor = element?.closest("a") ?? null;
  if (anchor === null || !root.contains(anchor)) return null;
  return anchor.getAttribute("href");
}
