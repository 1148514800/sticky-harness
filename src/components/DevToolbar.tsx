import { useState } from "react";
import { createTestWindow } from "../services/desktop";
import { logError, logInfo } from "../utils/logger";

/**
 * Temporary Phase 0 control used to prove Tauri can create several
 * independent windows at runtime. Phase 1 replaces this with real note
 * creation.
 */
export function DevToolbar() {
  const [error, setError] = useState<string | null>(null);
  const [isCreating, setIsCreating] = useState(false);

  async function handleCreateTestWindow() {
    setIsCreating(true);
    setError(null);

    try {
      const created = await createTestWindow();
      logInfo(`created test window "${created.label}"`);
    } catch (cause) {
      logError("could not create a test window", cause);
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setIsCreating(false);
    }
  }

  return (
    <div className="dev-toolbar">
      <button type="button" onClick={handleCreateTestWindow} disabled={isCreating}>
        {isCreating ? "Creating..." : "Create Test Window"}
      </button>
      {error ? <p className="dev-toolbar__error">{error}</p> : null}
    </div>
  );
}
