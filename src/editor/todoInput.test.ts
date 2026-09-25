import { describe, expect, it } from "vitest";
import { parseTodoMarker } from "./todoInput";

describe("parseTodoMarker", () => {
  it.each([
    ["[ ]", false],
    ["[x]", true],
    ["[X]", true],
    ["[]", false],
    ["- [ ]", false],
    ["- [x]", true],
    ["* [ ]", false],
    ["+ [x]", true],
  ])("reads %s as a marker with checked=%s", (text, checked) => {
    expect(parseTodoMarker(text)).toBe(checked);
  });

  it.each([
    "plain text",
    "[ ] trailing",
    "leading [ ]",
    "[y]",
    "- [ ] task text",
    "[  ]",
    "",
  ])("rejects %s", (text) => {
    expect(parseTodoMarker(text)).toBeNull();
  });
});