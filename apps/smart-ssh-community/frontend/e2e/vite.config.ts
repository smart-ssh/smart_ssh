// Vite config of the end-to-end dev server: the app's own config, plus the
// test entry in the dependency pre-scan. Without that, Vite discovers the
// harness imports (`@tauri-apps/api/mocks`) only at runtime and reloads the
// page in the middle of a test.
import { fileURLToPath } from "node:url";
import base from "../vite.config";

export default {
  ...base,
  root: fileURLToPath(new URL("..", import.meta.url)),
  optimizeDeps: {
    entries: ["index.html", "e2e/harness/index.html"],
  },
};
