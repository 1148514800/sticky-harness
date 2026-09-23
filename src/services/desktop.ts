import { invoke } from "@tauri-apps/api/core";
import type { AppPathInfo, CreatedWindow } from "../types/desktop";

/**
 * Thin wrapper around the Rust desktop commands.
 *
 * Window lifecycle, tray behaviour and local paths live in Rust; React only
 * asks for them. Every call converts a rejected IPC promise into a readable
 * Error so UI code can show a message instead of crashing.
 */

async function callCommand<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (error) {
    const reason = typeof error === "string" ? error : String(error);
    throw new Error(`${command} failed: ${reason}`);
  }
}

/** Ask Rust to create a new isolated test window. */
export async function createTestWindow(): Promise<CreatedWindow> {
  const label = await callCommand<string>("create_test_window");
  return { label, role: "test-note" };
}

/** Resolve the OS-specific app data directory owned by Rust. */
export async function getAppPathInfo(): Promise<AppPathInfo> {
  const appDataDir = await callCommand<string>("app_data_dir");
  return { appDataDir };
}
