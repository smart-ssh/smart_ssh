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
import { clearServerSudoPassword, deleteServer, getServer } from "../api";
import { testI18n } from "../testI18n";
import type { ServerDto } from "../types";
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
  createServer: vi.fn(),
  deleteServer: vi.fn(),
  getServer: vi.fn(),
  largeNoteDialogThresholdBytes: vi.fn(() => Promise.resolve(100000)),
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
