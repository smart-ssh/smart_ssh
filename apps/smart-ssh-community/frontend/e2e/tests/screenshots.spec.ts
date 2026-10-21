// The screenshot helper writes one image per window size for a named screen.
import fs from "node:fs";
import { expect, test } from "../support/app";
import { populated } from "../support/data";
import { captureScreen } from "../support/screenshots";

test("captureScreen writes the main screen at every screenshot size", async ({ app }) => {
  await app.launch(populated());
  await expect(app.page.getByText("db-primary")).toBeVisible();
  const files = await captureScreen(app.page, "main-screen");
  expect(files.map((f) => f.split("/").pop())).toEqual(["main-screen-1280x800.png", "main-screen-800x560.png", "main-screen-390x844.png"]);
  for (const f of files) expect(fs.statSync(f).size).toBeGreaterThan(1000);
});
