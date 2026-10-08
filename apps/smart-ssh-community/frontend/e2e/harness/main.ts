// Test-only entry point for the Playwright suite. It is served by the Vite
// dev server next to the real `index.html`, but is no input of `vite build`,
// so neither this file nor the fake backend ends up in the shipped bundle.
import { installFakeBackend } from "./fakeBackend";

const fixture = window.__E2E_FIXTURE__;
if (!fixture) {
  throw new Error("[e2e harness] no fixture: open this page through the Playwright `app` fixture");
}
installFakeBackend(fixture);

// The fake IPC layer must exist before the app's first `invoke`, which
// happens at module evaluation time of `main.tsx` (top-level await).
await import("../../src/main.tsx");
