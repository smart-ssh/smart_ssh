// Spec 0101, T14 (A16/A18/A20): die Entsperrmaske — falsches Passwort zeigt
// eine Meldung, die Codes der Etappe 3 sind übersetzt, und „Neu anfangen"
// erscheint genau dort, wo das Backend es anbietet.
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  answerStartupPrompt,
  getStartupState,
  quitApplication,
  startOverFromUnlockScreen,
  unlockWithMasterPassword,
} from "../api";
import { onStartupPrompt, onStartupUnlocked } from "../events";
import type { StartupPromptRequest, StartupStateDto } from "../types";
import { testI18n } from "../testI18n";
import { StartupGate } from "./StartupGate";

vi.mock("../api", () => ({
  getStartupState: vi.fn(),
  unlockWithMasterPassword: vi.fn(),
  startOverFromUnlockScreen: vi.fn(),
  answerStartupPrompt: vi.fn(() => Promise.resolve()),
  quitApplication: vi.fn(() => Promise.resolve()),
  // Die echten Extraktionen: Der Test prüft unter anderem, dass ein
  // `code` übersetzt wird (A20) — mit einer vereinfachten Attrappe prüfte
  // er nur die Attrappe.
  commandErrorMessage: (err: unknown) =>
    typeof err === "object" && err !== null && "message" in err
      ? String((err as { message: unknown }).message)
      : String(err),
  commandErrorCode: (err: unknown) =>
    typeof err === "object" && err !== null && "code" in err
      ? ((err as { code: string | null }).code ?? null)
      : null,
}));

vi.mock("../events", () => ({
  onStartupPrompt: vi.fn(() => Promise.resolve(() => {})),
  onStartupUnlocked: vi.fn(() => Promise.resolve(() => {})),
}));

// `../i18n` spricht beim Import die Tauri-Plugins `store`/`os` an (s.
// `testI18n.ts`) — in einem Test gibt es die nicht.
vi.mock("../i18n", () => ({
  applyStoredLanguage: vi.fn(() => Promise.resolve()),
}));

function state(overrides: Partial<StartupStateDto> = {}): StartupStateDto {
  return {
    screen: "unlock",
    mode: "password",
    offersStartOver: false,
    failedUnlockAttempts: 0,
    language: "de",
    ...overrides,
  };
}

function renderGate(initial: StartupStateDto | null) {
  return render(
    <I18nextProvider i18n={testI18n}>
      <StartupGate initialState={initial}>
        <div>DIE-APP-0101</div>
      </StartupGate>
    </I18nextProvider>,
  );
}

/** Die Frage, die der Zuhörer bekommen hätte — aus dem letzten
 * `onStartupPrompt`-Aufruf. */
function deliverPrompt(request: StartupPromptRequest) {
  const handler = vi.mocked(onStartupPrompt).mock.calls.at(-1)?.[0];
  if (!handler) throw new Error("kein Zuhörer für `startup:prompt` registriert");
  handler(request);
}

describe("StartupGate", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(onStartupPrompt).mockResolvedValue(() => {});
    vi.mocked(onStartupUnlocked).mockResolvedValue(() => {});
    vi.mocked(getStartupState).mockResolvedValue(state());
  });

  it("zeigt die App erst, wenn der Zustand steht (A16)", async () => {
    renderGate(state({ screen: "unlocked", mode: "keychain" }));
    expect(screen.getByText("DIE-APP-0101")).toBeTruthy();
  });

  it("zeigt im gesperrten Zustand die Entsperrmaske und nicht die App (A16)", async () => {
    renderGate(state());

    expect(screen.queryByText("DIE-APP-0101")).toBeNull();
    expect(screen.getByLabelText(/Master-Passwort/)).toBeTruthy();
    expect(screen.getByRole("button", { name: "Entsperren" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Beenden" })).toBeTruthy();
  });

  it("zeigt bei falschem Passwort die übersetzte Meldung und entsperrt nicht (A16/A20)", async () => {
    vi.mocked(unlockWithMasterPassword).mockRejectedValue({
      message: "roher Backend-Text",
      code: "WRONG_MASTER_PASSWORD",
    });
    vi.mocked(getStartupState).mockResolvedValue(state({ failedUnlockAttempts: 1 }));
    renderGate(state());

    fireEvent.change(screen.getByLabelText(/Master-Passwort/), {
      target: { value: "falsch-0101" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Entsperren" }));

    await waitFor(() => {
      expect(screen.getByRole("alert").textContent).toContain("Passwort falsch oder Datei");
    });
    // A20: der übersetzte Text, nicht der rohe des Backends.
    expect(screen.getByRole("alert").textContent).not.toContain("roher Backend-Text");
    expect(screen.queryByText("DIE-APP-0101")).toBeNull();
    // Klarstellung 11: Der Zähler wird nachgefragt, damit der Ausweg nicht
    // erst beim nächsten Start erscheint.
    await waitFor(() => expect(getStartupState).toHaveBeenCalled());
  });

  it("bietet „Neu anfangen“ nur an, wenn das Backend es anbietet (Klarstellung 11)", async () => {
    const withoutWayOut = renderGate(state({ failedUnlockAttempts: 2 }));
    expect(screen.queryByRole("button", { name: "Neu anfangen" })).toBeNull();
    withoutWayOut.unmount();

    renderGate(state({ failedUnlockAttempts: 3, offersStartOver: true }));
    expect(screen.getByRole("button", { name: "Neu anfangen" })).toBeTruthy();
  });

  it("zeigt bei *nicht erreichbarer* Verpackung „Erneut versuchen“ und nie „Neu anfangen“ (Klarstellung 10a/11)", async () => {
    renderGate(
      state({ screen: "unreachableWrapping", failedUnlockAttempts: 9, offersStartOver: false }),
    );

    expect(screen.getByRole("button", { name: "Erneut versuchen" })).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Neu anfangen" })).toBeNull();
    // Kein Passwortfeld: Über den Inhalt der Datei ist nichts gesagt, und
    // ein Passwort kann sie nicht lesbar machen.
    expect(screen.queryByLabelText(/Master-Passwort/)).toBeNull();
  });

  it("zeigt bei *ungültiger* Verpackung den Ausweg statt eines Passwortfelds (Klarstellung 9)", async () => {
    vi.mocked(startOverFromUnlockScreen).mockResolvedValue(state({ screen: "unlocked" }));
    renderGate(state({ screen: "unusableWrapping", offersStartOver: true }));

    expect(screen.queryByLabelText(/Master-Passwort/)).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Neu anfangen" }));
    await waitFor(() => expect(startOverFromUnlockScreen).toHaveBeenCalledTimes(1));
  });

  it("beendet über das Kommando, nicht über das Fenster (A16)", async () => {
    renderGate(state());
    fireEvent.click(screen.getByRole("button", { name: "Beenden" }));
    expect(quitApplication).toHaveBeenCalledTimes(1);
  });

  it("schickt für einen reinen Hinweis **keine** Antwort ans Backend (Klarstellung 10e)", async () => {
    renderGate(state());
    await waitFor(() => expect(onStartupPrompt).toHaveBeenCalled());

    deliverPrompt({
      kind: "notice",
      title: "Umbenannt",
      message: "Die Dateien heißen jetzt smart-ssh.db.unreadable-0101.",
    });

    const ok = await screen.findByRole("button", { name: "OK" });
    fireEvent.click(ok);
    // Der Startablauf wartet dort auf nichts; eine Antwort wäre eine
    // Antwort ohne Frage — und könnte die nächste, echte Frage vorab
    // beantworten.
    expect(answerStartupPrompt).not.toHaveBeenCalled();
    await waitFor(() => expect(screen.queryByRole("button", { name: "OK" })).toBeNull());
  });

  it("meldet es sichtbar, wenn sich die Zuhörer nicht anmelden lassen (spec-reviewer Runde 2)", async () => {
    // Scheitert `listen`, bleibt der Merker `listening` aus — und damit
    // kehrt der Effekt, der den Start im Fenster fortsetzt, immer früh
    // zurück. Ohne das `catch` an der Anmelde-IIFE stünde davon nur eine
    // unbehandelte Ablehnung in der Konsole: Der Bildschirm zeigte endlos
    // „wird fortgesetzt", und der Ausweg wäre wirkungslos **und** stumm.
    //
    // **Gegenbeweis geführt:** Ohne das `catch` erscheint keine Meldung,
    // und diese Zusicherung läuft in die Zeitgrenze.
    vi.mocked(onStartupPrompt).mockRejectedValueOnce(new Error("kein Zuhörer"));

    renderGate(state({ screen: "setUpMasterPassword", mode: "keychain" }));

    expect(await screen.findByText(/kein Zuhörer/)).toBeTruthy();
  });

  it("zeigt bei einer echten Frage nur die Knöpfe, die ihre Art anbietet (A3)", async () => {
    renderGate(state());
    await waitFor(() => expect(onStartupPrompt).toHaveBeenCalled());

    deliverPrompt({ kind: "startOverOrQuit", title: "D3", message: "Schlüssel unbrauchbar" });

    expect(await screen.findByRole("button", { name: "Neu anfangen" })).toBeTruthy();
    // Keine Wahl, die D3 nicht anbietet.
    expect(screen.queryByRole("button", { name: "Neuen Schlüssel erzeugen" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Ohne Übernahme fortfahren" })).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Neu anfangen" }));
    // Klarstellung 12: Eine Antwort ohne Passwort trägt auch keine
    // Bestätigung — das Backend liest sie dort ohnehin nicht, und ein
    // `true` an dieser Stelle wäre eine Zusage, die niemand gegeben hat.
    await waitFor(() =>
      expect(answerStartupPrompt).toHaveBeenCalledWith(
        "startOver",
        undefined,
        undefined,
        undefined,
      ),
    );
  });

  it("fragt ein neues Master-Passwort mit leeren Feldern und ausdrücklicher Bestätigung ab (A13/E10, Klarstellung 10e)", async () => {
    renderGate(state());
    await waitFor(() => expect(onStartupPrompt).toHaveBeenCalled());

    deliverPrompt({
      kind: "newMasterPassword",
      title: "Master-Passwort einrichten",
      message: "Ohne Passwort sind alle Daten verloren.",
    });

    const chosen = (await screen.findByLabelText("Neues Master-Passwort")) as HTMLInputElement;
    const repeated = screen.getByLabelText("Master-Passwort wiederholen") as HTMLInputElement;
    // Klarstellung 10e: ein geöffnetes Passwortfeld ist leer.
    expect(chosen.value).toBe("");
    expect(repeated.value).toBe("");

    const confirm = screen.getByRole("button", { name: "Ja, fortfahren" });
    expect(confirm).toHaveProperty("disabled", true);

    fireEvent.change(chosen, { target: { value: "Passwort-0101-lang" } });
    fireEvent.change(repeated, { target: { value: "Passwort-0101-lang" } });
    // A13/E10: Ohne die ausdrückliche Bestätigung der Warnung bleibt der
    // Knopf aus — auch bei gültigem Passwort.
    expect(screen.getByRole("button", { name: "Ja, fortfahren" })).toHaveProperty(
      "disabled",
      true,
    );

    fireEvent.click(screen.getByRole("checkbox"));
    fireEvent.click(screen.getByRole("button", { name: "Ja, fortfahren" }));
    await waitFor(() =>
      // Klarstellung 12: dieselbe Zusage wie in den Einstellungen, hier
      // über den Startdialog — sie reist als vierter Parameter mit.
      expect(answerStartupPrompt).toHaveBeenCalledWith(
        "confirm",
        "Passwort-0101-lang",
        "Passwort-0101-lang",
        true,
      ),
    );
  });

  it("lässt auch die Einrichtemaske aus D1 einen Ausweg (A16, Spec 0059)", async () => {
    // Review-Fund Runde 1: Wählt der Nutzer im Dialog D1 „Beenden", kommt
    // der Startablauf als Fehler zurück, und dieser Bildschirm bleibt
    // stehen. Ohne eigene Knöpfe hätte „Beenden" dann nicht beendet, und es
    // gäbe keinen Weg mehr aus der App außer dem Fenstersystem.
    vi.mocked(unlockWithMasterPassword).mockRejectedValue({
      message: "Der Start wurde abgebrochen. Es ist nichts verändert.",
      code: "STARTUP_FAILED",
    });
    vi.mocked(getStartupState).mockResolvedValue(
      state({ screen: "setUpMasterPassword", mode: "keychain" }),
    );
    renderGate(state({ screen: "setUpMasterPassword", mode: "keychain" }));

    await waitFor(() => expect(screen.getByRole("alert")).toBeTruthy());
    expect(screen.getByRole("button", { name: "Beenden" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Erneut versuchen" })).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Beenden" }));
    expect(quitApplication).toHaveBeenCalledTimes(1);
  });

  it("setzt den Startablauf nach „Erneut versuchen“ erneut fort (A16)", async () => {
    vi.mocked(unlockWithMasterPassword).mockRejectedValue({ message: "x", code: "STARTUP_FAILED" });
    vi.mocked(getStartupState).mockResolvedValue(
      state({ screen: "setUpMasterPassword", mode: "keychain" }),
    );
    renderGate(state({ screen: "setUpMasterPassword", mode: "keychain" }));

    await waitFor(() => expect(unlockWithMasterPassword).toHaveBeenCalledTimes(1));
    fireEvent.click(screen.getByRole("button", { name: "Erneut versuchen" }));
    // Ohne das Zurücksetzen des Merkers **und** eine Abhängigkeit, die sich
    // ändert, bliebe es bei einem Aufruf — der Knopf wäre dann wirkungslos.
    await waitFor(() => expect(unlockWithMasterPassword).toHaveBeenCalledTimes(2));
  });

  it("zeigt den Hinweis aus A5 auch nach der Entsperrung (Klarstellung 10e)", async () => {
    renderGate(state());
    await waitFor(() => expect(onStartupPrompt).toHaveBeenCalled());

    deliverPrompt({
      kind: "notice",
      title: "Umbenannt",
      message: "Die Dateien heißen jetzt smart-ssh.db.unreadable-0101.",
    });
    // Der Startablauf läuft hinter dem Hinweis weiter und entsperrt.
    vi.mocked(getStartupState).mockResolvedValue(state({ screen: "unlocked" }));
    const unlockedHandler = vi.mocked(onStartupUnlocked).mock.calls.at(-1)?.[0];
    unlockedHandler?.();

    // Der neue Dateiname darf nicht mit dem Wechsel zur App verschwinden —
    // sonst erfährt ihn der Nutzer nirgends.
    await waitFor(() => expect(screen.getByText("DIE-APP-0101")).toBeTruthy());
    expect(screen.getByText(/smart-ssh.db.unreadable-0101/)).toBeTruthy();
  });

  it("zeigt eine Meldung statt der App, wenn der Startzustand nicht zu lesen war", async () => {
    vi.mocked(getStartupState).mockRejectedValue({ message: "kaputt", code: null });
    renderGate(null);

    await waitFor(() => expect(screen.getByRole("alert")).toBeTruthy());
    expect(screen.queryByText("DIE-APP-0101")).toBeNull();
    expect(screen.getByText(/Startzustand nicht lesen/)).toBeTruthy();
  });
});
