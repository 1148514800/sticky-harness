import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Tauri expects a fixed dev server port and must not clear the screen,
// otherwise Rust compiler errors get wiped out.
// https://vite.dev/config/
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: {
      // src-tauri is watched by the Tauri CLI, not Vite.
      ignored: ["**/src-tauri/**"],
    },
  },
});
