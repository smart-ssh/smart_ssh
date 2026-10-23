// Test-only entry point for the Playwright suite. It is served by the Vite
// dev server next to the real `index.html`, but is no input of `vite build`,
// so neither this file nor the fake backend ends up in the shipped bundle.
import { installFakeBackend } from "./fakeBackend";

const fixture = window.__E2E_FIXTURE__;
if (!fixture) {
  throw new Error("[e2e harness] no fixture: open this page through the Playwright `app` fixture");
}
installFakeBackend(fixture);

// Test hook: lets a spec register extension actions the way an edition would.
const { registerComposerAction, registerDocumentAction, registerSettingsSection } = await import("../../src/extensions/registry");
const { createElement } = await import("react");
// A settings section with a wide, unbreakable heading line and a wide table, as an edition might add.
const registerTestSettingsSection = (id: string, label: string) =>
  registerSettingsSection({
    id,
    label,
    component: () =>
      createElement(
        "div",
        { "data-testid": "ext-section" },
        createElement("p", null, "Extension section content ".repeat(8)),
        createElement("button", { type: "button", className: "rounded border px-3 py-1 text-slate-100" }, "Extension action"),
      ),
  });
(window as unknown as { __e2eExtensions: unknown }).__e2eExtensions = {
  registerComposerAction,
  registerDocumentAction,
  registerTestSettingsSection,
};

// The fake IPC layer must exist before the app's first `invoke`, which
// happens at module evaluation time of `main.tsx` (top-level await).
await import("../../src/main.tsx");
