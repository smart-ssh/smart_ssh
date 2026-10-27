import { expect } from "vitest";
import { fireEvent } from "@testing-library/react";
import { withDisabledAllowed } from "./fireEventGuard";

/**
 * Clicks an element that the test expects to be disabled, to prove the control
 * does nothing (ADR 0122, R13). Asserts the element is disabled first; a
 * detached element still throws.
 */
export function clickDisabled(element: Element): void {
  expect(element.matches(":disabled")).toBe(true);
  withDisabledAllowed(() => fireEvent.click(element));
}
