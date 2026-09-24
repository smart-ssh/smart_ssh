// Spec 0069, Teil C2 (BL-0110), Test 29: der Ed25519-Hinweis im
// Server-Formular ist nur bei Anmeldeart "Private Key" sichtbar, bei jeder
// anderen Anmeldeart nicht.
//
// Spec 0071: Deckt die zwei Stellen ab, an denen diese Spec das
// Server-Formular ehrlicher macht — mehr nicht. Das Formular hatte bis
// hierher gar keine Testdatei; eine vollständige Abdeckung ist nicht Teil
// dieser Spec (s. ADR, "Bewusst nicht behoben").
//
// - **A14/I4**: „unbekannt" ist nicht „nein" — konnte der Schlüsselbund
//   nicht sagen, ob ein Sudo-Passwort hinterlegt ist, darf die Oberfläche
//   weder „hinterlegt" noch „nicht hinterlegt" behaupten.
// - **A17**: Das Löschen läuft durch, auch wenn ein Secret im
//   Schlüsselbund bleibt — der Nutzer erfährt aber davon, statt ein
//   stilles „erledigt" zu sehen.
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  clearServerSudoPassword,
  convertIdentityFileToKeychain,
  deleteServer,
  getServer,
  inspectKeyFile,
} from "../api";
import { pickFilePath } from "../fileDialog";
import { testI18n } from "../testI18n";
import type { KeyFileFactsDto, ServerDto } from "../types";
import { ServerForm } from "./ServerForm";

vi.mock("../api", () => ({
  // Spec 0069, Teil B (echte `code`-Extraktion für `runOllamaProbe`,
  // s. `ServerList.test.tsx`) und Spec 0071 (echte `.message`-Extraktion
  // für `KEYCHAIN_UNAVAILABLE`-artige Fehlerobjekte) — echte
  // Implementierungen statt einer vereinfachten Attrappe.
  commandErrorMessage: (err: unknown) =>
    typeof err === "object" && err !== null && "message" in err
      ? String((err as { message: unknown }).message)
      : String(err),
  commandErrorCode: (err: unknown) =>
    typeof err === "object" && err !== null && "code" in err
      ? ((err as { code: string | null }).code ?? null)
      : null,
  clearServerSudoPassword: vi.fn(),
  convertIdentityFileToKeychain: vi.fn(),
  createServer: vi.fn(),
  deleteServer: vi.fn(),
  getServer: vi.fn(),
  inspectKeyFile: vi.fn(),
  largeNoteDialogThresholdChars: vi.fn(() => Promise.resolve(100000)),
  previewEffectiveNotes: vi.fn(() => Promise.resolve("")),
  requestNoteShrink: vi.fn(),
  testConnection: vi.fn(),
  trustHostKey: vi.fn(),
  updateLocalServerNotes: vi.fn(),
  updateLocalServerTags: vi.fn(),
  updateServer: vi.fn(),
}));

vi.mock("../events", () => ({
  onNoteShrinkSucceeded: vi.fn(() => Promise.resolve(() => {})),
}));

vi.mock("../fileDialog", () => ({
  pickAndReadTextFile: vi.fn(() => Promise.resolve(null)),
  pickFilePath: vi.fn(() => Promise.resolve(null)),
}));

vi.mock("../riskSettings", () => ({
  loadRiskClassifierSettings: vi.fn(() => Promise.resolve({ enabled: false, providerId: null })),
}));

const SERVER_ID = "11111111-1111-4111-8111-111111111111";

function serverDto(overrides: Partial<ServerDto> = {}): ServerDto {
  return {
    id: SERVER_ID,
    name: "web-01",
    host: "example.invalid",
    port: 22,
    username: "deploy",
    groupId: null,
    tags: [],
    authKind: "agent",
    identityFilePath: null,
    jumpHost: null,
    notes: "",
    hasSudoPassword: false,
    sudoPasswordUnknown: false,
    isLocal: false,
    postIngestPolicy: "balanced",
    aiInjectionCheckEnabled: false,
    sftpServerPath: null,
    ...overrides,
  };
}

const onDeleted = vi.fn();

function renderNewServerForm() {
  return render(
    <I18nextProvider i18n={testI18n}>
      <ServerForm
        serverId={null}
        defaultGroupId={null}
        allGroups={[]}
        allServers={[]}
        onSaved={vi.fn()}
        onDeleted={vi.fn()}
      />
    </I18nextProvider>,
  );
}

function renderForm() {
  return render(
    <I18nextProvider i18n={testI18n}>
      <ServerForm
        serverId={SERVER_ID}
        defaultGroupId={null}
        allGroups={[]}
        allServers={[]}
        onSaved={vi.fn()}
        onDeleted={onDeleted}
      />
    </I18nextProvider>,
  );
}

function authKindSelect(): HTMLSelectElement {
  const select = screen
    .getAllByRole("combobox")
    .find((el) => within(el).queryByText("Private Key") !== null);
  if (!select) throw new Error("Anmeldeart-Auswahl nicht gefunden");
  return select as HTMLSelectElement;
}

const ED25519_HINT =
  "Empfohlen: Ed25519-Schlüssel. Neu erzeugen mit `ssh-keygen -t ed25519`. RSA-Schlüssel funktionieren weiterhin.";

beforeEach(() => {
  // Vorsorge gegen Reihenfolgeabhaengigkeit: Jeder Test setzt seine Mocks
  // selbst, nichts traegt aus dem vorigen herueber (spec-reviewer-Fund zur
  // Mock-Hygiene in `DiagnosticsSettings.test.tsx`). Gilt file-weit (auch
  // für die C2-Tests oben) — deren `serverId={null}`-Formular ruft
  // `getServer` ohnehin nie auf (s. `ServerForm.tsx`s `loadServer`).
  vi.clearAllMocks();
  vi.mocked(getServer).mockResolvedValue(serverDto());
});

describe("ServerForm Ed25519 recommendation (Spec 0069, Teil C2)", () => {
  it("is hidden for the default auth kind (Passwort)", () => {
    renderNewServerForm();

    expect(screen.queryByText(ED25519_HINT)).not.toBeInTheDocument();
  });

  it("appears when auth kind is switched to Private Key", () => {
    renderNewServerForm();

    fireEvent.change(authKindSelect(), { target: { value: "privateKey" } });

    expect(screen.getByText(ED25519_HINT)).toBeInTheDocument();
  });

  it("disappears again when switching away from Private Key", () => {
    renderNewServerForm();

    fireEvent.change(authKindSelect(), { target: { value: "privateKey" } });
    expect(screen.getByText(ED25519_HINT)).toBeInTheDocument();

    fireEvent.change(authKindSelect(), { target: { value: "agent" } });
    expect(screen.queryByText(ED25519_HINT)).not.toBeInTheDocument();
  });

  it("is hidden for certificate auth", () => {
    renderNewServerForm();

    fireEvent.change(authKindSelect(), { target: { value: "certificate" } });

    expect(screen.queryByText(ED25519_HINT)).not.toBeInTheDocument();
  });
});

describe("ServerForm — Sudo-Passwort-Zustand (Spec 0071, A14)", () => {
  it("sagt bei nicht lesbarem Schlüsselbund weder 'hinterlegt' noch 'nicht hinterlegt'", async () => {
    vi.mocked(getServer).mockResolvedValue(
      serverDto({ hasSudoPassword: false, sudoPasswordUnknown: true }),
    );

    renderForm();

    expect(
      await screen.findByText(/lässt sich ohne Systemschlüsselbund nicht feststellen/),
    ).toBeInTheDocument();
    // Der Ja-Text darf nicht zusätzlich erscheinen; der Nein-Text ist die
    // eigentliche Falschaussage aus X4 und muss weg sein.
    expect(screen.queryByText("(leer = unverändert, aktuell hinterlegt)")).not.toBeInTheDocument();
    expect(screen.queryByText("(leer = unverändert)")).not.toBeInTheDocument();
  });

  it("sagt 'nicht hinterlegt' nur, wenn der Schlüsselbund das auch beantworten konnte", async () => {
    vi.mocked(getServer).mockResolvedValue(
      serverDto({ hasSudoPassword: false, sudoPasswordUnknown: false }),
    );

    renderForm();

    expect(await screen.findByText("(leer = unverändert)")).toBeInTheDocument();
    expect(
      screen.queryByText(/lässt sich ohne Systemschlüsselbund nicht feststellen/),
    ).not.toBeInTheDocument();
  });
});

describe("ServerForm — Löschen mit Rückständen (Spec 0071, A17)", () => {
  it("meldet die im Schlüsselbund verbliebenen Secrets, statt still zu schließen", async () => {
    const leftover = `server:${SERVER_ID}:password`;
    vi.mocked(deleteServer).mockImplementation((_id: string, confirm: boolean) =>
      Promise.resolve({
        server: serverDto({ authKind: "password" }),
        serversLosingJumpHost: [],
        executed: confirm,
        secretsLeftBehind: confirm ? [leftover] : [],
      }),
    );

    renderForm();
    fireEvent.click(await screen.findByText("Server löschen"));
    fireEvent.click(await screen.findByText("Endgültig löschen"));

    const notice = await screen.findByTestId("secrets-left-behind");
    expect(notice).toHaveTextContent("Server gelöscht");
    expect(notice).toHaveTextContent(leftover);
    expect(notice).toHaveTextContent("verwaist");
    // Erst nach dem Wegklicken schließt die Maske — sonst hätte der Nutzer
    // den Hinweis nie gesehen.
    expect(onDeleted).not.toHaveBeenCalled();
    fireEvent.click(screen.getByText("Schließen"));
    await waitFor(() => expect(onDeleted).toHaveBeenCalled());
  });

  it("schließt ohne Rückstände unverändert sofort", async () => {
    vi.mocked(deleteServer).mockImplementation((_id: string, confirm: boolean) =>
      Promise.resolve({
        server: serverDto({ authKind: "password" }),
        serversLosingJumpHost: [],
        executed: confirm,
        secretsLeftBehind: [],
      }),
    );

    renderForm();
    fireEvent.click(await screen.findByText("Server löschen"));
    fireEvent.click(await screen.findByText("Endgültig löschen"));

    await waitFor(() => expect(onDeleted).toHaveBeenCalled());
    expect(screen.queryByTestId("secrets-left-behind")).not.toBeInTheDocument();
  });
});

describe("ServerForm — Sudo-Passwort entfernen schlägt fehl (Spec 0071, A17)", () => {
  it("sagt ausdrücklich, dass das Passwort weiter wirksam bleibt", async () => {
    vi.mocked(getServer).mockResolvedValue(serverDto({ hasSudoPassword: true }));
    vi.mocked(clearServerSudoPassword).mockRejectedValue({
      code: "KEYCHAIN_UNAVAILABLE",
      message: "Der Systemschlüsselbund ist nicht verfügbar.",
    });

    renderForm();
    fireEvent.click(await screen.findByText("Hinterlegtes Sudo-Passwort entfernen"));

    // Der generische Code-Text allein sagt nur "speichern oder lesen" —
    // dass das Passwort beim naechsten `sudo` wieder eingespeist wird, ist
    // die eigentliche Information auf diesem Pfad.
    const error = await screen.findByText(/konnte nicht entfernt werden/);
    expect(error).toHaveTextContent("weiterhin im Systemschlüsselbund");
    expect(error).toHaveTextContent("sudo");
    // Und die Maske darf nicht auf "kein Sudo-Passwort hinterlegt"
    // umschalten, obwohl nichts entfernt wurde.
    expect(screen.getByText("(leer = unverändert, aktuell hinterlegt)")).toBeInTheDocument();
  });
});

function validKeyFacts(overrides: Partial<KeyFileFactsDto> = {}): KeyFileFactsDto {
  return {
    exists: true,
    permissionsTooOpen: false,
    validKey: true,
    encrypted: false,
    problem: null,
    ...overrides,
  };
}

const IDENTITY_PATH = "/home/deploy/.ssh/id_ed25519";

describe("ServerForm — Anmeldeart Schlüsseldatei (Spec 0076, B-1..B-4)", () => {
  it("ist im Dropdown wählbar und zeigt danach Pfadfeld + Dateidialog-Knopf (B-1, B-2)", () => {
    renderNewServerForm();

    fireEvent.change(authKindSelect(), { target: { value: "identityFile" } });

    expect(screen.getByPlaceholderText("~/.ssh/id_ed25519")).toBeInTheDocument();
    expect(screen.getByText("Datei wählen…")).toBeInTheDocument();
  });

  it("übernimmt den vom Dateidialog gewählten Pfad ins Textfeld (B-2)", async () => {
    vi.mocked(pickFilePath).mockResolvedValue(IDENTITY_PATH);
    renderNewServerForm();
    fireEvent.change(authKindSelect(), { target: { value: "identityFile" } });

    fireEvent.click(screen.getByText("Datei wählen…"));

    expect(await screen.findByDisplayValue(IDENTITY_PATH)).toBeInTheDocument();
  });

  // Regressionstest mit Gegenbeweis: mit der Bedingung `auth.kind !==
  // "identityFile"` durch `true` ersetzt (Effekt ruft `inspectKeyFile` nie
  // auf) schlägt dieser Test fehl — verifiziert, danach wiederhergestellt.
  it("fragt nach einer Pause den Vorab-Befund ab und zeigt die übersetzte Meldung (B-3)", async () => {
    vi.mocked(inspectKeyFile).mockResolvedValue({
      exists: false,
      permissionsTooOpen: false,
      validKey: false,
      encrypted: false,
      problem: { code: "KEY_FILE_NOT_FOUND", message: "Datei nicht gefunden: /tmp/nope" },
    });
    renderNewServerForm();
    fireEvent.change(authKindSelect(), { target: { value: "identityFile" } });
    fireEvent.change(screen.getByPlaceholderText("~/.ssh/id_ed25519"), {
      target: { value: "/tmp/nope" },
    });

    await waitFor(() => expect(inspectKeyFile).toHaveBeenCalledWith("/tmp/nope"), {
      timeout: 2000,
    });
    // Der übersetzte, generische Text — NICHT der deutsche Backend-Fallback
    // mit dem konkreten Pfad — belegt, dass der Code (nicht nur die
    // Nachricht) durchgereicht wird (Spec 0024, Abschnitt 5).
    expect(
      await screen.findByText(
        "Die Schlüsseldatei wurde unter dem angegebenen Pfad nicht gefunden.",
      ),
    ).toBeInTheDocument();
  });

  // spec-reviewer-Fund (Review dieses Schritts): ein gescheiterter
  // `inspect_key_file`-Aufruf selbst verschwand bisher im `.catch(() =>
  // null)`, ununterscheidbar von "noch nicht geprüft". Regressionstest mit
  // Gegenbeweis: mit `.catch(() => { if (!cancelled) setIdentityFacts(null); })`
  // statt `setIdentityFactsError(true)` (dem Stand vor diesem Fix) zeigt
  // die Oberfläche nach dem Scheitern nichts an — dieser Test schlägt dann
  // fehl, weil der Fehlertext nie erscheint. Verifiziert, danach
  // wiederhergestellt.
  it("zeigt einen Hinweis, wenn der Vorab-Befund selbst nicht abgefragt werden konnte", async () => {
    vi.mocked(inspectKeyFile).mockRejectedValue(new Error("IPC kaputt"));
    renderNewServerForm();
    fireEvent.change(authKindSelect(), { target: { value: "identityFile" } });
    fireEvent.change(screen.getByPlaceholderText("~/.ssh/id_ed25519"), {
      target: { value: IDENTITY_PATH },
    });

    expect(
      await screen.findByText(
        "Der Vorab-Befund konnte nicht abgefragt werden. Das hindert das Speichern nicht — beim Verbinden wird die Datei ohnehin neu geprüft.",
        {},
        { timeout: 2000 },
      ),
    ).toBeInTheDocument();
  });

  // §6.3.2, restliche Fälle: zu weite Rechte und "verschlüsselt" werden
  // gemeldet, UND das Speichern bleibt möglich (B-3, letzter Satz — der
  // Submit-Knopf hängt an `saving`, nicht an den Vorab-Befund-Fakten).
  it("meldet zu weite Rechte und Verschlüsselung, hindert das Speichern aber nicht (§6.3.2)", async () => {
    vi.mocked(inspectKeyFile).mockResolvedValue({
      exists: true,
      permissionsTooOpen: true,
      validKey: true,
      encrypted: true,
      problem: null,
    });
    renderNewServerForm();
    fireEvent.change(authKindSelect(), { target: { value: "identityFile" } });
    fireEvent.change(screen.getByPlaceholderText("~/.ssh/id_ed25519"), {
      target: { value: IDENTITY_PATH },
    });

    await waitFor(() => expect(inspectKeyFile).toHaveBeenCalledWith(IDENTITY_PATH), {
      timeout: 2000,
    });

    expect(
      await screen.findByText(
        "Die Dateirechte sind zu offen (für Gruppe oder Welt lesbar oder beschreibbar). Beim Verbinden wird die Anmeldung deshalb abgelehnt — z. B. mit „chmod 600“ auf die Datei beheben.",
      ),
    ).toBeInTheDocument();
    expect(
      screen.getByText(
        "Der Schlüssel ist verschlüsselt — beim Verbinden ist die hinterlegte Passphrase nötig.",
      ),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Anlegen" })).not.toBeDisabled();
  });

  // spec-reviewer-Fund (Review Runde 2): Die erste Fassung des B-3-Fixes
  // behauptete "Rechte eng genug gesetzt" auch dann, wenn die Rechte gar
  // nie gemessen wurden (z. B. TOO_LARGE via `fstat`, bevor überhaupt
  // gelesen wird — vgl. `key_files.rs::probe`). Regressionstest mit
  // Gegenbeweis: mit `permissionsAreKnown = true` fest verdrahtet (statt
  // der Prüfung auf `KEY_FILE_INVALID_KEY`) erscheint "Rechte eng genug
  // gesetzt" trotz `KEY_FILE_TOO_LARGE" — dieser Test schlägt dann fehl.
  // Verifiziert, danach wiederhergestellt.
  it("behauptet bei KEY_FILE_TOO_LARGE keine nie gemessenen Rechte (spec-reviewer-Fund, Runde 2)", async () => {
    vi.mocked(inspectKeyFile).mockResolvedValue({
      exists: true,
      permissionsTooOpen: false,
      validKey: false,
      encrypted: false,
      problem: { code: "KEY_FILE_TOO_LARGE", message: "zu groß" },
    });
    renderNewServerForm();
    fireEvent.change(authKindSelect(), { target: { value: "identityFile" } });
    fireEvent.change(screen.getByPlaceholderText("~/.ssh/id_ed25519"), {
      target: { value: IDENTITY_PATH },
    });

    await waitFor(() => expect(inspectKeyFile).toHaveBeenCalledWith(IDENTITY_PATH), {
      timeout: 2000,
    });
    await screen.findByText("Die Schlüsseldatei überschreitet die zulässige Größe von 1 MiB.");

    expect(screen.queryByText("Die Dateirechte sind eng genug gesetzt.")).not.toBeInTheDocument();
    expect(
      screen.queryByText(
        "Die Dateirechte sind zu offen (für Gruppe oder Welt lesbar oder beschreibbar). Beim Verbinden wird die Anmeldung deshalb abgelehnt — z. B. mit „chmod 600“ auf die Datei beheben.",
      ),
    ).not.toBeInTheDocument();
  });

  // Gegenstück: Bei `KEY_FILE_INVALID_KEY` SIND die Rechte verlässlich
  // ermittelt (der einzige Fehlercode, der erst nach einer erfolgreichen
  // Rechteprüfung entstehen kann) — B-3 verlangt diese Zeile hier
  // ausdrücklich.
  it("zeigt Rechte-Warnung bei KEY_FILE_INVALID_KEY, weil sie dort verlässlich ermittelt ist", async () => {
    vi.mocked(inspectKeyFile).mockResolvedValue({
      exists: true,
      permissionsTooOpen: true,
      validKey: false,
      encrypted: false,
      problem: { code: "KEY_FILE_INVALID_KEY", message: "kein gültiger Schlüssel" },
    });
    renderNewServerForm();
    fireEvent.change(authKindSelect(), { target: { value: "identityFile" } });
    fireEvent.change(screen.getByPlaceholderText("~/.ssh/id_ed25519"), {
      target: { value: IDENTITY_PATH },
    });

    expect(
      await screen.findByText(
        "Die Dateirechte sind zu offen (für Gruppe oder Welt lesbar oder beschreibbar). Beim Verbinden wird die Anmeldung deshalb abgelehnt — z. B. mit „chmod 600“ auf die Datei beheben.",
      ),
    ).toBeInTheDocument();
  });

  // Regressionstest mit Gegenbeweis: den sofortigen `setIdentityFacts(null)`
  // am Effektanfang entfernt (dem Stand vor diesem Fix), bleibt der Befund
  // des VORHERIGEN Pfads bis zum Ablauf der 400-ms-Verzögerung sichtbar —
  // dieser Test schlägt dann fehl, weil "gültiger Schlüssel" weiterhin für
  // den neuen, noch ungeprüften Pfad angezeigt würde. Verifiziert, danach
  // wiederhergestellt.
  it("zeigt beim Pfadwechsel sofort 'wird geprüft', nicht den Befund des alten Pfads (Fix 7)", async () => {
    vi.mocked(inspectKeyFile).mockResolvedValue(validKeyFacts());
    renderNewServerForm();
    fireEvent.change(authKindSelect(), { target: { value: "identityFile" } });
    const input = screen.getByPlaceholderText("~/.ssh/id_ed25519");
    fireEvent.change(input, { target: { value: IDENTITY_PATH } });

    await screen.findByText("Sieht aus wie ein gültiger OpenSSH-Schlüssel.");

    fireEvent.change(input, { target: { value: "/tmp/anderer-pfad" } });

    // Sofort nach dem Tippen (noch innerhalb der 400-ms-Verzögerung) darf
    // der alte, jetzt nicht mehr zutreffende Befund nicht mehr stehen.
    expect(screen.queryByText("Sieht aus wie ein gültiger OpenSSH-Schlüssel.")).not.toBeInTheDocument();
    expect(screen.getByText("Wird geprüft …")).toBeInTheDocument();
  });

  // Regressionstest mit Gegenbeweis: die drei `setConvertConfirmOpen(false)`/
  // `setConverting(false)`/`setConvertError(null)`-Zeilen in `loadServer`
  // entfernt (dem Stand vor diesem Fix), bleibt der Bestätigungsdialog nach
  // einem Serverwechsel auf demselben gemounteten Formular offen und würde
  // — träfe der zweite Server ebenfalls auf `identity_file` — den Pfad des
  // ERSTEN Servers weiter zeigen. Verifiziert, danach wiederhergestellt.
  it("schließt den Überführungsdialog defensiv, wenn ein anderer Server geladen wird (Fix 8)", async () => {
    const OTHER_ID = "22222222-2222-4222-8222-222222222222";
    const OTHER_PATH = "/home/other/.ssh/id_ed25519";
    vi.mocked(getServer).mockImplementation((id: string) =>
      Promise.resolve(
        id === SERVER_ID
          ? serverDto({ authKind: "identity_file", identityFilePath: IDENTITY_PATH })
          : serverDto({
              id: OTHER_ID,
              authKind: "identity_file",
              identityFilePath: OTHER_PATH,
            }),
      ),
    );
    vi.mocked(inspectKeyFile).mockResolvedValue(validKeyFacts());

    const { rerender } = render(
      <I18nextProvider i18n={testI18n}>
        <ServerForm
          serverId={SERVER_ID}
          defaultGroupId={null}
          allGroups={[]}
          allServers={[]}
          onSaved={vi.fn()}
          onDeleted={vi.fn()}
        />
      </I18nextProvider>,
    );

    const button = await screen.findByRole("button", { name: "In den Schlüsselbund übernehmen" });
    await waitFor(() => expect(button).not.toBeDisabled());
    fireEvent.click(button);
    expect(await screen.findByText(IDENTITY_PATH)).toBeInTheDocument();

    rerender(
      <I18nextProvider i18n={testI18n}>
        <ServerForm
          serverId={OTHER_ID}
          defaultGroupId={null}
          allGroups={[]}
          allServers={[]}
          onSaved={vi.fn()}
          onDeleted={vi.fn()}
        />
      </I18nextProvider>,
    );

    await waitFor(() => expect(screen.queryByText(IDENTITY_PATH)).not.toBeInTheDocument());
    expect(screen.queryByText("Übernehmen")).not.toBeInTheDocument();
  });

  // Regressionstest mit Gegenbeweis: mit `setAuth(authStateFromKind(...))`
  // statt der pfaderhaltenden Fallunterscheidung (dem Stand vor diesem
  // Schritt) bleibt das Feld leer — verifiziert, danach wiederhergestellt.
  it("befüllt beim Bearbeiten eines Schlüsseldatei-Servers den Pfad vor (B-4)", async () => {
    vi.mocked(getServer).mockResolvedValue(
      serverDto({ authKind: "identity_file", identityFilePath: IDENTITY_PATH }),
    );
    vi.mocked(inspectKeyFile).mockResolvedValue(validKeyFacts());

    renderForm();

    expect(await screen.findByDisplayValue(IDENTITY_PATH)).toBeInTheDocument();
  });
});

describe("ServerForm — Überführung in den Schlüsselbund (Spec 0076, C-1/C-2/C-7)", () => {
  it("zeigt keinen Überführen-Knopf für andere Anmeldearten", async () => {
    vi.mocked(getServer).mockResolvedValue(serverDto({ authKind: "agent" }));

    renderForm();

    await screen.findByText(/web-01/);
    expect(
      screen.queryByRole("button", { name: "In den Schlüsselbund übernehmen" }),
    ).not.toBeInTheDocument();
  });

  // Regressionstest mit Gegenbeweis: mit `disabled={false}` statt
  // `disabled={!convertFacts?.validKey}` bliebe der Knopf trotz
  // ungültigem Schlüssel klickbar — verifiziert, danach wiederhergestellt.
  it("Knopf bleibt gesperrt, solange der gespeicherte Pfad kein gültiger Schlüssel ist (C-7)", async () => {
    vi.mocked(getServer).mockResolvedValue(
      serverDto({ authKind: "identity_file", identityFilePath: IDENTITY_PATH }),
    );
    vi.mocked(inspectKeyFile).mockResolvedValue(
      validKeyFacts({
        validKey: false,
        problem: { code: "KEY_FILE_INVALID_KEY", message: "kein gültiger Schlüssel" },
      }),
    );

    renderForm();

    const button = await screen.findByRole("button", {
      name: "In den Schlüsselbund übernehmen",
    });
    await waitFor(() => expect(inspectKeyFile).toHaveBeenCalledWith(IDENTITY_PATH));
    expect(button).toBeDisabled();
  });

  // Regressionstest mit Gegenbeweis: `onClick` von `handleConvertToKeychain`
  // auf ein No-op geändert lässt `convertIdentityFileToKeychain` nie
  // aufrufen — dieser Test schlägt dann fehl. Verifiziert, danach
  // wiederhergestellt.
  it("zeigt vor der Übernahme Pfad und Folgen, ruft dann convert_identity_file_to_keychain auf (C-1/C-2)", async () => {
    vi.mocked(getServer).mockResolvedValue(
      serverDto({ authKind: "identity_file", identityFilePath: IDENTITY_PATH }),
    );
    vi.mocked(inspectKeyFile).mockResolvedValue(validKeyFacts());
    vi.mocked(convertIdentityFileToKeychain).mockResolvedValue(
      serverDto({ authKind: "private_key" }),
    );

    renderForm();
    const button = await screen.findByRole("button", {
      name: "In den Schlüsselbund übernehmen",
    });
    await waitFor(() => expect(button).not.toBeDisabled());
    fireEvent.click(button);

    // C-2: der Dialog nennt den vollen Pfad, bevor irgendetwas passiert.
    expect(await screen.findByText(IDENTITY_PATH)).toBeInTheDocument();
    expect(convertIdentityFileToKeychain).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "Übernehmen" }));

    await waitFor(() => expect(convertIdentityFileToKeychain).toHaveBeenCalledWith(SERVER_ID));
  });
});
