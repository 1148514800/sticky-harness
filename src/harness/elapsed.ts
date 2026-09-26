/**
 * Elapsed-time formatting for the Harness Task Note.
 *
 * A pure function so it can be tested without a window. The protocol carries
 * `started_at` in Unix milliseconds and never a rendered duration, which is what
 * keeps the note's clock honest: it always shows the time between the start
 * stamp and *now*, rather than a number a producer computed some time ago.
 */

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;

/**
 * Format the time between `startedAt` and `now` as `MM:SS` or `H:MM:SS`.
 *
 * Matches how a stopwatch reads, which is what a running-task list should look
 * like: `00:43`, `08:17`, `1:04:32`.
 *
 * A negative difference means the producer's clock is ahead of ours, and a
 * clock skew is not worth showing the user as a negative duration, so it clamps
 * to `00:00`.
 */
export function formatElapsed(startedAt: number, now: number): string {
  const elapsed = Math.max(0, now - startedAt);

  const totalSeconds = Math.floor(elapsed / 1000);
  const seconds = totalSeconds % 60;
  const minutes = Math.floor(totalSeconds / 60) % 60;
  // Derived from `totalSeconds`, not from `elapsed`: HOUR is milliseconds, and
  // dividing milliseconds by it while the other two work in seconds is how the
  // hour count silently became zero.
  const hours = Math.floor(totalSeconds / (HOUR / 1000));

  if (hours > 0) {
    return `${hours}:${pad(minutes)}:${pad(seconds)}`;
  }
  return `${pad(minutes)}:${pad(seconds)}`;
}

function pad(value: number): string {
  return value.toString().padStart(2, "0");
}

/**
 * A short, plain-language label for a status.
 *
 * Only `running` and `waiting` normally appear, because the note reads the live
 * active view. The remaining cases exist so an unexpected value renders as
 * readable text rather than as a raw enum.
 */
export function formatStatus(status: string): string {
  switch (status) {
    case "running":
      return "Running";
    case "waiting":
      return "Waiting";
    case "failed":
      return "Failed";
    case "completed":
      return "Completed";
    case "cancelled":
      return "Cancelled";
    default:
      return "Unknown";
  }
}
