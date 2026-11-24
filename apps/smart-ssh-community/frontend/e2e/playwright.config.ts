import { defineConfig, devices } from "@playwright/test";

const PORT = 4174;
const LARGE = { width: 1280, height: 800 };
const SMALL = { width: 800, height: 560 };
const PHONE = { width: 390, height: 844 };

// Two engines × two desktop window sizes plus a phone size. WebKit is the engine closest to the
// webviews the app runs in on macOS and Linux.
const engines = [
  { name: "chromium", device: devices["Desktop Chrome"] },
  { name: "webkit", device: devices["Desktop Safari"] },
] as const;
const viewports = [
  { name: "large", viewport: LARGE },
  { name: "small", viewport: SMALL },
] as const;

// Specs tagged `@phone` run in the phone projects only; the phone projects run
// nothing else until the narrow-layout area issues opt their specs in by tag
// (Spec 0113).
const PHONE_TAG = /@phone/;

export default defineConfig({
  globalSetup: "./global-setup.ts",
  testDir: "./tests",
  outputDir: "./test-results",
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: 0,
  // On CI the JSON report feeds `report-flaky.mjs` (flaky tests in the job summary).
  reporter: process.env.CI
    ? [
        ["list"],
        ["html", { open: "never", outputFolder: "./playwright-report" }],
        ["json", { outputFile: "./test-results/results.json" }],
      ]
    : "list",
  use: {
    baseURL: `http://127.0.0.1:${PORT}`,
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
  },
  projects: engines.flatMap((engine) => [
    ...viewports.map((size) => ({
      name: `${engine.name}-${size.name}`,
      grepInvert: PHONE_TAG,
      // WebKit only, CI only: a retried pass shows as "flaky" in the report.
      retries: engine.name === "webkit" && process.env.CI ? 1 : 0,
      use: { ...engine.device, viewport: size.viewport, deviceScaleFactor: 1 },
    })),
    {
      name: `${engine.name}-phone`,
      grep: PHONE_TAG,
      retries: engine.name === "webkit" && process.env.CI ? 1 : 0,
      use: { ...engine.device, viewport: PHONE, deviceScaleFactor: 1 },
    },
  ]),
  webServer: {
    // The app's Vite config (see `vite.config.ts` here), on its own port so
    // it never collides with a running `npm run dev`.
    command: `npx vite --config e2e/vite.config.ts --host 127.0.0.1 --port ${PORT} --strictPort`,
    cwd: "..",
    url: `http://127.0.0.1:${PORT}/e2e/harness/index.html`,
    reuseExistingServer: !process.env.CI,
    timeout: 120_000,
  },
});
