// Issue #1325, Spec 0116: activity icon in the header and its popover.
// Runs in every project, so the same checks cover the wide and the narrow layout.
import { expect, test } from "../support/app";
import { populated } from "../support/data";
import { expectFullyInViewport } from "../support/layout";

const base = {
  id: "act-1",
  kind: "demo",
  key: "x",
  titleKey: "activities.title",
  titleParams: {},
  state: "running",
  done: 524288,
  total: 1048576,
  statusKey: null,
  errorCode: null,
  errorMessage: null,
  cancellable: true,
  startedAt: 1,
  dismissed: false,
};

test.describe("activity center", () => {
  test("no icon without activity; icon, popover, keyboard and cancel with one", async ({ app }) => {
    await app.launch(populated());
    const page = app.page;
    const icon = page.getByTestId("activity-center-button");
    await expect(page.getByRole("button", { name: /^Settings$/ })).toBeAttached();
    await expect(icon).toHaveCount(0);

    await app.emit("activity-changed", base);
    await expect(icon).toBeVisible();
    await expectFullyInViewport(icon);
    await expect(icon).toHaveAccessibleName("Activities: 1 running");

    await icon.focus();
    await page.keyboard.press("Enter");
    const popover = page.getByRole("dialog", { name: "Activities" });
    await expect(popover).toBeVisible();
    await expectFullyInViewport(popover);
    await expect(popover.getByRole("progressbar")).toHaveAttribute("value", "524288");

    await popover.getByRole("button", { name: "Cancel Activities" }).click();
    await expect.poll(async () => (await app.calls("activity_cancel")).length).toBe(1);
    await app.emit("activity-changed", { ...base, state: "cancelled", cancellable: false });
    await expect(popover.getByText("Cancelled")).toBeVisible();

    await page.keyboard.press("Escape");
    await expect(popover).toHaveCount(0);
    await expect(icon).toBeFocused();
  });

  test("a failed activity shows its error until dismissed", async ({ app }) => {
    await app.launch(populated());
    const page = app.page;
    await app.emit("activity-changed", { ...base, state: "failed", errorMessage: "Disk full", cancellable: false });
    await page.getByTestId("activity-center-button").click();
    const popover = page.getByRole("dialog", { name: "Activities" });
    await expect(popover.getByText(/Disk full/)).toBeVisible();
    await popover.getByRole("button", { name: "Dismiss Activities" }).click();
    await expect.poll(async () => (await app.calls("activity_dismiss")).length).toBe(1);
    await app.emit("activity-changed", { ...base, state: "failed", dismissed: true });
    await expect(page.getByTestId("activity-center-button")).toHaveCount(0);
  });
});

test.describe("activity center narrow @phone", () => {
  test("icon and popover fit the phone layout and Escape closes", async ({ app }) => {
    await app.launch(populated());
    const page = app.page;
    await app.emit("activity-changed", base);
    const icon = page.getByTestId("activity-center-button");
    await expect(icon).toBeVisible();
    await expectFullyInViewport(icon);
    await icon.click();
    const popover = page.getByRole("dialog", { name: "Activities" });
    await expectFullyInViewport(popover);
    await page.keyboard.press("Escape");
    await expect(popover).toHaveCount(0);
    await expect(icon).toBeFocused();
  });
});
