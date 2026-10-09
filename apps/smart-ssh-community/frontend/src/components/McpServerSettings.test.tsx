import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { getMcpServerSettings, listServers } from "../api";
import { testI18n } from "../testI18n";
import { McpServerSettings } from "./McpServerSettings";

vi.mock("../api", () => ({
  commandErrorMessage: (e: unknown) => String(e),
  getMcpServerSettings: vi.fn(),
  listServers: vi.fn(),
  regenerateMcpServerToken: vi.fn(),
  setMcpServerAllowedServers: vi.fn(),
  setMcpServerConfirmTimeoutSecs: vi.fn(),
  setMcpServerEnabled: vi.fn(),
}));

const settings = {
  enabled: true,
  endpoint: "http://127.0.0.1:4711",
  token: "tok-secret-123",
  allowedServerIds: [],
  confirmTimeoutSecs: 300,
};

function renderIt() {
  return render(
    <I18nextProvider i18n={testI18n}>
      <McpServerSettings />
    </I18nextProvider>,
  );
}

describe("McpServerSettings copy action (issue #184)", () => {
  beforeEach(() => {
    vi.mocked(getMcpServerSettings).mockResolvedValue(settings);
    vi.mocked(listServers).mockResolvedValue([]);
  });

  it("copies exactly the displayed snippet and confirms", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
    const { container } = renderIt();
    const button = await screen.findByRole("button", { name: "Konfiguration kopieren" });
    fireEvent.click(button);
    await waitFor(() => expect(writeText).toHaveBeenCalledTimes(1));
    const shown = container.querySelector("pre")?.textContent;
    expect(writeText).toHaveBeenCalledWith(shown);
    expect(shown).toContain("Bearer tok-secret-123");
    expect(await screen.findByText("Konfiguration kopiert.")).toBeInTheDocument();
  });

  it("shows an error when the clipboard is unavailable", async () => {
    const writeText = vi.fn().mockRejectedValue(new Error("denied"));
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
    renderIt();
    fireEvent.click(await screen.findByRole("button", { name: "Konfiguration kopieren" }));
    expect(await screen.findByText("Die Konfiguration konnte nicht kopiert werden.")).toBeInTheDocument();
  });

  it("has an English accessible name and no inner scroll region", async () => {
    await testI18n.changeLanguage("en");
    try {
      const { container } = renderIt();
      expect(await screen.findByRole("button", { name: "Copy configuration" })).toBeInTheDocument();
      expect(container.querySelector("pre")?.className).not.toMatch(/overflow|max-h/);
    } finally {
      await testI18n.changeLanguage("de");
    }
  });
});
