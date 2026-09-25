import { MarkdownManager } from "@tiptap/markdown";
import { StarterKit } from "@tiptap/starter-kit";
import { TaskItem, TaskList } from "@tiptap/extension-list";
import { Placeholder } from "@tiptap/extension-placeholder";
import type { AnyExtension } from "@tiptap/core";
import { TodoInput } from "./todoInput";

/**
 * The Markdown <-> editor document boundary.
 *
 * `NoteRecord.content` is always Markdown text, but the editor works on a rich
 * document. Every conversion between the two lives here instead of being spread
 * across components, so both directions stay consistent and testable.
 *
 * The `MarkdownManager` is pure: it turns a Markdown string into a Tiptap JSON
 * document and back with no DOM and no editor instance involved. Because it is
 * driven by the same extension list as the editor, the conversion is exactly
 * what a live editor would produce.
 */

/**
 * The document schema: the nodes and marks a note can contain.
 *
 * `StarterKit` covers paragraphs, H1-H3, bold, italic, strike, inline code,
 * code blocks, blockquotes, bullet/ordered lists, links and hard breaks.
 * `TaskList`/`TaskItem` add the clickable todos.
 *
 * This is deliberately a superset-free, shared list: the editor and the
 * converter both use it, so nothing can render in the editor yet fail to
 * serialise. Keep the scope small — tables, images, maths and nested layouts
 * are out of scope for this phase.
 */
export const NOTE_EXTENSIONS: AnyExtension[] = [
  StarterKit.configure({
    // A sticky note only needs three heading levels; deeper ones are out of scope.
    heading: { levels: [1, 2, 3] },
    link: {
      // A plain click places the caret. Ctrl+click opens the link through the
      // opener plugin; this stays off so the webview itself never navigates.
      openOnClick: false,
      autolink: true,
      // Typing or pasting [label](url) becomes a real link, same as the other
      // Markdown input rules.
      markdownLinks: true,
      HTMLAttributes: {
        rel: "noopener noreferrer nofollow",
        title: "Ctrl+click to open",
      },
    },
  }),
  TaskList,
  TaskItem.configure({ nested: true }),
];

/**
 * The editor's extension list: the schema plus plugins that only affect
 * editing, such as the empty-note placeholder and the todo input rule fix.
 * These add no document content, so the converter does not need them.
 */
export const EDITOR_EXTENSIONS: AnyExtension[] = [
  ...NOTE_EXTENSIONS,
  TodoInput,
  Placeholder.configure({ placeholder: "Type here…" }),
];

/**
 * Convert stored Markdown into the editor's document shape.
 *
 * Plain text is valid Markdown, so Phase 1 content loads with no migration.
 */
export function markdownToDoc(markdown: string): Record<string, unknown> {
  return markdownManager().parse(markdown) as unknown as Record<string, unknown>;
}

/** Serialise an editor document back into the Markdown that gets persisted. */
export function docToMarkdown(doc: unknown): string {
  return markdownManager().serialize(doc as never);
}

let cached: MarkdownManager | null = null;

/** Build the shared manager once; constructing it reads the extension list. */
function markdownManager(): MarkdownManager {
  cached ??= new MarkdownManager({ extensions: NOTE_EXTENSIONS });
  return cached;
}