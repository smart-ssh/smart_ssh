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
