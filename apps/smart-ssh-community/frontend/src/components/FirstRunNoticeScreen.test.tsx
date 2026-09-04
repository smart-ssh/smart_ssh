// Spec 0031, Abschnitt 4/6: "Weiter"-Button bleibt deaktiviert, bis die
// Checkbox aktiv ist — kein Wegklicken ohne bewusste Bestätigung.
import { fireEvent, render, screen } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { describe, expect, it, vi } from "vitest";
import { testI18n } from "../testI18n";
import { FirstRunNoticeScreen } from "./FirstRunNoticeScreen";

function renderScreen(onAcknowledge = vi.fn()) {
  render(
    <I18nextProvider i18n={testI18n}>
      <FirstRunNoticeScreen onAcknowledge={onAcknowledge} />
    </I18nextProvider>,
  );
  return onAcknowledge;
}

describe("first-run notice screen (Spec 0031)", () => {
  it("keeps the continue button disabled until the checkbox is checked", () => {
    renderScreen();
    const continueButton = screen.getByRole("button", { name: "Weiter" });
    expect(continueButton).toBeDisabled();

    fireEvent.click(screen.getByRole("checkbox"));
    expect(continueButton).toBeEnabled();

    fireEvent.click(screen.getByRole("checkbox"));
    expect(continueButton).toBeDisabled();
  });

  it("only calls onAcknowledge after the checkbox was checked and continue was clicked", () => {
    const onAcknowledge = renderScreen();
    const continueButton = screen.getByRole("button", { name: "Weiter" });

    // Ein deaktivierter Button feuert in jsdom (wie im echten Browser)
    // keinen Klick-Handler aus — trotzdem hier explizit geprüft, dass ohne
    // vorherige Checkbox-Aktivierung nichts passiert.
    fireEvent.click(continueButton);
    expect(onAcknowledge).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("checkbox"));
    fireEvent.click(continueButton);
    expect(onAcknowledge).toHaveBeenCalledTimes(1);
  });

  it("shows the exact responsibility and encryption text from spec 0031", () => {
    renderScreen();
    expect(screen.getByText(/Verantwortung für jedes bestätigte Kommando liegt bei dir/)).toBeInTheDocument();
    expect(screen.getByText(/nicht zusätzlich verschlüsselt/)).toBeInTheDocument();
  });
});
