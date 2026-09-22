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
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { deleteServer, getServer } from "../api";
import { testI18n } from "../testI18n";
import type { ServerDto } from "../types";
import { ServerForm } from "./ServerForm";

vi.mock("../api", () => ({
  commandErrorMessage: (err: unknown) => String(err),
  commandErrorCode: () => null,
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

vi.mock("../riskSettings", () => ({
  loadRiskClassifierSettings: vi.fn(() => Promise.resolve({ enabled: false, providerId: null })),
}));

vi.mock("../events", () => ({
  onNoteShrinkSucceeded: vi.fn(() => Promise.resolve(() => {})),
}));

vi.mock("../fileDialog", () => ({ pickAndReadTextFile: vi.fn() }));

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

beforeEach(() => {
  onDeleted.mockClear();
  vi.mocked(getServer).mockResolvedValue(serverDto());
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
