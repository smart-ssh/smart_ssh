import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import { clickDisabled } from "./clickDisabled";

describe("fireEvent guard (ADR 0122 R13/R14)", () => {
  it("throws on a detached node", () => {
    const { unmount } = render(<button>Cancel</button>);
    const button = screen.getByText("Cancel");
    unmount();
    expect(() => fireEvent.click(button)).toThrow(/detached.*Cancel.*R14/s);
    expect(() => fireEvent(button, new MouseEvent("click"))).toThrow(/R14/);
  });

  it("throws on a disabled button, passes through clickDisabled", () => {
    const onClick = vi.fn();
    render(
      <button disabled onClick={onClick}>
        Go
      </button>,
    );
    const button = screen.getByText("Go");
    expect(() => fireEvent.click(button)).toThrow(/disabled.*R13/s);
    clickDisabled(button);
    expect(onClick).not.toHaveBeenCalled();
  });

  it("throws on a button inside a disabled fieldset", () => {
    render(
      <fieldset disabled>
        <button>Inner</button>
      </fieldset>,
    );
    const button = screen.getByText("Inner");
    expect(() => fireEvent.click(button)).toThrow(/R13/);
    clickDisabled(button);
  });

  it("clickDisabled asserts the element is disabled", () => {
    render(<button>On</button>);
    expect(() => clickDisabled(screen.getByText("On"))).toThrow();
  });

  it("clickDisabled still throws on a detached element", () => {
    const { unmount } = render(<button disabled>Gone</button>);
    const button = screen.getByText("Gone");
    unmount();
    expect(() => clickDisabled(button)).toThrow(/R14/);
  });

  it("leaves connected enabled targets, window and document alone", () => {
    const onClick = vi.fn();
    render(<button onClick={onClick}>Ok</button>);
    fireEvent.click(screen.getByText("Ok"));
    expect(onClick).toHaveBeenCalledTimes(1);
    fireEvent.keyDown(document, { key: "a" });
    fireEvent.resize(window);
  });
});
