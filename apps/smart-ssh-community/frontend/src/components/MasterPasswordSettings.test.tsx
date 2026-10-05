// Spec 0101, T14 (A13/A15/A18, Klarstellung 10b): die Einstellungen zum
// Master-Passwort — Modusanzeige, Einrichten mit ausdrücklicher
// Bestätigung, und die Frage vor dem Überschreiben eines fremden
// Schlüssels im Schlüsselbund.
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { getMasterPasswordMode, setUpMasterPassword, switchToOsKeychain } from "../api";
import { testI18n } from "../testI18n";
import { MasterPasswordSettings } from "./MasterPasswordSettings";

vi.mock("../api", () => ({
  getMasterPasswordMode: vi.fn(),
  setUpMasterPassword: vi.fn(() => Promise.resolve("password")),
  changeMasterPassword: vi.fn(() => Promise.resolve()),
  switchToOsKeychain: vi.fn(),
  commandErrorMessage: (err: unknown) =>
    typeof err === "object" && err !== null && "message" in err
      ? String((err as { message: unknown }).message)
      : String(err),
  commandErrorCode: (err: unknown) =>
    typeof err === "object" && err !== null && "code" in err
      ? ((err as { code: string | null }).code ?? null)
      : null,
}));

function renderSettings() {
  return render(
    <I18nextProvider i18n={testI18n}>
      <MasterPasswordSettings />
    </I18nextProvider>,
  );
}

describe("MasterPasswordSettings", () => {
  beforeEach(() => vi.clearAllMocks());

  it("zeigt den aktiven Modus (A18)", async () => {
    vi.mocked(getMasterPasswordMode).mockResolvedValue("keychain");
    const keychain = renderSettings();
    expect(await screen.findByText(/Im Schlüsselbund deines Betriebssystems/)).toBeTruthy();
    // Im Schlüsselbund-Modus gibt es nichts zu wechseln — der Rückweg
    // erscheint nur im Passwort-Modus.
    expect(screen.queryByRole("button", { name: "Zum Schlüsselbund wechseln" })).toBeNull();
    keychain.unmount();

    vi.mocked(getMasterPasswordMode).mockResolvedValue("password");
    renderSettings();
    expect(await screen.findByText(/Schlüsseldatei neben der Datenbank/)).toBeTruthy();
    expect(screen.getByRole("button", { name: "Zum Schlüsselbund wechseln" })).toBeTruthy();
    // Und umgekehrt: eingerichtet ist es schon.
    expect(screen.queryByRole("button", { name: "Master-Passwort einrichten" })).toBeNull();
  });

  it("richtet erst nach der ausdrücklichen Bestätigung der Warnung ein (A13/E10)", async () => {
    vi.mocked(getMasterPasswordMode).mockResolvedValue("keychain");
    renderSettings();

    const chosen = await screen.findByLabelText("Neues Master-Passwort");
    const repeated = screen.getByLabelText("Master-Passwort wiederholen");
    fireEvent.change(chosen, { target: { value: "Passwort-0101-lang" } });
    fireEvent.change(repeated, { target: { value: "Passwort-0101-lang" } });

    const button = screen.getByRole("button", { name: "Master-Passwort einrichten" });
    expect(button).toHaveProperty("disabled", true);
    fireEvent.click(button);
    expect(setUpMasterPassword).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("checkbox"));
    fireEvent.click(screen.getByRole("button", { name: "Master-Passwort einrichten" }));
    await waitFor(() =>
      expect(setUpMasterPassword).toHaveBeenCalledWith(
        "Passwort-0101-lang",
        "Passwort-0101-lang",
      ),
    );
  });

  it("lehnt ein zu kurzes Passwort ab, ohne das Backend zu fragen (A13)", async () => {
    vi.mocked(getMasterPasswordMode).mockResolvedValue("keychain");
    renderSettings();

    const chosen = await screen.findByLabelText("Neues Master-Passwort");
    fireEvent.change(chosen, { target: { value: "kurz-0101" } });
    fireEvent.change(screen.getByLabelText("Master-Passwort wiederholen"), {
      target: { value: "kurz-0101" },
    });
    fireEvent.click(screen.getByRole("checkbox"));

    expect(screen.getByText(/Mindestens 12 Zeichen/)).toBeTruthy();
    expect(
      screen.getByRole("button", { name: "Master-Passwort einrichten" }),
    ).toHaveProperty("disabled", true);
    expect(setUpMasterPassword).not.toHaveBeenCalled();
  });

  it("fragt vor dem Überschreiben eines fremden Schlüssels und lässt ohne Bestätigung alles, wie es war (Klarstellung 10b)", async () => {
    vi.mocked(getMasterPasswordMode).mockResolvedValue("password");
    vi.mocked(switchToOsKeychain).mockRejectedValue({
      message: "roher Backend-Text",
      code: "KEYCHAIN_HOLDS_ANOTHER_KEY",
    });
    renderSettings();

    fireEvent.change(await screen.findByLabelText("Master-Passwort zur Bestätigung"), {
      target: { value: "Passwort-0101-lang" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Zum Schlüsselbund wechseln" }));

    // Der erste Aufruf fragt **ohne** Bestätigung — das Backend fasst dabei
    // nichts an.
    await waitFor(() =>
      expect(switchToOsKeychain).toHaveBeenCalledWith("Passwort-0101-lang", false),
    );
    expect(await screen.findByText(/Backups, die mit diesem Schlüssel/)).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Abbrechen" }));
    await waitFor(() =>
      expect(screen.queryByText(/Backups, die mit diesem Schlüssel/)).toBeNull(),
    );
    // Kein zweiter Aufruf: Ohne ausdrückliche Bestätigung bleibt alles, wie
    // es war.
    expect(switchToOsKeychain).toHaveBeenCalledTimes(1);
  });

  it("ersetzt den fremden Schlüssel erst mit der Bestätigung (Klarstellung 10b)", async () => {
    vi.mocked(getMasterPasswordMode).mockResolvedValue("password");
    vi.mocked(switchToOsKeychain)
      .mockRejectedValueOnce({ message: "x", code: "KEYCHAIN_HOLDS_ANOTHER_KEY" })
      .mockResolvedValueOnce("keychain");
    renderSettings();

    fireEvent.change(await screen.findByLabelText("Master-Passwort zur Bestätigung"), {
      target: { value: "Passwort-0101-lang" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Zum Schlüsselbund wechseln" }));
    await screen.findByText(/Backups, die mit diesem Schlüssel/);

    fireEvent.click(screen.getByRole("button", { name: "Ja, ersetzen" }));
    await waitFor(() =>
      expect(switchToOsKeychain).toHaveBeenLastCalledWith("Passwort-0101-lang", true),
    );
  });

  it("zeigt den übersetzten Fehlertext, nicht den rohen des Backends (A20)", async () => {
    vi.mocked(getMasterPasswordMode).mockResolvedValue("keychain");
    vi.mocked(setUpMasterPassword).mockRejectedValue({
      message: "roher Backend-Text",
      code: "MASTER_PASSWORD_REJECTED",
    });
    renderSettings();

    fireEvent.change(await screen.findByLabelText("Neues Master-Passwort"), {
      target: { value: "Passwort-0101-lang" },
    });
    fireEvent.change(screen.getByLabelText("Master-Passwort wiederholen"), {
      target: { value: "Passwort-0101-lang" },
    });
    fireEvent.click(screen.getByRole("checkbox"));
    fireEvent.click(screen.getByRole("button", { name: "Master-Passwort einrichten" }));

    await waitFor(() => expect(screen.getByRole("alert").textContent).toContain("abgelehnt"));
    expect(screen.getByRole("alert").textContent).not.toContain("roher Backend-Text");
  });
});
