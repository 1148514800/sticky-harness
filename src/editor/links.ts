/**
 * Decide when a Markdown link should leave the note.
 *
 * The editor is always editable, so a plain click has to move the caret.
 * Ctrl+click (Cmd+click elsewhere) is the gesture that opens the link.
 */

/** True when this click asks to open a link rather than edit it. */
export function isLinkOpenGesture(event: {
  button: number;
  ctrlKey: boolean;
  metaKey: boolean;
}): boolean {
  return event.button === 0 && (event.ctrlKey || event.metaKey);
}

const OPENABLE_PROTOCOLS = new Set(["http:", "https:", "mailto:"]);

/**
 * Return a URL the OS may open, or null when the link must stay inert.
 *
 * Note text is free-form Markdown, so a link can name any scheme. Only web
 * and mail links are handed to the opener plugin; `javascript:`, `file:` and
 * anything that does not parse are refused.
 */
export function openableLinkUrl(href: string): string | null {
  let url: URL;
  try {
    url = new URL(href.trim());
  } catch {
    return null;
  }

  if (!OPENABLE_PROTOCOLS.has(url.protocol)) return null;
  return url.href;
}
