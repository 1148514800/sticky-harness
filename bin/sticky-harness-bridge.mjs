#!/usr/bin/env node
/**
 * `sticky-harness-bridge` - tell Sticky Harness what a harness is doing.
 *
 * A producer, not an integration. Every command ends in the same place: one
 * `HarnessSnapshot` POSTed to the app's existing loopback endpoint. The bridge
 * never reads a harness's files, never starts or stops anything and never sends
 * a prompt; it only reports what you tell it.
 *
 * Why this is a Node script rather than a Rust binary: the app already needs
 * Node to be built and run, so a script works on every machine this project
 * supports without a second toolchain, a second build step or a second artifact.
 * It has no dependencies at all - `node:http`, `node:fs` and `node:os` are the
 * whole list.
 *
 * See `README.md` for the three-command example, and `src/harness/bridge.ts`
 * for the argument parsing, session and snapshot construction this file wires
 * together.
 */

import { mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { request } from "node:http";
import { homedir } from "node:os";
import { dirname, join, resolve } from "node:path";

import {
  BRIDGE_SESSION_FILE,
  applyUpdate,
  helpText,
  isSession,
  parseArgs,
  startSession,
  toSnapshot,
} from "../src/harness/bridge.ts";

const PROGRAM = "sticky-harness-bridge";
const HELP = "help";

/** The status each finishing command reports. */
const FINISH_STATUSES = { done: "completed", fail: "failed", cancel: "cancelled" };

/** Whether a status means the task is over. */
function isFinished(status) {
  return status !== "running" && status !== "waiting";
}

/** Where the session lives when nothing says otherwise. */
function defaultStateDir() {
  const override = process.env.STICKY_HARNESS_BRIDGE_DIR;
  if (override && override.trim()) {
    return resolve(override.trim());
  }
  return join(homedir(), ".sticky-harness");
}

function sessionPath(stateDir) {
  return join(stateDir, BRIDGE_SESSION_FILE);
}

/** Print to stderr and exit non-zero. */
function fail(message) {
  process.stderr.write(`${PROGRAM}: ${message}\n`);
  process.exit(1);
}

/** Print a warning to stderr; the command still succeeds. */
function warn(message) {
  process.stderr.write(`${PROGRAM}: ${message}\n`);
}

/**
 * Read the session, if there is a usable one.
 *
 * A file that does not parse, or does not look like a session, counts as absent
 * rather than being repaired: guessing an id would create a second task in the
 * window, and the honest answer is "there is no current task, run start".
 */
function readSession(stateDir) {
  let raw;
  try {
    raw = readFileSync(sessionPath(stateDir), "utf8");
  } catch (error) {
    if (error.code === "ENOENT") {
      return undefined;
    }
    fail(`could not read ${sessionPath(stateDir)}: ${error.message}`);
  }

  let parsed;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return undefined;
  }

  return isSession(parsed) ? parsed : undefined;
}

function writeSession(stateDir, session) {
  const path = sessionPath(stateDir);
  try {
    mkdirSync(dirname(path), { recursive: true });
    writeFileSync(path, `${JSON.stringify(session, null, 2)}\n`, "utf8");
  } catch (error) {
    fail(`could not write ${path}: ${error.message}`);
  }
}

function clearSession(stateDir) {
  try {
    rmSync(sessionPath(stateDir));
  } catch (error) {
    if (error.code !== "ENOENT") {
      fail(`could not remove ${sessionPath(stateDir)}: ${error.message}`);
    }
  }
}

/**
 * POST one snapshot to the app.
 *
 * The only I/O the bridge does with the app, and the only place that fails for a
 * reason the user did not cause. Every failure becomes a sentence that says what
 * to do, because "the app must be running" is the answer nine times out of ten.
 */
function post(endpoint, snapshot, timeoutMs) {
  return new Promise((settle, reject) => {
    const body = Buffer.from(JSON.stringify(snapshot), "utf8");

    let target;
    try {
      target = new URL(endpoint);
    } catch {
      reject(new Error(`${endpoint} is not a valid URL`));
      return;
    }

    const call = request(
      {
        hostname: target.hostname,
        port: target.port || 80,
        path: `${target.pathname}${target.search}`,
        method: "POST",
        headers: {
          "Content-Type": "application/json",
          "Content-Length": body.length,
        },
      },
      (response) => {
        let text = "";
        response.setEncoding("utf8");
        response.on("data", (chunk) => {
          text += chunk;
        });
        response.on("end", () => {
          if (response.statusCode >= 200 && response.statusCode < 300) {
            settle(text);
            return;
          }
          // The app answers a refused snapshot with its validator's own
          // message, so passing that through beats paraphrasing it.
          let reason = text;
          try {
            reason = JSON.parse(text).error ?? text;
          } catch {
            // Not JSON. The raw body is the best answer available.
          }
          reject(new Error(`the app refused the snapshot: ${reason}`));
        });
      },
    );

    call.setTimeout(timeoutMs, () => {
      call.destroy(new Error(`no answer within ${timeoutMs} ms`));
    });

    call.on("error", (error) => {
      if (error.code === "ECONNREFUSED" || error.code === "ECONNRESET") {
        reject(
          new Error(
            `nothing is listening on ${target.hostname}:${target.port || 80}; start Sticky Harness first`,
          ),
        );
        return;
      }
      reject(error);
    });

    call.write(body);
    call.end();
  });
}

/** How a session is described on the command line. */
function describe(session) {
  return `${session.harnessName}: ${session.title} (${session.status}, ${session.taskId})`;
}

async function main() {
  const parsed = parseArgs(process.argv.slice(2));

  if (parsed === HELP) {
    process.stdout.write(`${helpText(PROGRAM)}\n`);
    return;
  }
  if (typeof parsed === "string") {
    fail(`${parsed}\n\nRun "${PROGRAM} --help" for usage.`);
  }

  const stateDir = parsed.stateDir ? resolve(parsed.stateDir) : defaultStateDir();
  const now = Date.now();
  const previous = readSession(stateDir);

  let session;
  let finished = false;

  if (parsed.command === "start") {
    // The harness label falls back to the id, so `--harness codex --task "..."`
    // is enough; `--name` is only for a nicer label in the window.
    const harness = parsed.harnessId ?? parsed.harnessName ?? "";
    session = startSession({
      harness,
      harnessName: parsed.harnessName,
      task: parsed.task,
      taskId: parsed.taskId,
      status: parsed.status,
      message: parsed.message,
      now,
    });

    // Starting over is allowed and sometimes right, but never silent: the
    // previous task's snapshot is about to be replaced in the app.
    if (previous && !isFinished(previous.status)) {
      warn(`replacing the current task "${previous.title}"`);
    }
  } else {
    if (!previous) {
      fail(
        `no current task for "${parsed.command}"; run "${PROGRAM} start --harness <id> --task \"<title>\"" first`,
      );
    }

    const finishStatus = FINISH_STATUSES[parsed.command];
    if (finishStatus) {
      if (isFinished(previous.status)) {
        warn(`the current task is already ${previous.status}`);
      }
      session = applyUpdate(previous, { status: finishStatus, message: parsed.message });
      finished = true;
    } else {
      session = applyUpdate(previous, {
        status: parsed.status,
        title: parsed.title,
        message: parsed.message,
      });
    }
  }

  const snapshot = toSnapshot(session, now);

  if (parsed.dryRun) {
    // Nothing is written and nothing is sent, so a dry run is always safe.
    process.stdout.write(`${JSON.stringify(snapshot, null, 2)}\n`);
    return;
  }

  try {
    await post(parsed.endpoint, snapshot, parsed.timeoutMs);
  } catch (error) {
    fail(error.message);
  }

  writeSession(stateDir, session);

  // A finished task leaves no current task behind. Keeping the session would
  // make a later `update` look like it still had something to report, so it is
  // dropped once the app has accepted the final snapshot.
  if (finished) {
    clearSession(stateDir);
    process.stdout.write(`${session.harnessName}: ${session.title} -> ${session.status}\n`);
  } else if (parsed.command === "start") {
    process.stdout.write(`reported ${describe(session)} to ${parsed.endpoint}\n`);
  } else {
    process.stdout.write(`${describe(session)}\n`);
  }

  if (parsed.json) {
    process.stdout.write(`${JSON.stringify(snapshot)}\n`);
  }
}

main().catch((error) => {
  fail(error instanceof Error ? error.message : String(error));
});
