import { fireEvent, render, screen } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { RootErrorScreen } from "./RootErrorScreen";
import { ErrorBoundary } from "./ui";
import { testI18n } from "../testI18n";

function Broken(): never {
  throw new Error("outside any slot");
}

describe("root error boundary (issue #880)", () => {
  let consoleError: ReturnType<typeof vi.spyOn>;
  const reload = vi.fn();
  beforeEach(() => {
    consoleError = vi.spyOn(console, "error").mockImplementation(() => {});
    vi.stubGlobal("location", { ...window.location, reload });
  });
  afterEach(() => {
    consoleError.mockRestore();
    vi.unstubAllGlobals();
    reload.mockClear();
  });

  it("shows the recovery screen and reloads on click", () => {
    render(
      <I18nextProvider i18n={testI18n}>
        <ErrorBoundary id="app-root" fallback={<RootErrorScreen />}>
          <div>
            <Broken />
          </div>
        </ErrorBoundary>
      </I18nextProvider>,
    );
    expect(screen.getByRole("heading")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button"));
    expect(reload).toHaveBeenCalledTimes(1);
    expect(consoleError).toHaveBeenCalled();
  });
});
