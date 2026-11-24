// Issue #1377: review screenshots of the AI Provider settings and the add-provider dialog
// (SCREENSHOT_LABEL=before|after, SCREENSHOT_SCHEME=dark).
import { expect, openSettings, test } from "../support/app";
import { aiProvider, populated } from "../support/data";
import { captureCurrent, captureScreen } from "../support/screenshots";


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
  const dialog = await openSettings(page, "AI Provider");
  await expect(dialog.getByText("Primary").first()).toBeVisible();
  const second = dialog.getByRole("checkbox", { name: /second opinion/i }).first();
  await second.check({ force: true });
  await expect(second).toBeChecked();
  await captureScreen(page, `settings-aiprovider`);

  await dialog.getByRole("button", { name: "Add Provider", exact: true }).click();
  const add = page.getByRole("dialog", { name: "Add Provider" });
  await expect(add).toBeVisible();
  const advanced = add.getByRole("button", { name: /Advanced/ });
  await advanced.click();
  await expect(advanced).toHaveAttribute("aria-expanded", "true");
  await captureScreen(page, `add-provider-dialog`);
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
  // Names and models must not be ellipsised; action buttons must stay inside their row.
  const clipped = await dialog.locator("ul li").evaluateAll((rows) =>
    rows.flatMap((row) => {
      const edge = row.getBoundingClientRect();
      const names = Array.from(row.querySelectorAll<HTMLElement>("p.truncate"))
        .filter((el) => el.scrollWidth > el.clientWidth)
        .map((el) => el.textContent);
      const buttons = Array.from(row.querySelectorAll<HTMLElement>("button"))
        .filter((el) => {
          const r = el.getBoundingClientRect();
          return r.left < edge.left - 0.5 || r.right > edge.right + 0.5;
        })
        .map((el) => el.textContent);
      return [...names, ...buttons];
    }),
  );
  expect(clipped).toEqual([]);
});

// Issue #1435: at lg and up the columns stay aligned and name/model stay readable;
// the action buttons wrap before name/model shrink.
for (const [language, settingsName, categoryName] of [
  ["en", "Settings", "AI Provider"],
  ["uk", "Параметри", "ШІ-провайдер"],
] as const) {
  for (const width of [1024, 1280]) {
    test(`provider rows stay readable and aligned (${language}, ${width}px)`, async ({ app }) => {
      await app.launch({
        ...populated(),
        settings: { ...populated().settings, language },
        aiProviders: [
          aiProvider({ id: "p-1", displayName: "Primary", providerType: "anthropic", model: "claude-sonnet-4-5", isActive: true, webResearchEnabled: true, attestationUrl: "https://att.example.com/a" }),
          aiProvider({ id: "p-2", displayName: "Backup", providerType: "anthropic", model: "claude-opus-4-1", isActive: false }),
        ],
      });
      const page = app.page;
      await page.setViewportSize({ width, height: 800 });
      await page.getByRole("button", { name: settingsName, exact: true }).click();
      const dialog = page.getByRole("dialog", { name: settingsName });
      await dialog.getByRole("button", { name: categoryName, exact: true }).click();
      const m1 = dialog.getByText("claude-sonnet-4-5", { exact: true });
      const m2 = dialog.getByText("claude-opus-4-1", { exact: true });
      await expect(m1).toBeVisible();
      await expect(m2).toBeVisible();
      const widths = await dialog.locator("ul li p.truncate").evaluateAll((els) =>
        els.filter((el) => el.scrollWidth > el.clientWidth).map((el) => el.textContent),
      );
      expect(widths).toEqual([]);
      const a = await m1.boundingBox();
      const b = await m2.boundingBox();
      expect(a && b && Math.abs(a.x - b.x) < 1).toBe(true);
      // Attestation and web-research rows span the whole row, below the actions.
      const row = dialog.locator("ul li").first();
      const rowBox = await row.boundingBox();
      const full = await row.locator(":scope > div").evaluateAll((divs) =>
        divs.map((d) => d.getBoundingClientRect().width),
      );
      expect(rowBox).not.toBeNull();
      for (const w of full) expect(w).toBeGreaterThan(rowBox!.width - 40);
    });
  }
}

for (const [language, settingsName, categoryName] of [
    ["en", "Settings", "AI Provider"],
    ["uk", "Параметри", "ШІ-провайдер"],
  ] as const) {
  test(`review screenshots: provider rows at 1024px (${language})`, async ({ app }) => {
    await app.launch({
      ...populated(),
      settings: { ...populated().settings, language },
      aiProviders: [
        aiProvider({ id: "p-1", displayName: "Primary", model: "claude-sonnet-4-5", isActive: true, webResearchEnabled: true, attestationUrl: "https://att.example.com/a" }),
        aiProvider({ id: "p-2", displayName: "Backup", model: "claude-opus-4-1", isActive: false }),
      ],
    });
    const page = app.page;
    await page.setViewportSize({ width: 1024, height: 800 });
    await page.getByRole("button", { name: settingsName, exact: true }).click();
    const dialog = page.getByRole("dialog", { name: settingsName });
    await dialog.getByRole("button", { name: categoryName, exact: true }).click();
    await expect(dialog.getByText("claude-sonnet-4-5", { exact: true })).toBeVisible();
    await captureCurrent(page, `provider-rows-1024-${language}`);
  });
}
