// Issue #1438: the shared `.switch` control stays readable in forced-colors
// mode (Windows high contrast): on/off differ in knob colour and track fill,
// disabled uses GrayText, and the focus ring stays visible.
import type { Locator } from "@playwright/test";
import { type AppDriver, expect, openSettings, test } from "../support/app";
import { populated } from "../support/data";

type Look = { track: string; knob: string; border: string; outline: string; outlineStyle: string; opacity: string };

const look = (sw: Locator) =>
  sw.evaluate((el): Look => {
    const cs = getComputedStyle(el);
    const knob = getComputedStyle(el, "::after");
    return {
      track: cs.backgroundColor,
      knob: knob.backgroundColor,
      border: cs.borderTopColor,
      outline: cs.outlineColor,
      outlineStyle: cs.outlineStyle,
      opacity: cs.opacity,
    };
  });

async function openSwitch(app: AppDriver) {
  await app.launch(populated());
  const dialog = await openSettings(app.page, "Sessions & Data");
  return dialog.locator("input.switch");
}

test("forced colors: on and off differ in knob and track, knob contrasts with track", async ({ app }) => {
  await app.page.emulateMedia({ forcedColors: "active" });
  const sw = await openSwitch(app);
  await expect(sw).not.toBeChecked();
  const off = await look(sw);
  await sw.check();
  const on = await look(sw);
  expect(on.track).not.toBe(off.track);
  expect(on.knob).not.toBe(off.knob);
  expect(off.knob).not.toBe(off.track);
  expect(on.knob).not.toBe(on.track);
});

test("forced colors: disabled switch is distinct and the focus ring is visible", async ({ app }) => {
  await app.page.emulateMedia({ forcedColors: "active" });
  const sw = await openSwitch(app);
  const enabled = await look(sw);
  // Reach the switch by keyboard so :focus-visible matches in every engine.
  for (let i = 0; i < 40 && !(await sw.evaluate((el) => el === document.activeElement)); i++) {
    await app.page.keyboard.press("Tab");
  }
  await expect(sw).toBeFocused();
  const focused = await look(sw);
  expect(focused.outlineStyle).not.toBe("none");
  expect(focused.outline).not.toBe(focused.track);
  await sw.evaluate((el) => ((el as HTMLInputElement).disabled = true));
  const disabled = await look(sw);
  expect(disabled.border).not.toBe(enabled.border);
  expect(disabled.knob).not.toBe(enabled.knob);
});

test("without forced colors the switch keeps its theme-token look", async ({ app }) => {
  await app.page.emulateMedia({ forcedColors: "none" });
  const sw = await openSwitch(app);
  const off = await look(sw);
  // Theme tokens, not the system colours of the forced-colors block.
  const canvas = await sw.evaluate((el) => {
    const d = document.createElement("div");
    d.style.backgroundColor = "Canvas";
    el.parentElement!.appendChild(d);
    const c = getComputedStyle(d).backgroundColor;
    d.remove();
    return c;
  });
  expect(off.track).not.toBe(canvas);
  await sw.check();
  const on = await look(sw);
  expect(on.track).not.toBe(off.track);
  expect(on.opacity).toBe("1");
});
