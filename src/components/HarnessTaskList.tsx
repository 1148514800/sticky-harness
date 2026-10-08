import type { ActiveHarnessTask } from "../types/desktop";
import { groupByHarness } from "../harness/grouping";
import { formatElapsed, formatStatus } from "../harness/elapsed";

interface HarnessTaskListProps {
  /** Live active tasks, already ordered and filtered by Rust. */
  tasks: ActiveHarnessTask[];
  /** The moment to measure elapsed time against, so one render has one clock. */
  now: number;
}

/**
 * The body of the Harness Task Note: what is running, grouped by harness.
 *
 * Read-only by design. Every piece of state here comes from the registry, and
 * nothing on this surface can edit a task, so there is no input, no button and
 * no local copy that could drift from the source.
 *
 * The note shows only what the task is and how long it has been going. Task and
 * harness ids, timestamps and any future protocol metadata stay out: they are
 * not what the user is looking at, and showing them would turn a glanceable note
 * into a debug panel.
 */
export function HarnessTaskList({ tasks, now }: HarnessTaskListProps) {
  const groups = groupByHarness(tasks);

  if (groups.length === 0) {
    return <p className="harness__empty">当前没有运行中的任务</p>;
  }

  return (
    <ul className="harness__groups">
      {groups.map((group) => (
        <li className="harness__group" key={group.harnessId}>
          <p className="harness__harness">{group.harnessName}</p>
          <ul className="harness__tasks">
            {group.tasks.map((task) => (
              <li className="harness__task" key={task.task_id}>
                <p className="harness__title">{task.title}</p>
                <p className="harness__meta">
                  <span className={`harness__status harness__status--${task.status}`}>
                    {formatStatus(task.status)}
                  </span>
                  <span className="harness__dot" aria-hidden="true">
                    ·
                  </span>
                  <span className="harness__elapsed">{formatElapsed(task.started_at, now)}</span>
                </p>
                {task.message ? <p className="harness__message">{task.message}</p> : null}
              </li>
            ))}
          </ul>
        </li>
      ))}
    </ul>
  );
}
