// Spec 0050, Teil 1 ("Testbarkeit"): "eine registrierte Sektion erscheint
// als Nav-Eintrag und rendert ihren Inhalt (Komponententest mit einer
// Test-Registrierung)". Genau das ist die kritische Regressionsgefahr aus
// Abschnitt 1.2 — verschwände dieser Kontributionspunkt beim Umbau auf die
// zweispaltige Struktur, verschwände auch die private Pro-Lizenz-Sektion
// (die sich exakt hierüber registriert), ohne dass irgendein öffentlicher
// Test das je bemerken würde. Nutzt eine echte Test-Registrierung über
// `registerSettingsSection` statt einer der eingebauten Kategorien, damit
// der Test beweist, dass generisch JEDE registrierte Sektion ankommt, nicht
// nur die beiden bereits bekannten (`chat-retention`/`mcp-server`).
import { render, screen, waitFor } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { registerSettingsSection, resetRegistryForTests } from "../extensions/registry";
import { testI18n } from "../testI18n";
import { SettingsScreen } from "./SettingsScreen";

vi.mock("../api", () => ({
  listAiProviders: vi.fn(() => Promise.resolve([])),
  addAiProvider: vi.fn(),
  deleteAiProvider: vi.fn(),
  discoverModels: vi.fn(),
  fetchAttestationInfo: vi.fn(),
  setActiveAiProvider: vi.fn(),
  openLogDirectory: vi.fn(),
  commandErrorMessage: (err: unknown) => String(err),
}));

vi.mock("../riskSettings", () => ({
  loadRiskClassifierSettings: vi.fn(() => Promise.resolve({ enabled: false, providerId: null })),
  saveRiskClassifierSettings: vi.fn(),
}));

function TestSection() {
  return <p>test-section-content</p>;
}

function renderSettingsScreen() {
  return render(
    <I18nextProvider i18n={testI18n}>
      <SettingsScreen onClose={vi.fn()} onProvidersChanged={vi.fn()} />
    </I18nextProvider>,
  );
}

describe("SettingsScreen registered sections (Spec 0050, Fund 1.2)", () => {
  beforeEach(() => {
    resetRegistryForTests();
  });

  afterEach(() => {
    resetRegistryForTests();
  });

  it("shows a registered settings section as its own nav entry and renders its content on click", async () => {
    registerSettingsSection({
      id: "test-registered-section",
      label: "Test-Sektion",
      component: TestSection,
    });

    renderSettingsScreen();

    await waitFor(() => expect(screen.getByText("Test-Sektion")).toBeInTheDocument());
    expect(screen.queryByText("test-section-content")).not.toBeInTheDocument();

    screen.getByText("Test-Sektion").click();

    await waitFor(() => expect(screen.getByText("test-section-content")).toBeInTheDocument());
  });

  it("falls back to the id as the nav label when a registered section has no label", async () => {
    registerSettingsSection({ id: "unlabeled-section", component: TestSection });

    renderSettingsScreen();

    await waitFor(() => expect(screen.getByText("unlabeled-section")).toBeInTheDocument());
  });

  it("still shows the built-in categories alongside registered sections", async () => {
    registerSettingsSection({ id: "test-registered-section", label: "Test-Sektion", component: TestSection });

    renderSettingsScreen();

    await waitFor(() => expect(screen.getAllByText("KI-Provider").length).toBeGreaterThan(0));
    expect(screen.getByText("Anzeige & Sprache")).toBeInTheDocument();
    expect(screen.getByText("Diagnose")).toBeInTheDocument();
  });
});
