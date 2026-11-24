// Issue #1428: narrow settings list icons and row rules (screenshots for review).
import { openSettings, test } from "../support/app";
import { populated } from "../support/data";
import { captureScreen } from "../support/screenshots";

test.describe("settings list icons @phone", () => {
  test("screenshots: narrow list", async ({ app }, info) => {
    await app.launch(populated());
    await openSettings(app.page);
    await captureScreen(app.page, `settings-list-icons-${info.project.name}`);
  });
});
