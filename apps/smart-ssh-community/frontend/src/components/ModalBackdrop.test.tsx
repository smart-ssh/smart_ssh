import { render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { ModalBackdrop } from "./ModalBackdrop";

// Issue #244: `display:none` auf einem Vorfahren blendet auch `fixed`
// Nachfahren aus; `layer="app"` hängt den Backdrop deshalb an `document.body`.
describe("ModalBackdrop layer (issue #244)", () => {
  let style: HTMLStyleElement;

  beforeEach(() => {
    style = document.createElement("style");
    style.textContent = ".hidden { display: none; }";
    document.head.appendChild(style);
  });

  afterEach(() => style.remove());

  it('renders an "app" backdrop at document.body, visible although its container is hidden', () => {
    render(
      <div data-testid="container" className="hidden">
        <ModalBackdrop layer="app" className="fixed inset-0">
          <p>dialog</p>
        </ModalBackdrop>
      </div>,
    );
    const backdrop = document.querySelector(
      "[data-modal-backdrop]",
    ) as HTMLElement;
    expect(backdrop.parentElement).toBe(document.body);
    expect(screen.getByTestId("container").contains(backdrop)).toBe(false);
    expect(backdrop.closest(".hidden")).toBeNull();
    expect(getComputedStyle(backdrop).display).not.toBe("none");
    expect(screen.getByText("dialog")).toBeVisible();
  });

  it('keeps a "sessionTab" backdrop inside its container', () => {
    render(
      <div data-testid="container" className="hidden">
        <ModalBackdrop layer="sessionTab" className="fixed inset-0">
          <p>dialog</p>
        </ModalBackdrop>
      </div>,
    );
    const backdrop = document.querySelector(
      "[data-modal-backdrop]",
    ) as HTMLElement;
    expect(screen.getByTestId("container").contains(backdrop)).toBe(true);
    expect(backdrop.parentElement).toBe(screen.getByTestId("container"));
    expect(backdrop).not.toBeVisible();
  });
});
