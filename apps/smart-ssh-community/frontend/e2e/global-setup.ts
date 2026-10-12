// Warms the Vite dev server before any test runs. Vite transforms modules
// on first request; with four workers opening the app at once, a cold server
// let the first tests spend most of their 30 s budget just loading the page
// (seen as sporadic timeouts in the WebKit projects). Loading the harness
// once, in each engine, until the network is idle makes every test start
// against a warm server. No timeout is raised and nothing is retried.
import { chromium, webkit, type FullConfig } from "@playwright/test";

export default async function globalSetup(config: FullConfig): Promise<void> {
  const baseURL = config.projects[0]?.use.baseURL;
  if (!baseURL) throw new Error("global-setup: baseURL is not configured");
  for (const engine of [chromium, webkit]) {
    const browser = await engine.launch();
    try {
      const page = await browser.newPage();
      await page.goto(`${baseURL}/e2e/harness/index.html`, { waitUntil: "networkidle", timeout: 90_000 });
    } finally {
      await browser.close();
    }
  }
}
