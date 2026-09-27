/**
 * The harness bridge: the shared logic behind the `sticky-harness-bridge` CLI.
 *
 * A bridge is a producer, not an integration. It turns "this task is running"
 * into the same `HarnessSnapshot` a HTTP push would carry, so Codex, DeepSeek
 * and a home-grown harness all reach the Harness Task Note through one path and
 * one protocol. There is no per-vendor code here on purpose: a vendor-specific
 * bridge would mean the protocol had to learn about vendors, which is exactly
 * what the protocol exists to avoid.
 *
 * Everything in this file is pure - values in, values out, no I/O, no clock
 * reads, no argv access. The CLI does the reading, writing and posting, which is
 * what makes the interesting decisions directly testable.
 *
 * The bridge only ever *reports*. It never starts, stops, prompts or inspects a
 * harness, and it never reads anything the harness wrote.
 */

/** Where a push normally goes. Loopback only, like the endpoint itself. */
export const BRIDGE_ENDPOINT = "http://127.0.0.1:17899/api/harness/snapshot";

/** The port the app's push endpoint listens on unless it was changed. */
export const BRIDGE_PORT = 17899;

/** How long the bridge waits for the app before giving up, in milliseconds. */
export const BRIDGE_TIMEOUT_MS = 2000;

/** The file name the bridge keeps its own session in. */
export const BRIDGE_SESSION_FILE = "bridge-session.json";

/** The `source.name` every bridge snapshot carries. */
export const BRIDGE_SOURCE_NAME = "sticky-harness-bridge";

/**
 * The commands the CLI accepts.
 *
 * `start` is the only one that can begin a task; the rest act on the session
 * that `start` wrote, which is what keeps a task's id stable across calls.
 */
export type BridgeCommand = "start" | "update" | "done" | "fail" | "cancel";

/**
 * The status the bridge can report.
 *
 * A subset of the protocol's statuses: `unknown` is deliberately absent,
 * because a bridge always knows what it is being told. The four terminal states
 * the protocol allows are collapsed to the three the CLI exposes - `done` means
 * completed - so there is exactly one spelling per intent.
 */
export type BridgeStatus = "running" | "waiting" | "completed" | "failed" | "cancelled";

/** Every status the CLI will accept, including the aliases users will type. */
const STATUS_ALIASES: Record<string, BridgeStatus> = {
  running: "running",
  start: "running",
  started: "running",
  waiting: "waiting",
  wait: "waiting",
  blocked: "waiting",
  completed: "completed",
  complete: "completed",
  done: "completed",
  finished: "completed",
  success: "completed",
  succeeded: "completed",
  failed: "failed",
  fail: "failed",
  error: "failed",
  cancelled: "cancelled",
  canceled: "cancelled",
  cancel: "cancelled",
};

/** The command that finishes a task, and the status it reports. */
const FINISH_COMMANDS: Partial<Record<BridgeCommand, BridgeStatus>> = {
  done: "completed",
  fail: "failed",
  cancel: "cancelled",
};

/** What the bridge remembers between calls. */
export interface BridgeSession {
  /** The harness the task belongs to, as the app keys its snapshots. */
  harnessId: string;
  /** The name shown in the Harness Tasks window. */
  harnessName: string;
  /** The task's stable id. Derived once, at `start`, then never recomputed. */
  taskId: string;
  /** The task title. Renamable through `update --title`. */
  title: string;
  /** When the task began, in Unix milliseconds. Fixed at `start`. */
  startedAt: number;
  /** The last status reported. Remembered so an update cannot silently reset it. */
  status: BridgeStatus;
  /** The last message reported, if any. */
  message?: string;
}

/** One task, in the protocol's own shape. */
export interface BridgeTask {
  task_id: string;
  title: string;
  status: BridgeStatus;
  started_at: number;
  updated_at: number;
  message?: string;
}

/** A snapshot, in the protocol's own shape. */
export interface BridgeSnapshot {
  harness_id: string;
  harness_name: string;
  source: { type: string; name: string };
  updated_at: number;
  tasks: BridgeTask[];
}

/** What `parseArgs` understood from the command line. */
export interface BridgeOptions {
  command: BridgeCommand;
  harnessId?: string;
  harnessName?: string;
  task?: string;
  taskId?: string;
  title?: string;
  status?: BridgeStatus;
  message?: string;
  endpoint: string;
  stateDir?: string;
  timeoutMs: number;
  json: boolean;
  /** Print the snapshot instead of sending it. Never touches the network. */
  dryRun: boolean;
}

/** The flags that take a value, so `--message` cannot swallow the next command. */
const VALUE_FLAGS = new Set([
  "--harness",
  "--harness-id",
  "--name",
  "--task",
  "--task-id",
  "--title",
  "--status",
  "--message",
  "--port",
  "--endpoint",
  "--state-dir",
  "--timeout",
]);

/** The commands, in the order the help text lists them. */
export const BRIDGE_COMMANDS: BridgeCommand[] = ["start", "update", "done", "fail", "cancel"];

/**
 * Turn a raw status into a protocol status.
 *
 * Accepts the aliases a person actually types (`done`, `fail`, `cancel`) rather
 * than only the protocol spelling, and returns a message naming the value when
 * it cannot, so a typo is answered instead of silently dropped.
 */
export function resolveStatus(raw: string): StatusResult {
  const status = STATUS_ALIASES[raw.trim().toLowerCase()];
  if (!status) {
    return {
      error: `unknown status "${raw.trim()}"; expected running, waiting, completed, failed or cancelled`,
    };
  }
  return { status };
}

/**
 * The outcome of reading a status.
 *
 * A discriminated result rather than "a status or a message": a valid status is
 * itself a string, so a caller checking `typeof === "string"` to detect the
 * error would reject every correct status.
 */
export type StatusResult = { status: BridgeStatus } | { error: string };

/** Whether a status means the task is over. */
export function isFinished(status: BridgeStatus): boolean {
  return status !== "running" && status !== "waiting";
}

/**
 * A stable id derived from a human label.
 *
 * Ids must be stable across calls and safe as an identifier, so a title is
 * lowercased and reduced to `[a-z0-9-]`. A label with nothing usable in it - a
 * title written entirely in a non-Latin script, say - falls back to a short hash
 * of the original rather than to an empty id, because an empty id would be
 * rejected by the protocol and a shared one would collide.
 */
export function slug(value: string): string {
  const reduced = value
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 64);

  if (reduced) {
    return reduced;
  }

  // FNV-1a, so the same label always produces the same id on every run.
  let hash = 0x811c9dc5;
  for (const character of value) {
    hash ^= character.codePointAt(0) ?? 0;
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  return `id-${hash.toString(16).padStart(8, "0")}`;
}

/**
 * Read the command line.
 *
 * Returns the options, or a message explaining the first problem. Nothing here
 * touches the network or the disk, so a bad command line is answered before any
 * state is written.
 */
export function parseArgs(argv: string[]): BridgeOptions | string {
  const args = [...argv];
  if (args.length === 0 || args[0] === "-h" || args[0] === "--help" || args[0] === "help") {
    return "help";
  }

  const command = args.shift() as string;
  if (!BRIDGE_COMMANDS.includes(command as BridgeCommand)) {
    return `unknown command "${command}"; expected ${BRIDGE_COMMANDS.join(", ")}`;
  }

  const options: BridgeOptions = {
    command: command as BridgeCommand,
    endpoint: BRIDGE_ENDPOINT,
    timeoutMs: BRIDGE_TIMEOUT_MS,
    json: false,
    dryRun: false,
  };

  // A finished command carries its status implicitly, so `--status` on one is a
  // contradiction worth pointing out rather than silently ignoring.
  const finishStatus = FINISH_COMMANDS[options.command];
  let port: number | undefined;

  while (args.length > 0) {
    const flag = args.shift() as string;

    if (flag === "--json") {
      options.json = true;
      continue;
    }
    if (flag === "--dry-run") {
      options.dryRun = true;
      continue;
    }
    if (flag === "-h" || flag === "--help") {
      return "help";
    }
    if (!VALUE_FLAGS.has(flag)) {
      return `unknown option "${flag}"`;
    }

    const value = args.shift();
    if (value === undefined) {
      return `${flag} needs a value`;
    }

    switch (flag) {
      case "--harness":
        options.harnessId = value;
        break;
      case "--harness-id":
        options.harnessId = value;
        break;
      case "--name":
        options.harnessName = value;
        break;
      case "--task":
        options.task = value;
        break;
      case "--task-id":
        options.taskId = value;
        break;
      case "--title":
        options.title = value;
        break;
      case "--message":
        options.message = value;
        break;
      case "--state-dir":
        options.stateDir = value;
        break;
      case "--endpoint":
        options.endpoint = value;
        break;
      case "--timeout": {
        const parsed = Number(value);
        if (!Number.isFinite(parsed) || parsed <= 0) {
          return `--timeout must be a positive number of milliseconds, got "${value}"`;
        }
        options.timeoutMs = Math.trunc(parsed);
        break;
      }
      case "--port": {
        const parsed = Number(value);
        if (!Number.isInteger(parsed) || parsed < 1 || parsed > 65535) {
          return `--port must be a port number between 1 and 65535, got "${value}"`;
        }
        port = parsed;
        break;
      }
      case "--status": {
        if (finishStatus) {
          return `${options.command} already reports a final status; --status is not allowed with it`;
        }
        const resolved = resolveStatus(value);
        if ("error" in resolved) {
          return resolved.error;
        }
        options.status = resolved.status;
        break;
      }
      default:
        return `unknown option "${flag}"`;
    }
  }

  if (port !== undefined) {
    if (options.endpoint !== BRIDGE_ENDPOINT) {
      return "use either --port or --endpoint, not both";
    }
    options.endpoint = `http://127.0.0.1:${port}/api/harness/snapshot`;
  }

  if (options.command === "start") {
    if (!options.harnessId && !options.harnessName) {
      return "start needs --harness, so the task has a harness to belong to";
    }
    if (!options.task) {
      return "start needs --task, the title shown in Harness Tasks";
    }
  } else {
    for (const [flag, value] of [
      ["--harness", options.harnessId],
      ["--harness-id", options.harnessId],
      ["--name", options.harnessName],
      ["--task", options.task],
      ["--task-id", options.taskId],
    ] as const) {
      if (value) {
        return `${flag} only applies to start; the current task keeps its id and title`;
      }
    }
  }

  if (options.command === "update" && !options.status && !options.title) {
    // An explicit empty --message is a real instruction (clear the message), so
    // "was the flag given" decides here, not whether the value is truthy.
    if (options.message === undefined) {
      return "update needs at least one of --status, --message or --title";
    }
  }

  if (options.title !== undefined && !options.title.trim()) {
    return "--title must not be empty";
  }

  return options;
}

/** What `start` needs to begin a task. */
export interface StartInput {
  harness: string;
  harnessName?: string;
  task: string;
  taskId?: string;
  status?: BridgeStatus;
  message?: string;
  now: number;
}

/**
 * Begin a task, and return the session that represents it.
 *
 * The task id is derived once here from the title and then kept, so every later
 * call names the same task. That is the whole reason the bridge keeps a session:
 * without it, `update` and `done` would guess, and a guessing producer shows up
 * in the note as a flood of half-finished tasks.
 */
export function startSession(input: StartInput): BridgeSession {
  const harnessName = input.harnessName?.trim() || input.harness.trim();
  const title = input.task.trim();

  const session: BridgeSession = {
    harnessId: slug(input.harness),
    harnessName,
    taskId: input.taskId?.trim() || slug(title),
    title,
    startedAt: input.now,
    status: input.status ?? "running",
  };

  const message = input.message?.trim();
  if (message) {
    session.message = message;
  }

  return session;
}

/** What `update` may change. */
export interface UpdateInput {
  status?: BridgeStatus;
  title?: string;
  message?: string;
}

/**
 * Apply an update, returning a new session.
 *
 * Only what was passed changes, so `update --message "Running tests"` does not
 * quietly reset a task that had been reported as `waiting`. An empty
 * `--message` removes the message rather than sending a blank one.
 */
export function applyUpdate(session: BridgeSession, input: UpdateInput): BridgeSession {
  const next: BridgeSession = { ...session };

  if (input.status) {
    next.status = input.status;
  }
  if (input.title?.trim()) {
    next.title = input.title.trim();
  }
  if (input.message !== undefined) {
    const message = input.message.trim();
    if (message) {
      next.message = message;
    } else {
      delete next.message;
    }
  }

  return next;
}

/**
 * The snapshot a session stands for.
 *
 * Always exactly one task: a bridge session is one task, and the app replaces a
 * harness's snapshot by `harness_id`, so repeated calls update one row rather
 * than accumulating new ones.
 *
 * `updated_at` never moves backwards. The protocol rejects a task that finished
 * before it started, and a clock that steps back mid-task would otherwise turn a
 * correct bridge into a producer bug.
 */
export function toSnapshot(session: BridgeSession, now: number): BridgeSnapshot {
  const updatedAt = Math.max(now, session.startedAt);

  const task: BridgeTask = {
    task_id: session.taskId,
    title: session.title,
    status: session.status,
    started_at: session.startedAt,
    updated_at: updatedAt,
  };
  if (session.message) {
    task.message = session.message;
  }

  return {
    harness_id: session.harnessId,
    harness_name: session.harnessName,
    source: { type: "push", name: BRIDGE_SOURCE_NAME },
    updated_at: updatedAt,
    tasks: [task],
  };
}

/**
 * Whether a stored value is a session this version of the bridge can use.
 *
 * A session file that does not match is treated as missing rather than
 * partially trusted, so a hand-edited or half-written file cannot make the
 * bridge report a task with no id.
 */
export function isSession(value: unknown): value is BridgeSession {
  if (typeof value !== "object" || value === null) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return (
    typeof candidate.harnessId === "string" &&
    candidate.harnessId.length > 0 &&
    typeof candidate.harnessName === "string" &&
    candidate.harnessName.length > 0 &&
    typeof candidate.taskId === "string" &&
    candidate.taskId.length > 0 &&
    typeof candidate.title === "string" &&
    candidate.title.length > 0 &&
    typeof candidate.startedAt === "number" &&
    typeof candidate.status === "string" &&
    (candidate.message === undefined || typeof candidate.message === "string")
  );
}

/** The help text, kept next to the parser so the two cannot drift apart. */
export function helpText(program: string): string {
  return [
    "Report what a harness is doing to Sticky Harness.",
    "",
    `Usage: ${program} <command> [options]`,
    "",
    "Commands:",
    "  start    Begin a task (replaces any current one)",
    "  update   Change the current task's status, message or title",
    "  done     Finish it as completed",
    "  fail     Finish it as failed",
    "  cancel   Finish it as cancelled",
    "",
    "Start options:",
    '  --harness <id>      Harness id, e.g. codex, deepseek, my-bot',
    "  --name <label>      Display name in Harness Tasks (defaults to --harness)",
    '  --task <title>      Task title, e.g. "Refactor runtime"',
    "  --task-id <id>      Pin the task id instead of deriving it from the title",
    "",
    "Update options:",
    "  --status <status>   running | waiting",
    "  --title <title>     Rename the current task",
    "",
    "Any command:",
    '  --message <text>    Short status line, e.g. "Running tests" ("" clears it)',
    "  --port <n>          Harness port (default 17899)",
    "  --endpoint <url>    Full loopback endpoint, instead of --port",
    "  --timeout <ms>      How long to wait for the app (default 2000)",
    "  --state-dir <path>  Where to keep bridge-session.json",
    "  --json              Also print the snapshot that was sent",
    "  --dry-run           Print the snapshot and send nothing",
    "  -h, --help          This text",
    "",
    "Examples:",
    '  sticky-harness-bridge start --harness codex --task "Refactor runtime"',
    '  sticky-harness-bridge update --message "Running tests"',
    '  sticky-harness-bridge update --status waiting --message "Waiting for review"',
    "  sticky-harness-bridge done",
  ].join("\n");
}
