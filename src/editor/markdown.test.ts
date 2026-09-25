import { describe, expect, it } from "vitest";
import { docToMarkdown, markdownToDoc } from "./markdown";

/** Markdown -> editor document -> Markdown. */
function roundTrip(markdown: string): string {
  return docToMarkdown(markdownToDoc(markdown));
}

/** Collect the node and mark types present in a document, for structure checks. */
function nodeTypes(value: unknown, found = new Set<string>()): Set<string> {
  if (Array.isArray(value)) {
    for (const entry of value) nodeTypes(entry, found);
    return found;
  }
  if (value === null || typeof value !== "object") return found;

  const record = value as Record<string, unknown>;
  if (typeof record.type === "string") found.add(record.type);

  for (const entry of Object.values(record)) nodeTypes(entry, found);
  return found;
}

describe("markdown round trip", () => {
  const cases: Array<[string, string]> = [
    ["plain text", "hello world"],
    ["multiple paragraphs", "line one\n\nline two"],
    ["soft line break", "line one\nline two"],
    ["h1", "# Heading 1"],
    ["h2", "## Heading 2"],
    ["h3", "### Heading 3"],
    ["bold", "This is **bold** text."],
    ["italic", "This is *italic* text."],
    ["strikethrough", "This is ~~struck~~ text."],
    ["bullet list", "- alpha\n- beta"],
    ["ordered list", "1. first\n2. second"],
    ["ordered list with start offset", "5. five\n6. six"],
    ["inline code", "Use `python train.py` now."],
    ["code block", "```\ncode block\n```"],
    ["code block with language", "```python\nprint(1)\n```"],
    ["blockquote", "> a quote"],
    ["multi-line blockquote", "> quote line 1\n> quote line 2"],
    ["link", "[OpenAI](https://openai.com)"],
    ["task list", "- [ ] Task A\n- [x] Task B"],
    ["cjk text", "训练备注\n\nloss 已经开始下降"],
    ["cjk task list", "- [ ] 检查训练\n- [x] 修改配置"],
  ];

  it.each(cases)("preserves %s", (_name, markdown) => {
    expect(roundTrip(markdown)).toBe(markdown);
  });

  it("treats empty content as empty", () => {
    expect(roundTrip("")).toBe("");
  });

  it("keeps a document without trailing newline stable", () => {
    // Trailing whitespace is not meaningful Markdown, but it must settle.
    const once = roundTrip("text with trailing spaces\n");
    expect(roundTrip(once)).toBe(once);
  });
});

describe("plain text stays plain", () => {
  /**
   * A note is stored as Markdown source, so text typed without any syntax must
   * be written back exactly as typed. Stray escapes would still render, but
   * they would stop the JSON file from matching what the user typed.
   */
  it.each([
    "FINAL_TOKEN_123456",
    "snake_case_here",
    "a_b_c",
    "100%",
    "plain sentence with punctuation: yes!",
  ])("keeps %s free of unnecessary escapes", (markdown) => {
    expect(roundTrip(markdown)).toBe(markdown);
  });

  it("still escapes an underscore that really opens emphasis", () => {
    // A leading underscore could be read as emphasis, so that one stays
    // escaped and the document still round-trips.
    expect(roundTrip("\\_leading underscore")).toBe("\\_leading underscore");
  });

  it("keeps the double underscore of a word stable across saves", () => {
    // The first pass drops the redundant escape and keeps the second, which is
    // the one that stops the pair from reading as bold. Saving again must not
    // keep eating escapes, so the second pass is the fixed point.
    const once = roundTrip("a__b");
    expect(once).toBe("a_\\_b");
    expect(roundTrip(once)).toBe(once);
  });

  it("leaves emphasis intact", () => {
    expect(roundTrip("plain _em_ text")).toBe("plain *em* text");
    expect(roundTrip("__strong__ text")).toBe("**strong** text");
  });
});

describe("markdown parsing shape", () => {
  it("maps Phase 1 plain text onto paragraphs, not a code block", () => {
    const types = nodeTypes(markdownToDoc("hello world"));
    expect(types.has("paragraph")).toBe(true);
    expect(types.has("codeBlock")).toBe(false);
  });

  it("builds real heading nodes", () => {
    const doc = markdownToDoc("# 今日任务");
    const heading = JSON.stringify(doc);
    expect(heading).toContain("\"heading\"");
    expect(heading).toContain("\"level\":1");
  });

  it("builds task items with a checked attribute", () => {
    const doc = markdownToDoc("- [ ] Task A\n- [x] Task B");
    const items = JSON.stringify(doc);
    expect(items).toContain("\"taskList\"");
    expect(items).toContain("\"taskItem\"");
    expect(items).toContain("\"checked\":false");
    expect(items).toContain("\"checked\":true");
  });

  it("builds bold, italic, strike, code and link marks", () => {
    const doc = markdownToDoc(
      "**b** *i* ~~s~~ `c` [l](https://openai.com)",
    );
    const text = JSON.stringify(doc);
    for (const mark of ["bold", "italic", "strike", "code", "link"]) {
      expect(text).toContain(`"${mark}"`);
    }
  });

  it("keeps a quote as a blockquote rather than a plain paragraph", () => {
    const types = nodeTypes(markdownToDoc("> a quote"));
    expect(types.has("blockquote")).toBe(true);
  });
});

describe("todo interaction", () => {
  it("writes a flipped checkbox back into the Markdown", () => {
    const doc = markdownToDoc("- [ ] Task A\n- [x] Task B");

    // Flip the first task's checkbox the way the editor does, by mutating the
    // node attribute and serialising again.
    const taskList = (doc.content as Array<Record<string, unknown>>).find(
      (node) => node.type === "taskList",
    );
    const items = taskList?.content as Array<Record<string, unknown>>;
    (items[0].attrs as Record<string, unknown>).checked = true;

    expect(docToMarkdown(doc)).toBe("- [x] Task A\n- [x] Task B");
  });

  it("can uncheck a completed task", () => {
    const doc = markdownToDoc("- [x] Task B");
    const taskList = (doc.content as Array<Record<string, unknown>>).find(
      (node) => node.type === "taskList",
    );
    const items = taskList?.content as Array<Record<string, unknown>>;
    (items[0].attrs as Record<string, unknown>).checked = false;

    expect(docToMarkdown(doc)).toBe("- [ ] Task B");
  });
});