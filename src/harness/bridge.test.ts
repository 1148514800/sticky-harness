import { describe, expect, it } from "vitest";

import {
  BRIDGE_ENDPOINT,
  applyUpdate,
  helpText,
  isFinished,
  isSession,
  parseArgs,
  resolveStatus,
  slug,
  startSession,
  toSnapshot,
} from "./bridge";
import type { BridgeOptions } from "./bridge";

/** A started session, the baseline for most of these tests. */
function session(overrides = {}) {
  return {
    ...startSession({
      harness: "codex",
      task: "Refactor runtime",
      now: 1_700_000_000_000,
    }),
    ...overrides,
  };
}

/**
 * Parse a command line that is expected to be valid.
 *
 * `parseArgs` answers a bad line with a message, so a test that goes on to use
 * the result has to say which half it expected. Throwing here keeps that
 * assertion in the one test that cares instead of narrowing at every call.
 */
function parsed(argv: string[]): BridgeOptions {
  const result = parseArgs(argv);
  if (typeof result === "string") {
    throw new Error(`expected options, got: ${result}`);
  }
  return result;
}

/**
 * Parse a `start` command line, requiring the two arguments `start` mandates.
 *
 * `start` without a harness or a task is rejected at runtime; this says so in
 * the type as well, so a test does not have to re-check what it just passed.
 */
function parsedStart(argv: string[]): BridgeOptions & { harnessId: string; task: string } {
  const options = parsed(argv);
  if (!options.harnessId || !options.task) {
    throw new Error("expected a start command with --harness and --task");
  }
  return options as BridgeOptions & { harnessId: string; task: string };
}

describe("parseArgs", () => {
  it("reads a start command", () => {
    const options = parseArgs(["start", "--harness", "codex", "--task", "Refactor runtime"]);

    expect(options).toMatchObject({
      command: "start",
      harnessId: "codex",
      task: "Refactor runtime",
      endpoint: BRIDGE_ENDPOINT,
      timeoutMs: 2000,
      json: false,
      dryRun: false,
    });
  });

  it("applies the default port and both overrides", () => {
    expect(parseArgs(["update", "--message", "x"])).toMatchObject({ endpoint: BRIDGE_ENDPOINT });

    expect(parseArgs(["update", "--message", "x", "--port", "18001"])).toMatchObject({
      endpoint: "http://127.0.0.1:18001/api/harness/snapshot",
    });

    expect(
      parseArgs(["update", "--message", "x", "--endpoint", "http://127.0.0.1:9/api/harness/snapshot"]),
    ).toMatchObject({ endpoint: "http://127.0.0.1:9/api/harness/snapshot" });
  });

  it("refuses --port and --endpoint together", () => {
    const problem = parseArgs([
      "update",
      "--message",
      "x",
      "--port",
      "18001",
      "--endpoint",
      "http://127.0.0.1:9/api/harness/snapshot",
    ]);

    expect(typeof problem).toBe("string");
    expect(problem).toContain("not both");
  });

  it("requires a harness and a task to start", () => {
    expect(parseArgs(["start", "--task", "x"])).toContain("--harness");
    expect(parseArgs(["start", "--harness", "codex"])).toContain("--task");
  });

  it("refuses start-only flags on the other commands", () => {
    // Otherwise `update --task "something else"` would look like it renamed the
    // task while actually being ignored, or worse, silently started a new one.
    expect(parseArgs(["update", "--task", "other", "--message", "x"])).toContain("only applies to start");
    expect(parseArgs(["done", "--harness", "codex"])).toContain("only applies to start");
    expect(parseArgs(["done", "--name", "Codex"])).toContain("only applies to start");
  });

  it("refuses a status on a finishing command", () => {
    expect(parseArgs(["done", "--status", "failed"])).toContain("already reports a final status");
  });

  it("requires something to update", () => {
    expect(parseArgs(["update"])).toContain("at least one of");
  });

  it("accepts an update with only some fields", () => {
    expect(parseArgs(["update", "--message", "Running tests"])).toMatchObject({
      command: "update",
      message: "Running tests",
    });
    expect(parseArgs(["update", "--status", "waiting"])).toMatchObject({ status: "waiting" });
    expect(parseArgs(["update", "--title", "New name"])).toMatchObject({ title: "New name" });
  });

  it("reports an unknown command, flag and status by name", () => {
    expect(parseArgs(["pause"])).toContain('unknown command "pause"');
    expect(parseArgs(["update", "--message", "x", "--wat"])).toContain('unknown option "--wat"');
    expect(parseArgs(["update", "--message", "x", "--status", "sleeping"])).toContain(
      'unknown status "sleeping"',
    );
  });

  it("reports a flag that is missing its value", () => {
    expect(parseArgs(["start", "--harness"])).toContain("needs a value");
    expect(parseArgs(["update", "--message"])).toContain("needs a value");
  });

  it("validates the port, the timeout and reads --json/--dry-run", () => {
    expect(parseArgs(["update", "--message", "x", "--port", "0"])).toContain("between 1 and 65535");
    expect(parseArgs(["update", "--message", "x", "--port", "70000"])).toContain("between 1 and 65535");
    expect(parseArgs(["update", "--message", "x", "--timeout", "-5"])).toContain("positive number");
    expect(parseArgs(["update", "--message", "x", "--timeout", "abc"])).toContain("positive number");
    expect(parseArgs(["update", "--message", "x", "--json"])).toMatchObject({ json: true });
    expect(parseArgs(["update", "--message", "x", "--dry-run"])).toMatchObject({ dryRun: true });
  });

  it("treats a bare invocation and --help as help", () => {
    expect(parseArgs([])).toBe("help");
    expect(parseArgs(["--help"])).toBe("help");
    expect(parseArgs(["start", "--harness", "codex", "--task", "x", "--help"])).toBe("help");
  });

  it("accepts a message that begins with a dash", () => {
    // A flag parser that guessed "the next token looks like a flag" would eat
    // this message and fail on an unrelated missing value.
    expect(parseArgs(["update", "--message", "- running tests now"])).toMatchObject({
      message: "- running tests now",
    });
  });

  it("keeps an empty message, which means clear it", () => {
    expect(parseArgs(["update", "--message", ""])).toMatchObject({ message: "" });
  });

  it("reads --name and --task-id for start", () => {
    expect(
      parseArgs(["start", "--harness", "codex", "--name", "Codex", "--task", "x", "--task-id", "t-1"]),
    ).toMatchObject({ harnessId: "codex", harnessName: "Codex", taskId: "t-1" });
  });

  it("accepts --harness-id as a spelling of --harness", () => {
    expect(parseArgs(["start", "--harness-id", "deepseek", "--task", "x"])).toMatchObject({
      harnessId: "deepseek",
    });
  });

  it("reads --state-dir", () => {
    expect(parseArgs(["update", "--message", "x", "--state-dir", "C:/tmp/bridge"])).toMatchObject({
      stateDir: "C:/tmp/bridge",
    });
  });
});

describe("resolveStatus", () => {
  it("accepts the protocol spelling and the aliases people type", () => {
    expect(resolveStatus("running")).toEqual({ status: "running" });
    expect(resolveStatus("waiting")).toEqual({ status: "waiting" });
    expect(resolveStatus("done")).toEqual({ status: "completed" });
    expect(resolveStatus("fail")).toEqual({ status: "failed" });
    expect(resolveStatus("cancel")).toEqual({ status: "cancelled" });
    expect(resolveStatus("  Canceled  ")).toEqual({ status: "cancelled" });
  });

  it("names the value when it cannot be understood", () => {
    // A valid status is itself a string, so the result has to be discriminated:
    // a caller testing `typeof === "string"` would reject every correct status.
    expect(resolveStatus("sleeping")).toMatchObject({ error: expect.stringContaining('"sleeping"') });
    expect(resolveStatus("")).toHaveProperty("error");
  });

  it("knows which statuses are over", () => {
    expect(isFinished("running")).toBe(false);
    expect(isFinished("waiting")).toBe(false);
    expect(isFinished("completed")).toBe(true);
    expect(isFinished("failed")).toBe(true);
    expect(isFinished("cancelled")).toBe(true);
  });
});

describe("slug", () => {
  it("reduces a title to a stable id", () => {
    expect(slug("Refactor runtime")).toBe("refactor-runtime");
    expect(slug("  Fix: broken/thing!! ")).toBe("fix-broken-thing");
  });

  it("is stable for a label that reduces to nothing", () => {
    // An empty id would be rejected by the protocol and a shared one would
    // collide, so a non-Latin label falls back to a hash of itself.
    const first = slug("重构运行时");
    expect(first).toMatch(/^id-[0-9a-f]{8}$/);
    expect(slug("重构运行时")).toBe(first);
    expect(slug("别的任务")).not.toBe(first);
  });

  it("caps the length", () => {
    expect(slug("a".repeat(200))).toHaveLength(64);
  });
});

describe("startSession", () => {
  it("derives the id from the title and defaults the name to the harness", () => {
    const started = startSession({ harness: "codex", task: "Refactor runtime", now: 42 });

    expect(started).toMatchObject({
      harnessId: "codex",
      harnessName: "codex",
      taskId: "refactor-runtime",
      title: "Refactor runtime",
      startedAt: 42,
      status: "running",
    });
    expect(started.message).toBeUndefined();
  });

  it("keeps a pinned task id and an explicit name", () => {
    const started = startSession({
      harness: "codex",
      harnessName: "Codex CLI",
      task: "Refactor runtime",
      taskId: "task-7",
      now: 42,
    });

    expect(started.taskId).toBe("task-7");
    expect(started.harnessName).toBe("Codex CLI");
  });

  it("can begin a task already waiting", () => {
    // A harness that starts a task blocked on something should not have to
    // report running first and correct itself a moment later.
    const started = startSession({ harness: "codex", task: "x", status: "waiting", now: 1 });
    expect(started.status).toBe("waiting");
  });

  it("carries a start status through to the snapshot", () => {
    const options = parsedStart(["start", "--harness", "codex", "--task", "x", "--status", "waiting"]);
    expect(options).toMatchObject({ command: "start", status: "waiting" });

    const started = startSession({
      harness: options.harnessId,
      task: options.task,
      status: options.status,
      now: 7,
    });
    expect(toSnapshot(started, 8).tasks[0].status).toBe("waiting");
  });

  it("does not invent a message from an empty one", () => {
    expect(startSession({ harness: "codex", task: "x", message: "   ", now: 1 }).message).toBeUndefined();
  });
});

describe("applyUpdate", () => {
  it("changes only what was passed", () => {
    const before = session({ status: "waiting", message: "Waiting for review" });
    const after = applyUpdate(before, { message: "Running tests" });

    expect(after.status).toBe("waiting");
    expect(after.message).toBe("Running tests");
    expect(after.startedAt).toBe(before.startedAt);
    expect(after.taskId).toBe(before.taskId);
  });

  it("can rename the task without moving it", () => {
    const after = applyUpdate(session(), { title: "  New title  " });
    expect(after.title).toBe("New title");
    expect(after.taskId).toBe("refactor-runtime");
  });

  it("ignores an empty title rather than blanking it", () => {
    expect(applyUpdate(session(), { title: "   " }).title).toBe("Refactor runtime");
  });

  it("refuses an empty --title outright", () => {
    // Clearing a title would leave a row in the window with nothing to show.
    expect(parseArgs(["update", "--title", "  "])).toContain("--title must not be empty");
  });

  it("clears the message when given an empty one", () => {
    const withMessage = session({ message: "Running tests" });
    expect(applyUpdate(withMessage, { message: "" }).message).toBeUndefined();
  });

  it("leaves the message alone when it was not mentioned", () => {
    const withMessage = session({ message: "Running tests" });
    expect(applyUpdate(withMessage, { status: "waiting" }).message).toBe("Running tests");
  });
});

describe("toSnapshot", () => {
  it("is one task with the protocol's field names", () => {
    const started = startSession({ harness: "codex", task: "Refactor runtime", now: 1000 });
    const snapshot = toSnapshot(started, 2000);

    expect(snapshot).toEqual({
      harness_id: "codex",
      harness_name: "codex",
      source: { type: "push", name: "sticky-harness-bridge" },
      updated_at: 2000,
      tasks: [
        {
          task_id: "refactor-runtime",
          title: "Refactor runtime",
          status: "running",
          started_at: 1000,
          updated_at: 2000,
        },
      ],
    });
  });

  it("keeps the task id across a whole lifecycle", () => {
    // The single most important property for the window: one task, updated,
    // never a second row.
    const started = startSession({ harness: "codex", task: "Refactor runtime", now: 1000 });
    const waiting = applyUpdate(started, { status: "waiting" });
    const finished = applyUpdate(waiting, { status: "completed" });

    const ids = [started, waiting, finished].map((s) => toSnapshot(s, 3000).tasks[0].task_id);
    expect(new Set(ids).size).toBe(1);
    expect(toSnapshot(finished, 3000).tasks[0].status).toBe("completed");
  });

  it("never moves updated_at backwards when the clock steps back", () => {
    // The protocol rejects a task that finished before it started, so a clock
    // that steps back mid-task must not turn a correct bridge into a bug.
    const started = startSession({ harness: "codex", task: "x", now: 5000 });
    const snapshot = toSnapshot(started, 4000);

    expect(snapshot.updated_at).toBe(5000);
    expect(snapshot.tasks[0].updated_at).toBe(5000);
  });

  it("carries the message only when there is one", () => {
    const started = startSession({ harness: "codex", task: "x", message: "Running tests", now: 1 });
    expect(toSnapshot(started, 2).tasks[0].message).toBe("Running tests");
    expect(toSnapshot(startSession({ harness: "codex", task: "x", now: 1 }), 2).tasks[0].message).toBeUndefined();
  });

  it("carries the harness name the producer chose", () => {
    const started = startSession({ harness: "codex", harnessName: "Codex CLI", task: "x", now: 1 });
    expect(toSnapshot(started, 2).harness_name).toBe("Codex CLI");
    expect(toSnapshot(started, 2).harness_id).toBe("codex");
  });
});

describe("isSession", () => {
  it("accepts a session this version wrote", () => {
    expect(isSession(session())).toBe(true);
    expect(isSession(session({ message: "Running tests" }))).toBe(true);
  });

  it("rejects anything it cannot fully trust", () => {
    // A half-written or hand-edited file must not make the bridge report a task
    // with no id, so it counts as "no current task" instead.
    expect(isSession(undefined)).toBe(false);
    expect(isSession(null)).toBe(false);
    expect(isSession("start")).toBe(false);
    expect(isSession({})).toBe(false);
    expect(isSession({ ...session(), taskId: "" })).toBe(false);
    expect(isSession({ ...session(), harnessId: "" })).toBe(false);
    expect(isSession({ ...session(), title: "" })).toBe(false);
    expect(isSession({ ...session(), startedAt: "yesterday" })).toBe(false);
    expect(isSession({ ...session(), message: 7 })).toBe(false);
  });
});

describe("helpText", () => {
  it("documents every command and the endpoint flag", () => {
    const text = helpText("sticky-harness-bridge");

    for (const command of ["start", "update", "done", "fail", "cancel"]) {
      expect(text).toContain(command);
    }
    expect(text).toContain("--harness");
    expect(text).toContain("--port");
  });
});
