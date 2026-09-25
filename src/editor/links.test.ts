import { describe, expect, it } from "vitest";
import { isLinkOpenGesture, openableLinkUrl } from "./links";

describe("isLinkOpenGesture", () => {
  it("opens on ctrl+click and cmd+click", () => {
    expect(isLinkOpenGesture({ button: 0, ctrlKey: true, metaKey: false })).toBe(true);
    expect(isLinkOpenGesture({ button: 0, ctrlKey: false, metaKey: true })).toBe(true);
  });

  it("keeps a plain click for editing", () => {
    expect(isLinkOpenGesture({ button: 0, ctrlKey: false, metaKey: false })).toBe(false);
  });

  it("ignores modifier clicks that are not the primary button", () => {
    expect(isLinkOpenGesture({ button: 1, ctrlKey: true, metaKey: false })).toBe(false);
    expect(isLinkOpenGesture({ button: 2, ctrlKey: false, metaKey: true })).toBe(false);
  });
});

describe("openableLinkUrl", () => {
  it.each([
    ["https://openai.com", "https://openai.com/"],
    ["http://example.com/a", "http://example.com/a"],
    ["  https://openai.com/path  ", "https://openai.com/path"],
    ["mailto:notes@example.com", "mailto:notes@example.com"],
  ])("accepts %s", (href, opened) => {
    expect(openableLinkUrl(href)).toBe(opened);
  });

  it.each([
    "javascript:alert(1)",
    "file:///C:/secret.txt",
    "data:text/html,hi",
    "not a url",
    "",
    "   ",
    "/relative/path",
  ])("refuses %s", (href) => {
    expect(openableLinkUrl(href)).toBeNull();
  });
});
