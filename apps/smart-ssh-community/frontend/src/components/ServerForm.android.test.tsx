// Issue #679: on Android a new server cannot use "Key file" (a content URI
// is no path to keep); keys come in via "Private key".
import { fireEvent, render, screen } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { testI18n } from "../testI18n";
import { ServerForm } from "./ServerForm";

vi.mock("../api", () => ({
  commandErrorMessage: (err: unknown) => String(err),
  commandErrorCode: () => null,
  clearServerSudoPassword: vi.fn(),
  convertIdentityFileToKeychain: vi.fn(),
  createServer: vi.fn(),
  deleteServer: vi.fn(),
  getServer: vi.fn(),
  inspectKeyFile: vi.fn(),
  listStoredHostKeys: vi.fn(() => Promise.resolve([])),
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


describe("ServerForm on Android (issue #679)", () => {
  beforeEach(() => {
    vi.spyOn(navigator, "userAgent", "get").mockReturnValue(
      "Mozilla/5.0 (Linux; Android 14) AppleWebKit/537.36 Chrome/120 Mobile Safari/537.36",
    );
  });

  function renderNew() {
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

  it("does not offer 'Key file' for a new server but keeps 'Private key'", async () => {
    renderNew();
    fireEvent.click(await screen.findByRole("tab", { name: "Verbindung" }));
    await screen.findByRole("option", { name: "Private Key" });
    expect(screen.queryByRole("option", { name: "Schlüsseldatei" })).not.toBeInTheDocument();
  });

  it("offers 'Key file' on desktop", async () => {
    vi.spyOn(navigator, "userAgent", "get").mockReturnValue("Mozilla/5.0 (X11; Linux x86_64)");
    renderNew();
    fireEvent.click(await screen.findByRole("tab", { name: "Verbindung" }));
    expect(await screen.findByRole("option", { name: "Schlüsseldatei" })).toBeInTheDocument();
  });
});
