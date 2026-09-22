// Spec 0069, Teil C2 (BL-0110), Test 29: der Ed25519-Hinweis im
// Server-Formular ist nur bei Anmeldeart "Private Key" sichtbar, bei jeder
// anderen Anmeldeart nicht.
import { fireEvent, render, screen, within } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { describe, expect, it, vi } from "vitest";
import { testI18n } from "../testI18n";
import { ServerForm } from "./ServerForm";

vi.mock("../api", () => ({
  clearServerSudoPassword: vi.fn(),
  commandErrorCode: () => null,
  commandErrorMessage: (err: unknown) => String(err),
  createServer: vi.fn(),
  deleteServer: vi.fn(),
  getServer: vi.fn(),
  largeNoteDialogThresholdBytes: vi.fn(() => Promise.resolve(50000)),
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

function authKindSelect(): HTMLSelectElement {
  const select = screen
    .getAllByRole("combobox")
    .find((el) => within(el).queryByText("Private Key") !== null);
  if (!select) throw new Error("Anmeldeart-Auswahl nicht gefunden");
  return select as HTMLSelectElement;
}

const ED25519_HINT =
  "Empfohlen: Ed25519-Schlüssel. Neu erzeugen mit `ssh-keygen -t ed25519`. RSA-Schlüssel funktionieren weiterhin.";

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
