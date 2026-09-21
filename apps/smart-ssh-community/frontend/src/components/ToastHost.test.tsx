// Spec 0067, Teil B2: Erfolg verschwindet automatisch, Fehler bleibt stehen.
import { act, fireEvent, render, screen } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { testI18n } from "../testI18n";
import { showToast } from "../toastBus";
import { SUCCESS_TOAST_MS, ToastHost } from "./ToastHost";

function renderHost() {
  return render(
    <I18nextProvider i18n={testI18n}>
      <ToastHost />
    </I18nextProvider>,
  );
}

describe("ToastHost (Spec 0067, Teil B)", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("hides a success toast automatically", () => {
    renderHost();
    act(() => showToast({ kind: "success", message: "„a.txt“ gelöscht" }));
    expect(screen.getByText(/a\.txt“ gelöscht/)).toBeInTheDocument();

    act(() => {
      vi.advanceTimersByTime(SUCCESS_TOAST_MS + 10);
    });

    expect(screen.queryByText(/a\.txt“ gelöscht/)).not.toBeInTheDocument();
  });

  it("keeps an error toast until it is dismissed", () => {
    renderHost();
    act(() => showToast({ kind: "error", message: "Löschen fehlgeschlagen: Keine Berechtigung" }));

    act(() => {
      vi.advanceTimersByTime(SUCCESS_TOAST_MS * 5);
    });
    expect(screen.getByText(/Keine Berechtigung/)).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Meldung schließen" }));
    expect(screen.queryByText(/Keine Berechtigung/)).not.toBeInTheDocument();
  });

  it("runs the toast action and closes the toast", () => {
    const onClick = vi.fn();
    renderHost();
    act(() =>
      showToast({ kind: "success", message: "heruntergeladen", action: { label: "Im Finder zeigen", onClick } }),
    );

    fireEvent.click(screen.getByRole("button", { name: "Im Finder zeigen" }));

    expect(onClick).toHaveBeenCalled();
    expect(screen.queryByText(/heruntergeladen/)).not.toBeInTheDocument();
  });
});
