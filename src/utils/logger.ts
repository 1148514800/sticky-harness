/**
 * Minimal logging helpers.
 *
 * Errors are surfaced in the UI where useful, but they should always be
 * visible in the webview console as well so a failing IPC call is traceable.
 */

export function logError(message: string, error: unknown): void {
  console.error(`[sticky-harness] ${message}`, error);
}

export function logInfo(message: string, ...details: unknown[]): void {
  console.info(`[sticky-harness] ${message}`, ...details);
}
