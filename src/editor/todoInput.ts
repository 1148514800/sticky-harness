import { Extension } from "@tiptap/core";
import { Plugin } from "@tiptap/pm/state";

/**
 * A complete todo marker at the end of an already-converted list item.
 *
 * Inside a list the bullet is usually added by the editor, but a user typing
 * `- [ ] ` out of habit adds a redundant one, so it is allowed here too.
 */
const TODO_MARKER = /^(?:[-+*]\s)?\[([ xX]?)\]$/;

/**
 * Read a Markdown todo marker.
 *
 * Returns the checked state, or `null` when the text is not a bare marker.
 * Exported so the rule can be unit tested without a live editor.
 */
export function parseTodoMarker(text: string): boolean | null {
  const match = TODO_MARKER.exec(text);
  if (!match) return null;
  return match[1].toLowerCase() === "x";
}

/**
 * Lets `- [ ] ` typed in a note become a real todo.
 *
 * The task list extension ships an input rule for `[ ] `, but the bullet list
 * rule for `- ` runs first and has already turned the line into a bullet whose
 * text is the literal `[ ] `. Wrapping a paragraph that sits inside a list item
 * into a task item is not a valid transform, so the built-in rule declines and
 * the marker stays as text.
 *
 * This plugin closes that gap. Once the marker is complete it removes the
 * literal text and hands over to Tiptap's own commands, so the conversion is
 * the same one the toolbar and keyboard shortcuts use:
 *
 * - a bullet list becomes a task list,
 * - inside a task list the redundant marker is dropped and its `x` honoured.
 */
export const TodoInput = Extension.create({
  name: "todoInput",

  addProseMirrorPlugins() {
    const editor = this.editor;

    return [
      new Plugin({
        props: {
          handleTextInput(view, from, to, text) {
            // A marker only counts once its trailing space is typed, which is
            // also the moment the built-in input rules have finished declining.
            if (text !== " ") return false;

            const { state } = view;
            if (!state.selection.empty) return false;

            const { $from } = state.selection;
            if ($from.parent.type.name !== "paragraph") return false;

            // Inside a list a paragraph is paragraph -> listItem -> list.
            const listItem = $from.node($from.depth - 1);
            const list = $from.node($from.depth - 2);
            if (!listItem || !list) return false;

            const checked = parseTodoMarker($from.parent.textBetween(0, $from.parentOffset));
            if (checked === null) return false;

            const markerStart = from - $from.parentOffset;

            if (listItem.type.name === "listItem" && list.type.name === "bulletList") {
              view.dispatch(state.tr.delete(markerStart, to));
              editor
                .chain()
                .focus()
                .toggleTaskList()
                .updateAttributes("taskItem", { checked })
                .run();
              // The space is consumed: it only existed to complete the marker.
              return true;
            }

            if (listItem.type.name === "taskItem" && list.type.name === "taskList") {
              // Already a todo, so only the redundant marker is removed.
              view.dispatch(state.tr.delete(markerStart, to));
              editor.chain().focus().updateAttributes("taskItem", { checked }).run();
              return true;
            }

            return false;
          },
        },
      }),
    ];
  },
});