import { describe, expect, it } from "vitest";
import { groupByHarness } from "./grouping";
import type { ActiveHarnessTask } from "../types/desktop";

function task(
  harnessId: string,
  harnessName: string,
  taskId: string,
  startedAt = 1_700_000_000_000,
): ActiveHarnessTask {
  return {
    harness_id: harnessId,
    harness_name: harnessName,
    task_id: taskId,
    title: `Task ${taskId}`,
    status: "running",
    started_at: startedAt,
    updated_at: startedAt,
  };
}

describe("groupByHarness", () => {
  it("returns nothing for nothing", () => {
    expect(groupByHarness([])).toEqual([]);
  });

  it("keeps a single task under its harness", () => {
    const groups = groupByHarness([task("a", "Harness A", "t1")]);

    expect(groups).toHaveLength(1);
    expect(groups[0].harnessName).toBe("Harness A");
    expect(groups[0].tasks.map((entry) => entry.task_id)).toEqual(["t1"]);
  });

  it("puts two tasks of one harness in one group", () => {
    // 1 Harness = N tasks, which is the case the protocol insists on.
    const groups = groupByHarness([
      task("a", "Harness A", "t1", 1),
      task("a", "Harness A", "t2", 2),
    ]);

    expect(groups).toHaveLength(1);
    expect(groups[0].tasks.map((entry) => entry.task_id)).toEqual(["t1", "t2"]);
  });

  it("separates two harnesses even when their tasks interleave", () => {
    const groups = groupByHarness([
      task("a", "Harness A", "t1", 1),
      task("b", "Harness B", "t2", 2),
      task("a", "Harness A", "t3", 3),
    ]);

    expect(groups.map((group) => group.harnessId)).toEqual(["a", "b"]);
    expect(groups[0].tasks.map((entry) => entry.task_id)).toEqual(["t1", "t3"]);
    expect(groups[1].tasks.map((entry) => entry.task_id)).toEqual(["t2"]);
  });

  it("preserves the order the registry chose", () => {
    // Core sorts by harness id, then start time; grouping must not undo that.
    const groups = groupByHarness([
      task("alpha", "Alpha", "early", 1),
      task("alpha", "Alpha", "late", 9),
      task("beta", "Beta", "only", 5),
    ]);

    expect(groups.map((group) => group.harnessId)).toEqual(["alpha", "beta"]);
    expect(groups[0].tasks.map((entry) => entry.task_id)).toEqual(["early", "late"]);
  });

  it("is stable across repeated calls with the same input", () => {
    const tasks = [
      task("a", "Harness A", "t1", 1),
      task("b", "Harness B", "t2", 2),
    ];

    expect(groupByHarness(tasks)).toEqual(groupByHarness(tasks));
  });

  it("uses the harness name from the task, so the group reads correctly", () => {
    const groups = groupByHarness([task("a", "Codex Local", "t1")]);

    expect(groups[0].harnessName).toBe("Codex Local");
  });
});
