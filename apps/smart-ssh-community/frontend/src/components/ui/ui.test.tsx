import { fireEvent, render, screen } from "@testing-library/react";
import { clickDisabled } from "../../test/clickDisabled";
import { useRef } from "react";
import { describe, expect, it, vi } from "vitest";
import { Badge, Button, CornerTicks, DialogFrame, StatusSquare, Tabs, Field, FIELD_CONTROL_CLASS, Tooltip } from ".";

describe("Button", () => {
  it("defaults to type=button so it never submits a form by accident", () => {
    const onSubmit = vi.fn((e: React.FormEvent) => e.preventDefault());
    render(
      <form onSubmit={onSubmit}>
        <Button>Plain</Button>
      </form>,
    );
    fireEvent.click(screen.getByRole("button", { name: "Plain" }));
    expect(onSubmit).not.toHaveBeenCalled();
    expect(screen.getByRole("button")).toHaveAttribute("type", "button");
  });

  it("applies variant and size classes and forwards clicks", () => {
    const onClick = vi.fn();
    render(
      <Button variant="danger" size="sm" onClick={onClick}>
        Delete
      </Button>,
    );
    const b = screen.getByRole("button", { name: "Delete" });
    expect(b.className).toContain("text-danger-fg");
    expect(b.className).toContain("text-xs");
    fireEvent.click(b);
    expect(onClick).toHaveBeenCalledTimes(1);
  });

  it("does not fire when disabled", () => {
    const onClick = vi.fn();
    render(
      <Button disabled onClick={onClick}>
        Off
      </Button>,
    );
    clickDisabled(screen.getByRole("button"));
    expect(onClick).not.toHaveBeenCalled();
  });
});

describe("Field", () => {
  it("links label, help and error to the control", () => {
    render(
      <Field label="Host" help="The server name" error="Required">
        {(p) => <input className={FIELD_CONTROL_CLASS} {...p} />}
      </Field>,
    );
    const input = screen.getByLabelText("Host");
    expect(input).toHaveAttribute("aria-invalid", "true");
    const described = input.getAttribute("aria-describedby")!.split(" ");
    expect(described).toHaveLength(2);
    expect(document.getElementById(described[0])).toHaveTextContent(
      "The server name",
    );
    expect(document.getElementById(described[1])).toHaveTextContent("Required");
    expect(screen.getByRole("alert")).toHaveTextContent("Required");
  });

  it("sets no aria-describedby/aria-invalid without help or error", () => {
    render(<Field label="Name">{(p) => <input {...p} />}</Field>);
    const input = screen.getByLabelText("Name");
    expect(input).not.toHaveAttribute("aria-describedby");
    expect(input).not.toHaveAttribute("aria-invalid");
  });
});

function Harness({
  onClose,
  onSubmit,
}: {
  onClose: () => void;
  onSubmit?: () => void;
}) {
  const cancelRef = useRef<HTMLButtonElement>(null);
  return (
    <>
      <button>outside</button>
      <DialogFrame
        layer="app"
        title="Rename"
        onClose={onClose}
        initialFocusRef={cancelRef}
        onSubmit={
          onSubmit
            ? (e) => {
                e.preventDefault();
                onSubmit();
              }
            : undefined
        }
        actions={
          <>
            <Button ref={cancelRef} onClick={onClose}>
              Cancel
            </Button>
            <Button variant="primary" type="submit">
              Save
            </Button>
          </>
        }
      >
        <Field label="Name">{(p) => <input {...p} />}</Field>
      </DialogFrame>
    </>
  );
}

describe("DialogFrame", () => {
  it("is a labelled modal dialog and focuses the initial element", () => {
    render(<Harness onClose={() => {}} />);
    const dialog = screen.getByRole("dialog", { name: "Rename" });
    expect(dialog).toHaveAttribute("aria-modal", "true");
    expect(screen.getByRole("button", { name: "Cancel" })).toHaveFocus();
  });

  it("falls back to focusing the dialog itself", () => {
    render(
      <DialogFrame layer="app" title="T" onClose={() => {}}>
        text
      </DialogFrame>,
    );
    expect(screen.getByRole("dialog")).toHaveFocus();
  });

  it("closes on Escape", () => {
    const onClose = vi.fn();
    render(<Harness onClose={onClose} />);
    fireEvent.keyDown(document.activeElement!, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("wraps Tab and Shift+Tab inside the dialog", () => {
    render(<Harness onClose={() => {}} />);
    const dialog = screen.getByRole("dialog");
    const save = screen.getByRole("button", { name: "Save" });
    const name = screen.getByLabelText("Name");
    // order: Name field, Cancel, Save
    save.focus();
    fireEvent.keyDown(save, { key: "Tab" });
    expect(name).toHaveFocus();
    fireEvent.keyDown(name, { key: "Tab", shiftKey: true });
    expect(save).toHaveFocus();
    expect(dialog.contains(document.activeElement)).toBe(true);
  });

  it("puts body and actions in a form whose submit button is the primary action", () => {
    const onSubmit = vi.fn();
    render(<Harness onClose={() => {}} onSubmit={onSubmit} />);
    const save = screen.getByRole("button", { name: "Save" });
    expect(save).toHaveAttribute("type", "submit");
    expect(save.closest("form")).toBe(
      screen.getByLabelText("Name").closest("form"),
    );
    expect(screen.getByRole("button", { name: "Cancel" })).toHaveAttribute(
      "type",
      "button",
    );
    fireEvent.click(save);
    expect(onSubmit).toHaveBeenCalledTimes(1);
  });
});

describe("Tooltip", () => {
  it("shows on focus and hover, hides on Escape and blur, and describes the trigger", () => {
    render(<Tooltip content="Why this is risky">Label</Tooltip>);
    const trigger = screen.getByText("Label");
    expect(trigger).toHaveAccessibleDescription("Why this is risky");
    expect(screen.getByRole("tooltip", { hidden: true })).not.toBeVisible();
    fireEvent.focus(trigger);
    expect(screen.getByRole("tooltip")).toBeVisible();
    fireEvent.keyDown(trigger, { key: "Escape" });
    expect(screen.getByRole("tooltip", { hidden: true })).not.toBeVisible();
    fireEvent.mouseEnter(trigger);
    expect(screen.getByRole("tooltip")).toBeVisible();
    fireEvent.mouseLeave(trigger);
    fireEvent.focus(trigger);
    fireEvent.blur(trigger);
    expect(screen.getByRole("tooltip", { hidden: true })).not.toBeVisible();
  });
});

describe("Angular primitives (issue #1362)", () => {
  it("Button: primary/secondary/ghost variants and iconOnly square", () => {
    render(
      <>
        <Button variant="primary">P</Button>
        <Button variant="secondary">S</Button>
        <Button variant="ghost">G</Button>
        <Button iconOnly aria-label="Icon">i</Button>
      </>,
    );
    expect(screen.getByRole("button", { name: "P" }).className).toContain("bg-accent text-on-accent");
    expect(screen.getByRole("button", { name: "S" }).className).toContain("border-border-strong");
    expect(screen.getByRole("button", { name: "S" }).className).toContain("label-condensed");
    expect(screen.getByRole("button", { name: "G" }).className).toContain("text-fg-muted");
    expect(screen.getByRole("button", { name: "S" }).className).toContain("h-8");
    const icon = screen.getByRole("button", { name: "Icon" });
    expect(icon.className).toContain("w-8");
    expect(icon.className).toContain("h-8");
  });

  it("Field label uses the uppercase label style", () => {
    render(<Field label="Host">{(p) => <input {...p} />}</Field>);
    expect(screen.getByText("Host").className).toContain("text-label");
  });

  it("StatusSquare is decorative, square, and can pulse", () => {
    const { container } = render(<><StatusSquare tone="success" /><StatusSquare tone="warning" pulse /></>);
    const [a, b] = Array.from(container.querySelectorAll("[data-status-square]")) as HTMLElement[];
    expect(a).toHaveAttribute("aria-hidden", "true");
    expect(a.className).toContain("bg-success");
    expect(a.style.width).toBe("8px");
    expect(a.className).not.toContain("rounded");
    expect(b.className).toContain("animate-pulse");
  });

  it("Badge renders its tone and an optional status square", () => {
    const { container } = render(<Badge tone="danger" square>Blocked</Badge>);
    expect(screen.getByText("Blocked").className).toContain("border-danger-border");
    expect(screen.getByText("Blocked").className).toContain("h-5");
    expect(container.querySelector('[data-status-square="danger"]')).not.toBeNull();
  });

  it("CornerTicks: top draws two ticks, all draws four, hidden from AT", () => {
    const { container, rerender } = render(<CornerTicks />);
    expect(container.querySelectorAll("[data-corner-tick]")).toHaveLength(2);
    rerender(<CornerTicks corners="all" tone="danger" />);
    const ticks = container.querySelectorAll("[data-corner-tick]");
    expect(ticks).toHaveLength(4);
    expect(ticks[0]).toHaveAttribute("aria-hidden", "true");
    expect(ticks[0].className).toContain("border-danger");
  });

  it("DialogFrame draws ticks only when asked, with a hidden-in-narrow wrapper", () => {
    const { rerender } = render(
      <DialogFrame layer="app" title="Plain" onClose={() => {}}>x</DialogFrame>,
    );
    expect(document.querySelectorAll("[data-corner-tick]")).toHaveLength(0);
    rerender(<DialogFrame layer="app" title="Key" ticks="danger" tone="danger" onClose={() => {}}>x</DialogFrame>);
    expect(document.querySelectorAll("[data-corner-tick]")).toHaveLength(2);
    expect(screen.getByTestId("dialog-ticks").className).toContain("narrow:hidden");
    const panel = screen.getByRole("dialog");
    expect(panel.className).toContain("border-danger-border");
    expect(panel.className).toContain("shadow-offset-lg");
    expect(screen.getByRole("heading", { name: "Key" }).className).toContain("text-title");
  });

  it("Tabs: active tab has the accent top rule, invalid shows a square", () => {
    const { container } = render(
      <Tabs
        idBase="t"
        ariaLabel="x"
        value="a"
        onChange={() => {}}
        tabs={[{ id: "a", label: "A" }, { id: "b", label: "B", invalid: true, invalidLabel: "bad" }]}
      />,
    );
    expect(screen.getByRole("tab", { name: "A" }).className).toContain("border-accent");
    expect(container.querySelector('[data-status-square="danger"]')).not.toBeNull();
  });
});
