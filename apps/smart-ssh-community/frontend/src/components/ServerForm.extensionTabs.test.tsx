// Issue #608: extension tabs and section placement in the server settings.
// (Mocks and helpers mirror ServerForm.tabs.test.tsx.)
// Issue #606: the server form is split into General, Connection, Security
// and Notes tabs (Spec 0008). One draft, one Save.
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import axe from "axe-core";
import { I18nextProvider } from "react-i18next";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createServer, getServer, updateServer } from "../api";
import { testI18n } from "../testI18n";
import type { ServerDto } from "../types";
import {
  registerServerFormSection,
  registerServerSettingsTab,
  resetRegistryForTests,
  unregisterServerSettingsTab,
} from "../extensions/registry";
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

const tabNames = () => screen.getAllByRole("tab").map((t) => t.textContent?.trim());
const openTab = (name: string) => fireEvent.click(screen.getByRole("tab", { name }));

function renderForm(serverId: string | null = SERVER_ID, extra: Partial<Parameters<typeof ServerForm>[0]> = {}) {
  return render(
    <I18nextProvider i18n={testI18n}>
      <ServerForm
        serverId={serverId}
        defaultGroupId={null}
        allGroups={[]}
        allServers={[]}
        onSaved={vi.fn()}
        onDeleted={vi.fn()}
        {...extra}
      />
    </I18nextProvider>,
  );
}

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(getServer).mockResolvedValue(serverDto());
  vi.mocked(updateServer).mockResolvedValue(undefined);
  vi.mocked(createServer).mockResolvedValue(SERVER_ID);
});


const OTHER_ID = "22222222-2222-4222-8222-222222222222";
const Ext = ({ serverId }: { serverId: string }) => <div data-testid="ext-tab">ext for {serverId}</div>;
const Sec = () => <p>sec-body</p>;

afterEach(() => resetRegistryForTests());

describe("ServerForm extension tabs (issue #608)", () => {
  it("shows no extra tab or markup without registrations", async () => {
    const { container } = renderForm();
    await waitFor(() => expect(screen.getByDisplayValue("web-01")).toBeInTheDocument());
    expect(tabNames()).toEqual(["Allgemein", "Verbindung", "Sicherheit", "Notizen"]);
    expect(container.querySelector("[data-extension-section]")).toBeNull();
    expect(container.querySelectorAll('[role="tabpanel"]')).toHaveLength(4);
    // Siblings of the form: only the General wrapper (hidden unless active)
    // and the Notes panel; no empty wrappers for Connection/Security.
    const form = container.querySelector("form") as HTMLElement;
    const after = Array.from(form.parentElement!.children).slice(
      Array.from(form.parentElement!.children).indexOf(form) + 1,
    );
    expect(after.map((el) => el.getAttribute("role") ?? "div")).toEqual(["div", "tabpanel"]);
  });

  it("keeps Connection sections inside a labelled region and is keyboard/axe clean with an extension tab", async () => {
    registerServerSettingsTab({ id: "a", label: "Ten", component: Ext });
    registerServerFormSection({ id: "s-conn", tab: "connection", component: Sec });
    const { container } = renderForm();
    await waitFor(() => expect(screen.getByDisplayValue("web-01")).toBeInTheDocument());
    const region = screen.getByText("sec-body").closest('section') as HTMLElement;
    const connTab = screen.getByRole("tab", { name: "Verbindung" });
    expect(region.getAttribute("aria-labelledby")).toBe(connTab.id);
    // Keyboard: End reaches the last (extension) tab.
    screen.getByRole("tab", { selected: true }).focus();
    fireEvent.keyDown(document.activeElement as HTMLElement, { key: "End" });
    expect(screen.getByRole("tab", { name: "Ten", selected: true })).toHaveFocus();
    expect(screen.getByTestId("ext-tab")).toBeVisible();
    const results = await axe.run(container, { rules: { "color-contrast": { enabled: false } } });
    expect(results.violations.filter((v) => v.impact === "serious" || v.impact === "critical")).toEqual([]);
  });

  it("lists extension tabs after Notes sorted by order, lazily mounted, with the server id", async () => {
    registerServerSettingsTab({ id: "b", label: "Twenty", order: 20, component: Ext });
    registerServerSettingsTab({ id: "a", label: "Ten", order: 10, component: Ext });
    renderForm();
    await waitFor(() => expect(screen.getByDisplayValue("web-01")).toBeInTheDocument());
    expect(tabNames()).toEqual(["Allgemein", "Verbindung", "Sicherheit", "Notizen", "Ten", "Twenty"]);
    expect(screen.queryByTestId("ext-tab")).toBeNull();
    openTab("Ten");
    expect(screen.getByTestId("ext-tab")).toHaveTextContent(`ext for ${SERVER_ID}`);
    openTab("Allgemein");
    expect(screen.getByTestId("ext-tab")).not.toBeVisible();
  });

  it("re-renders for a different server", async () => {
    registerServerSettingsTab({ id: "a", label: "Ten", component: Ext });
    vi.mocked(getServer).mockImplementation((id: string) =>
      Promise.resolve(serverDto({ id, name: id === OTHER_ID ? "web-02" : "web-01" })),
    );
    const view = renderForm();
    await waitFor(() => expect(screen.getByDisplayValue("web-01")).toBeInTheDocument());
    openTab("Ten");
    view.rerender(
      <I18nextProvider i18n={testI18n}>
        <ServerForm serverId={OTHER_ID} defaultGroupId={null} allGroups={[]} allServers={[]} onSaved={vi.fn()} onDeleted={vi.fn()} />
      </I18nextProvider>,
    );
    await waitFor(() => expect(screen.getByDisplayValue("web-02")).toBeInTheDocument());
    openTab("Ten");
    expect(screen.getByTestId("ext-tab")).toHaveTextContent(`ext for ${OTHER_ID}`);
  });

  it("adds a late tab without losing the draft and falls back to General on unregister", async () => {
    renderForm();
    await waitFor(() => expect(screen.getByDisplayValue("web-01")).toBeInTheDocument());
    fireEvent.change(screen.getByDisplayValue("web-01"), { target: { value: "draft" } });
    act(() => registerServerSettingsTab({ id: "a", label: "Late", component: Ext }));
    expect(tabNames()).toContain("Late");
    expect(screen.getByDisplayValue("draft")).toBeInTheDocument();
    openTab("Late");
    expect(screen.getByRole("tab", { name: "Late" })).toHaveAttribute("aria-selected", "true");
    act(() => unregisterServerSettingsTab("a"));
    expect(tabNames()).not.toContain("Late");
    expect(screen.getByRole("tab", { name: "Allgemein" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByDisplayValue("draft")).toBeVisible();
  });

  it("is shown for the local pseudo-server and not for a new server", async () => {
    registerServerSettingsTab({ id: "a", label: "Ten", component: Ext });
    vi.mocked(getServer).mockResolvedValue(serverDto({ isLocal: true, name: "Local" }));
    const view = renderForm();
    await waitFor(() => expect(screen.getByText("Local")).toBeInTheDocument());
    expect(tabNames()).toEqual(["Allgemein", "Notizen", "Ten"]);
    view.unmount();
    renderForm(null);
    expect(tabNames()).not.toContain("Ten");
  });

  it("renders sections in the chosen tab, defaulting to General", async () => {
    registerServerFormSection({ id: "s-default", component: Sec });
    registerServerFormSection({ id: "s-conn", tab: "connection", component: Sec });
    renderForm();
    await waitFor(() => expect(screen.getByDisplayValue("web-01")).toBeInTheDocument());
    const wrapper = (id: string) => document.querySelector(`[data-extension-section="${id}"]`) as HTMLElement;
    expect(wrapper("s-default").parentElement).toBeVisible();
    expect(wrapper("s-conn").parentElement).not.toBeVisible();
    openTab("Verbindung");
    expect(wrapper("s-conn").parentElement).toBeVisible();
    expect(wrapper("s-default").parentElement).not.toBeVisible();
  });
});
