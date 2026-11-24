import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { clickDisabled } from "../../test/clickDisabled";
import { Menu, MenuItem, MenuSeparator } from "./Menu";

function setup(onClose = vi.fn()) {
  render(
    <div>
      <button type="button">opener</button>
      <p>outside</p>
      <Menu label="Actions" anchor={{ x: 10, y: 10 }} onClose={onClose}>
        <MenuItem onClick={vi.fn()}>One</MenuItem>
        <MenuItem disabled>Two</MenuItem>
        <MenuSeparator />
        <MenuItem danger shortcut="Del">Three</MenuItem>
      </Menu>
    </div>,
  );
  return onClose;
}

describe("Menu", () => {
  it("exposes menu, menuitems and separator, and focuses the first item", () => {
    setup();
    expect(screen.getByRole("menu", { name: "Actions" })).toBeInTheDocument();
    expect(screen.getAllByRole("menuitem")).toHaveLength(3);
    expect(screen.getByRole("separator")).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "One" })).toHaveFocus();
  });

  it("moves focus with arrows, Home and End and skips disabled items", () => {
    setup();
    const one = screen.getByRole("menuitem", { name: "One" });
    const three = screen.getByRole("menuitem", { name: /Three/ });
    fireEvent.keyDown(one, { key: "ArrowDown" });
    expect(three).toHaveFocus();
    fireEvent.keyDown(three, { key: "ArrowDown" });
    expect(one).toHaveFocus();
    fireEvent.keyDown(one, { key: "ArrowUp" });
    expect(three).toHaveFocus();
    fireEvent.keyDown(three, { key: "Home" });
    expect(one).toHaveFocus();
    fireEvent.keyDown(one, { key: "End" });
    expect(three).toHaveFocus();
  });

  it("Escape closes and returns focus to the opener", () => {
    const opener = document.createElement("button");
    document.body.appendChild(opener);
    opener.focus();
    const onClose = vi.fn();
    render(
      <Menu label="A" anchor={{ x: 0, y: 0 }} onClose={onClose}>
        <MenuItem>One</MenuItem>
      </Menu>,
    );
    expect(screen.getByRole("menuitem")).toHaveFocus();
    fireEvent.keyDown(screen.getByRole("menuitem"), { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);
    expect(opener).toHaveFocus();
    opener.remove();
  });

  it("Tab closes", () => {
    const onClose = setup();
    fireEvent.keyDown(screen.getByRole("menuitem", { name: "One" }), { key: "Tab" });
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("a click outside closes, a click on the menu padding does not", () => {
    const onClose = setup();
    fireEvent.click(screen.getByRole("menu"));
    expect(onClose).not.toHaveBeenCalled();
    fireEvent.click(screen.getByText("outside"));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("activating an enabled item runs it and closes; a disabled one does nothing", () => {
    const onClose = setup();
    clickDisabled(screen.getByRole("menuitem", { name: "Two" }));
    expect(onClose).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("menuitem", { name: "One" }));
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});
