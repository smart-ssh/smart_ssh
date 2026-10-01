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

  // Spec 0098, T8 (A4/A5/A6): Der Verbindungstest meldet einen
  // Schlüsselbund-Fehler als `networkError` mit `KEYCHAIN_ACCESS_FAILED` —
  // die Variante des Ergebnisses bleibt (§5), der Code trägt die Aussage.
  // Die Anzeige darf deshalb weder „Netzwerkfehler" sagen (es ist keiner)
  // noch den rohen Backend-Text zeigen. `message` enthält hier den
  // `Display`-Text aus `core`; die Nutzlast der Bibliothek steht seit
  // Commit 2 nicht mehr darin, aber angezeigt werden darf auch dieser Text
  // nicht.
  //
  // *Gegenbeweis:* ohne den Eintrag in `KNOWN_ERROR_CODES` greift
  // `translateErrorCode` auf den Fallback zurück und die Zeile lautet
  // „✗ Netzwerkfehler: deploy@jump.example:22: Zugriff auf den
  // Schlüsselbund fehlgeschlagen (Passwort)" — beide Erwartungen unten
  // schlagen dann fehl.
  it("shows the keychain reason for a networkError, not a network error text", () => {
    renderBadge({
      kind: "networkError",
      message: "deploy@jump.example:22: Zugriff auf den Schlüsselbund fehlgeschlagen (Passwort)",
      code: "KEYCHAIN_ACCESS_FAILED",
    });

    expect(screen.getByText(/gesperrt/)).toBeInTheDocument();
    expect(screen.queryByText(/Netzwerkfehler/)).not.toBeInTheDocument();
    expect(screen.queryByText(/deploy@jump\.example:22/)).not.toBeInTheDocument();
  });

  it("still shows the plain success text unaffected by the code change", () => {
    renderBadge({ kind: "success" });

    expect(screen.getByText("✓ Verbindung erfolgreich")).toBeInTheDocument();
  });
});
