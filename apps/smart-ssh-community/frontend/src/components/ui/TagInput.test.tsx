import { fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { describe, expect, it, vi } from "vitest";
import { TagInput } from ".";

function Harness({ initial = [], ...rest }: { initial?: string[]; readOnlyTags?: string[]; disabled?: boolean; spy?: (n: string[]) => void }) {
  const [tags, setTags] = useState(initial);
  return (
    <TagInput
      value={tags}
      onChange={(n) => {
        rest.spy?.(n);
        setTags(n);
      }}
      readOnlyTags={rest.readOnlyTags}
      readOnlyTitle="from server"
      disabled={rest.disabled}
      aria-label="Tags"
    />
  );
}
const input = () => screen.getByRole("textbox", { name: "Tags" });
const type = (v: string) => fireEvent.change(input(), { target: { value: v } });

describe("TagInput", () => {
  it("adds by Enter, by comma and by blur", () => {
    render(<Harness />);
    type(" a ");
    fireEvent.keyDown(input(), { key: "Enter" });
    type("b");
    fireEvent.keyDown(input(), { key: "," });
    type("c");
    fireEvent.blur(input());
    expect(screen.getAllByRole("button").map((b) => b.getAttribute("aria-label"))).toHaveLength(3);
    expect(screen.getByText("a")).toBeInTheDocument();
    expect(screen.getByText("b")).toBeInTheDocument();
    expect(screen.getByText("c")).toBeInTheDocument();
    expect(input()).toHaveValue("");
  });

  it("ignores duplicates and whitespace-only drafts", () => {
    const spy = vi.fn();
    render(<Harness initial={["a"]} spy={spy} />);
    type("a");
    fireEvent.keyDown(input(), { key: "Enter" });
    type("   ");
    fireEvent.keyDown(input(), { key: "Enter" });
    expect(spy).not.toHaveBeenCalled();
    expect(input()).toHaveValue("");
  });

  it("Backspace on an empty draft removes the last tag, not with a draft", () => {
    const spy = vi.fn();
    render(<Harness initial={["a", "b"]} spy={spy} />);
    type("x");
    fireEvent.keyDown(input(), { key: "Backspace" });
    expect(spy).not.toHaveBeenCalled();
    type("");
    fireEvent.keyDown(input(), { key: "Backspace" });
    expect(spy).toHaveBeenLastCalledWith(["a"]);
  });

  it("remove button removes the right tag and is labelled", () => {
    const spy = vi.fn();
    render(<Harness initial={["a", "b"]} spy={spy} />);
    const buttons = screen.getAllByRole("button");
    expect(buttons[0]).toHaveAccessibleName(/a/);
    fireEvent.click(buttons[0]);
    expect(spy).toHaveBeenLastCalledWith(["b"]);
  });

  it("read-only tags have a title and no remove button", () => {
    render(<Harness initial={["a"]} readOnlyTags={["srv"]} />);
    expect(screen.getByText("srv")).toHaveAttribute("title", "from server");
    expect(screen.getAllByRole("button")).toHaveLength(1);
  });

  it("disabled state blocks input and removal", () => {
    render(<Harness initial={["a"]} disabled />);
    expect(input()).toBeDisabled();
    expect(screen.getByRole("button")).toBeDisabled();
  });
});
