import { defineConfig } from "vitest/config";

// Unit tests cover the pure Markdown <-> editor-document conversion, so they
// run in a plain Node environment with no browser or Tauri needed.
export default defineConfig({
  test: {
    environment: "node",
    include: ["src/**/*.test.ts"],
  },
});