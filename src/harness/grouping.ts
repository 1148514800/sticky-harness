/**
 * Grouping for the Harness Task Note.
 *
 * The protocol allows one harness to run several tasks, so the note shows tasks
 * under their harness rather than as one flat list. This module only reshapes
 * what the registry already ordered: it never re-sorts, so the order the UI
 * shows is the order Core decided, and a refresh cannot make tasks jump around.
 */

import type { ActiveHarnessTask } from "../types/desktop";

/** One harness and the active tasks it currently has. */
export interface HarnessGroup {
  harnessId: string;
  harnessName: string;
  tasks: ActiveHarnessTask[];
}

/**
 * Group active tasks by harness, preserving the input order.
 *
 * Two properties matter and both come from not sorting here:
 *
 * - Harness order follows the registry's (harness id), so it is stable.
 * - Task order within a harness follows the registry's (start time, then id),
 *   so an older task stays above a newer one across refreshes.
 *
 * A harness appears once no matter how many tasks it has, which is what keeps
 * "how many harnesses are running" honest.
 */
export function groupByHarness(tasks: ActiveHarnessTask[]): HarnessGroup[] {
  const groups: HarnessGroup[] = [];
  const byId = new Map<string, HarnessGroup>();

  for (const task of tasks) {
    let group = byId.get(task.harness_id);
    if (!group) {
      group = {
        harnessId: task.harness_id,
        harnessName: task.harness_name,
        tasks: [],
      };
      byId.set(task.harness_id, group);
      groups.push(group);
    }
    group.tasks.push(task);
  }

  return groups;
}
