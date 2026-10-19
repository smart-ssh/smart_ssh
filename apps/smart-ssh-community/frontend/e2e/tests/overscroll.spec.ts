// Issue #538: no elastic overscroll on the document or on scroll containers.
import { expect, test } from "../support/app";

test("html, body and arbitrary elements have overscroll-behavior: none", async ({ app }) => {
  await app.launch({});
  const values = await app.page.evaluate(() => {
    const probe = document.createElement("div");
    probe.style.overflow = "auto";
    document.body.appendChild(probe);
    const get = (el: Element) => getComputedStyle(el).overscrollBehaviorY;
    const result = {
      html: get(document.documentElement),
      body: get(document.body),
      scroller: get(probe),
    };
    probe.remove();
    return result;
  });
  expect(values).toEqual({ html: "none", body: "none", scroller: "none" });
});
