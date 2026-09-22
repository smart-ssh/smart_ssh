// Spec 0069, Teil A5, Test 20: `TestResultBadge` (Server-Formular) zeigt
// bei `networkError` mit einem bekannten Code die übersetzte Meldung statt
// des rohen Backend-Texts; ohne Code bleibt das bisherige Verhalten
// (rohe `message` in der Standard-Meldung) unverändert.
import { render, screen } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { describe, expect, it } from "vitest";
import { testI18n } from "../testI18n";
import type { TestConnectionResult } from "../types";
import { TestResultBadge } from "./ServerForm";

function renderBadge(result: TestConnectionResult) {
  return render(
    <I18nextProvider i18n={testI18n}>
      <TestResultBadge result={result} />
    </I18nextProvider>,
  );
}

describe("TestResultBadge (Spec 0069, Teil A5)", () => {
  it("shows the translated reason for a networkError with a known code", () => {
    renderBadge({
      kind: "networkError",
      message: "Connection refused (os error 61)",
      code: "SSH_CONNECTION_REFUSED",
    });

    expect(screen.getByText(/lehnt die Verbindung ab/)).toBeInTheDocument();
    expect(screen.queryByText(/Connection refused \(os error 61\)/)).not.toBeInTheDocument();
  });

  // *Gegenbeweis (s. Bericht):* vor diesem Schritt gab es kein `code`-Feld
  // und `TestResultBadge` zeigte immer den rohen `message`-Text — dieser
  // Test (und der obige) belegen den neuen Zweig.
  it("falls back to the raw message when no code is present", () => {
    renderBadge({
      kind: "networkError",
      message: "some raw backend text",
      code: null,
    });

    expect(screen.getByText("✗ Netzwerkfehler: some raw backend text")).toBeInTheDocument();
  });

  it("still shows the plain success text unaffected by the code change", () => {
    renderBadge({ kind: "success" });

    expect(screen.getByText("✓ Verbindung erfolgreich")).toBeInTheDocument();
  });
});
