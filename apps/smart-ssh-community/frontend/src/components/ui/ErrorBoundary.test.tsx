import { render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ErrorBoundary } from "./ErrorBoundary";

function Thrower({ fail }: { fail: boolean }) {
  if (fail) throw new Error("boom-secret");
  return <p>child-ok</p>;
}

describe("ErrorBoundary (issue #880)", () => {
  let consoleError: ReturnType<typeof vi.spyOn>;
  beforeEach(() => {
    consoleError = vi.spyOn(console, "error").mockImplementation(() => {});
  });
  afterEach(() => consoleError.mockRestore());

  it("renders the fallback, calls onError once with the id and logs", () => {
    const onError = vi.fn();
    render(
      <ErrorBoundary id="x" fallback={<p>fallback</p>} onError={onError}>
        <Thrower fail />
      </ErrorBoundary>,
    );
    expect(screen.getByText("fallback")).toBeInTheDocument();
    expect(screen.queryByText(/boom-secret/)).toBeNull();
    expect(onError).toHaveBeenCalledTimes(1);
    expect(onError.mock.calls[0][0]).toBe("x");
    expect(consoleError.mock.calls.some((c) => String(c[0]).includes('"x"'))).toBe(true);
  });

  it("renders nothing by default", () => {
    const { container } = render(
      <ErrorBoundary id="x">
        <Thrower fail />
      </ErrorBoundary>,
    );
    expect(container).toBeEmptyDOMElement();
  });

  it("renders the child again when a reset key changes", () => {
    const ui = (key: string, fail: boolean) => (
      <ErrorBoundary id="x" resetKeys={[key]}>
        <Thrower fail={fail} />
      </ErrorBoundary>
    );
    const { rerender } = render(ui("a", true));
    expect(screen.queryByText("child-ok")).toBeNull();
    rerender(ui("a", false));
    expect(screen.queryByText("child-ok")).toBeNull();
    rerender(ui("b", false));
    expect(screen.getByText("child-ok")).toBeInTheDocument();
  });
});
