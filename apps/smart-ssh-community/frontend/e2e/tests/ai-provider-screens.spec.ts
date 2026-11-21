// Issue #1377: review screenshots of the AI Provider settings and the add-provider dialog
// (SCREENSHOT_LABEL=before|after, SCREENSHOT_SCHEME=dark).
import { expect, openSettings, test } from "../support/app";
import { aiProvider, populated } from "../support/data";
import { captureScreen } from "../support/screenshots";

const scheme = process.env.SCREENSHOT_SCHEME === "dark" ? "dark" : "light";

test("review screenshots: AI provider settings and add dialog", async ({ app }) => {
  await app.launch({
    ...populated(),
    commands: { set_risk_second_opinion_settings: { result: null } },
    aiProviders: [
      aiProvider({ id: "p-1", displayName: "Primary", providerType: "generic_openai_compatible", baseUrl: "https://llm.example.com/v1", isActive: true }),
      aiProvider({ id: "p-2", displayName: "Backup", baseUrl: null, isActive: false }),
    ],
  });
  const page = app.page;
  if (scheme === "dark") await page.emulateMedia({ colorScheme: "dark" });
  const dialog = await openSettings(page, "AI Provider");
  await expect(dialog.getByText("Primary").first()).toBeVisible();
  const second = dialog.getByRole("checkbox", { name: /second opinion/i }).first();
  await second.check({ force: true });
  await captureScreen(page, `settings-aiprovider-${scheme}`);

  await dialog.getByRole("button", { name: "Add Provider", exact: true }).click();
  const add = page.getByRole("dialog", { name: "Add Provider" });
  await expect(add).toBeVisible();
  await add.getByRole("button", { name: /Advanced/ }).click();
  await captureScreen(page, `add-provider-dialog-${scheme}`);
});

test("provider rows share column x positions", async ({ app }) => {
  await app.launch({
    ...populated(),
    aiProviders: [
      aiProvider({ id: "p-1", displayName: "Primary", model: "model-a", isActive: true }),
      aiProvider({ id: "p-2", displayName: "Backup", model: "model-b", isActive: false }),
    ],
  });
  const dialog = await openSettings(app.page, "AI Provider");
  const a = await dialog.getByText("model-a", { exact: true }).boundingBox();
  const b = await dialog.getByText("model-b", { exact: true }).boundingBox();
  expect(a && b && Math.abs(a.x - b.x) < 1).toBe(true);
  // Names, models and action buttons must stay readable (no ellipsis, no overflow) at every size.
  const clipped = await dialog.locator("ul li").evaluateAll((rows) =>
    rows.flatMap((row) =>
      Array.from(row.querySelectorAll<HTMLElement>("p.truncate, div.justify-end")).filter(
        (el) => el.scrollWidth > el.clientWidth,
      ).map((el) => el.textContent),
    ),
  );
  expect(clipped).toEqual([]);
});
