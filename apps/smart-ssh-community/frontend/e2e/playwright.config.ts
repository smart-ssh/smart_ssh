import { defineConfig, devices } from "@playwright/test";

const PORT = 4174;
const LARGE = { width: 1280, height: 800 };
const SMALL = { width: 800, height: 560 };

// Two engines × two window sizes. WebKit is the engine closest to the
// webviews the app runs in on macOS and Linux.
const engines = [
  { name: "chromium", device: devices["Desktop Chrome"] },
  { name: "webkit", device: devices["Desktop Safari"] },
] as const;
const viewports = [
  { name: "large", viewport: LARGE },
  { name: "small", viewport: SMALL },
] as const;

export default defineConfig({
  testDir: "./tests",
  outputDir: "./test-results",
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: 0,
  reporter: process.env.CI ? [["list"], ["html", { open: "never", outputFolder: "./playwright-report" }]] : "list",
  use: {
    baseURL: `http://127.0.0.1:${PORT}`,
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
  },
  projects: engines.flatMap((engine) =>
    viewports.map((size) => ({
      name: `${engine.name}-${size.name}`,
      use: { ...engine.device, viewport: size.viewport, deviceScaleFactor: 1 },
    })),
  ),
  webServer: {
    // The dev server of the real Vite config, on its own port so it never
    // collides with a running `npm run dev`.
    command: `npx vite --host 127.0.0.1 --port ${PORT} --strictPort`,
    cwd: "..",
    url: `http://127.0.0.1:${PORT}/e2e/harness/index.html`,
    reuseExistingServer: !process.env.CI,
    timeout: 120_000,
  },
});
