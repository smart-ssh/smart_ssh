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
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { registerSettingsSection, resetRegistryForTests } from "../extensions/registry";
import { testI18n } from "../testI18n";
import { ChatRetentionSettings } from "./ChatRetentionSettings";
import { McpServerSettings } from "./McpServerSettings";
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
  // Spec 0055, Teil 3: die Regressionstests unten klicken tatsächlich in
  // "Sitzungen & Daten"/"MCP-Server" hinein (anders als die Tests oben, die
  // nur die Nav-Einträge prüfen) — beide Sektionen laden beim Mounten
  // echte Daten.
  getChatSessionRetentionDays: vi.fn(() => Promise.resolve(null)),
  setChatSessionRetentionDays: vi.fn(),
  getMcpServerSettings: vi.fn(() =>
    Promise.resolve({
      enabled: false,
      endpoint: "http://127.0.0.1:0",
      token: "",
      confirmTimeoutSecs: 120,
      allowedServerIds: [],
    }),
  ),
  listServers: vi.fn(() => Promise.resolve([])),
  regenerateMcpServerToken: vi.fn(),
  setMcpServerAllowedServers: vi.fn(),
  setMcpServerConfirmTimeoutSecs: vi.fn(),
  setMcpServerEnabled: vi.fn(),
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

describe("SettingsScreen section headings (Spec 0055, Teil 3)", () => {
  beforeEach(() => {
    // `registerBuiltinExtensions.ts`s `registerSettingsSection`-Aufrufe für
    // "chat-retention"/"mcp-server" laufen nur EINMAL als Modul-Nebeneffekt
    // beim allerersten Import — `resetRegistryForTests()` (hier UND in
    // jedem vorherigen Test dieser Datei) löscht sie unwiderruflich, ein
    // erneutes Auswerten des ES-Moduls gibt es nicht. Explizit erneut
    // registrieren statt uns auf einen einmaligen Seiteneffekt zu
    // verlassen, dessen Zeitpunkt von der Testreihenfolge abhinge.
    resetRegistryForTests();
    registerSettingsSection({
      id: "chat-retention",
      label: "Sitzungen & Daten",
      component: ChatRetentionSettings,
    });
    registerSettingsSection({ id: "mcp-server", label: "MCP-Server", component: McpServerSettings });
  });

  afterEach(() => {
    resetRegistryForTests();
  });

  /** Nur `SettingsScreen` selbst darf einen Titel rendern — der
   * `<h2>`-Navigationstitel ("Einstellungen") und GENAU EIN `<h3>` für die
   * aktive Kategorie (`{active?.label}`). Zeigt eine einzelne
   * Sektions-Komponente zusätzlich ihre eigene Überschrift, wächst diese
   * Zahl — das war genau der 0050-Review-Fund, den dieser Test
   * regressionssichert. */
  const expectExactlyOneSectionHeading = async (categoryLabel: string) => {
    renderSettingsScreen();
    fireEvent.click(await screen.findByText(categoryLabel));
    await waitFor(() => expect(screen.getAllByText(categoryLabel).length).toBeGreaterThan(0));
    const headings = screen.getAllByRole("heading");
    expect(headings).toHaveLength(2); // "Einstellungen" (h2) + Kategorie-Titel (h3)
  };

  it("'Anzeige & Sprache' (LanguageSettings) shows the category title exactly once", async () => {
    await expectExactlyOneSectionHeading("Anzeige & Sprache");
  });

  it("'Sitzungen & Daten' (registered ChatRetentionSettings) shows the category title exactly once", async () => {
    // `registerBuiltinExtensions.ts` registriert dies als Modul-Nebeneffekt
    // beim Import von `AiProviderSettings.tsx` — hier über denselben Import
    // wie `SettingsScreen.tsx` selbst bereits sichergestellt.
    await expectExactlyOneSectionHeading("Sitzungen & Daten");
  });

  it("'MCP-Server' (registered McpServerSettings) shows the category title exactly once", async () => {
    await expectExactlyOneSectionHeading("MCP-Server");
  });
});

describe("SettingsScreen registered section labels follow the UI language (Spec 0055, Teil 4)", () => {
  beforeEach(() => {
    resetRegistryForTests();
    registerSettingsSection({
      id: "mcp-server",
      label: "settings.categories.mcpServer",
      component: McpServerSettings,
    });
  });

  afterEach(async () => {
    resetRegistryForTests();
    await testI18n.changeLanguage("de");
  });

  it("shows the English nav label when the UI language is English", async () => {
    await testI18n.changeLanguage("en");
    renderSettingsScreen();

    expect(await screen.findByText("MCP Server")).toBeInTheDocument();
    expect(screen.queryByText("MCP-Server")).not.toBeInTheDocument();
  });
});
