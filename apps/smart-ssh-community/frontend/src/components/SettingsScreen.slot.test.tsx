// Issue #674: extension slot inside the "Data and backup" category.
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  registerSettingsSection,
  resetRegistryForTests,
  unregisterSettingsSection,
} from "../extensions/registry";
import { setNarrow, setWide } from "../test/matchMedia";
import { testI18n } from "../testI18n";
import { SettingsScreen } from "./SettingsScreen";

vi.mock("./BackupSettings", () => ({
  BackupSettings: () => <p>builtin-backup</p>,
}));
vi.mock("./AboutSettings", () => ({ AboutSettings: () => <p>builtin-AboutSettings</p> }));
vi.mock("./AiProviderSettings", () => ({ AiProviderSettings: () => <p>builtin-AiProviderSettings</p> }));
vi.mock("./DiagnosticsSettings", () => ({ DiagnosticsSettings: () => <p>builtin-DiagnosticsSettings</p> }));
vi.mock("./FileTypeSettings", () => ({ FileTypeSettings: () => <p>builtin-FileTypeSettings</p> }));
vi.mock("./LanguageSettings", () => ({ LanguageSettings: () => <p>builtin-LanguageSettings</p> }));
vi.mock("./MasterPasswordSettings", () => ({ MasterPasswordSettings: () => <p>builtin-MasterPasswordSettings</p> }));
vi.mock("./VoiceInputSettings", () => ({ VoiceInputSettings: () => <p>builtin-VoiceInputSettings</p> }));
vi.mock("../api/api", async () =>
  (await import("../test/apiMock")).createApiMock({
    listAiProviders: vi.fn(() => Promise.resolve([])),
    discoverModels: vi.fn(),
  }),
);

const mk = (text: string) => () => <p>{text}</p>;

function renderScreen() {
  return render(
    <I18nextProvider i18n={testI18n}>
      <SettingsScreen onClose={vi.fn()} onProvidersChanged={vi.fn()} />
    </I18nextProvider>,
  );
}

async function openDataBackup(narrow: boolean) {
  renderScreen();
  const nav = (await screen.findAllByText("Daten und Backup")).at(0)!;
  fireEvent.click(nav);
  if (narrow) await screen.findByText("builtin-backup");
}

function order(texts: string[]) {
  const all = Array.from(document.querySelectorAll("p")).map((p) => p.textContent);
  return texts.map((t) => all.indexOf(t));
}

describe("SettingsScreen data-backup slot (#674)", () => {
  beforeEach(() => resetRegistryForTests());
  afterEach(() => {
    resetRegistryForTests();
    setWide();
  });

  for (const narrow of [false, true]) {
    it(`renders categorized sections after the built-in one, by order (${narrow ? "narrow" : "wide"})`, async () => {
      if (narrow) setNarrow();
      else setWide();
      registerSettingsSection({ id: "late", category: "data-backup", order: 20, component: mk("sec-late") });
      registerSettingsSection({ id: "early", category: "data-backup", order: 10, component: mk("sec-early") });
      await openDataBackup(narrow);
      await screen.findByText("sec-early");
      const [b, e, l] = order(["builtin-backup", "sec-early", "sec-late"]);
      expect(b).toBeGreaterThanOrEqual(0);
      expect(b).toBeLessThan(e);
      expect(e).toBeLessThan(l);
      // Not a top-level nav entry.
      expect(screen.queryByText("early")).not.toBeInTheDocument();
    });
  }

  it("shows a late registration and drops an unregistered section", async () => {
    await openDataBackup(false);
    await screen.findByText("builtin-backup");
    act(() => registerSettingsSection({ id: "x", category: "data-backup", component: mk("sec-x") }));
    await screen.findByText("sec-x");
    act(() => unregisterSettingsSection("x"));
    await waitFor(() => expect(screen.queryByText("sec-x")).not.toBeInTheDocument());
  });

  it("does not render a categorized section in other categories", async () => {
    registerSettingsSection({ id: "x", category: "data-backup", component: mk("sec-x") });
    renderScreen();
    await screen.findAllByText("KI-Provider");
    expect(screen.queryByText("sec-x")).not.toBeInTheDocument();
  });
});

describe("SettingsScreen category slots for every built-in category (#1311)", () => {
  beforeEach(() => resetRegistryForTests());
  afterEach(() => resetRegistryForTests());

  const cats: [string, string][] = [
    ["ai-provider", "KI-Provider"],
    ["display-language", "Anzeigesprache"],
    ["diagnostics", "Diagnose"],
    ["file-types", "Dateien"],
    ["master-password", "Master-Passwort"],
    ["data-backup", "Daten und Backup"],
    ["voice-input", "Spracheingabe"],
    ["about", "Über"],
  ];

  for (const [id] of cats) {
    it(`renders sections of ${id} in order, only inside that category`, async () => {
      registerSettingsSection({ id: "b", category: id, order: 5, component: mk("sec-b") });
      registerSettingsSection({ id: "a", category: id, order: 5, component: mk("sec-a") });
      registerSettingsSection({ id: "c", category: id, order: 1, component: mk("sec-c") });
      renderScreen();
      const navs = await screen.findAllByRole("button", { name: /./ });
      const idx = cats.findIndex(([c]) => c === id);
      const nav = navs.filter((b) => b.closest("li"))[idx];
      fireEvent.click(nav);
      await screen.findByText("sec-c");
      const [c, b, a] = order(["sec-c", "sec-b", "sec-a"]);
      expect(c).toBeLessThan(b);
      expect(b).toBeLessThan(a);
      // Not in another category.
      const other = navs.filter((x) => x.closest("li"))[(idx + 1) % cats.length];
      fireEvent.click(other);
      await waitFor(() => expect(screen.queryByText("sec-c")).not.toBeInTheDocument());
    });
  }

  it("shows a section with an unknown category as top-level entry and warns", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    registerSettingsSection({ id: "odd", label: "Odd entry", category: "nope", component: mk("sec-odd") });
    renderScreen();
    fireEvent.click(await screen.findByText("Odd entry"));
    await screen.findByText("sec-odd");
    expect(warn).toHaveBeenCalledWith(expect.stringContaining('"nope"'));
    warn.mockRestore();
  });

  it("a throwing section only breaks its own boundary", async () => {
    const err = vi.spyOn(console, "error").mockImplementation(() => {});
    const Boom = () => {
      throw new Error("boom");
    };
    registerSettingsSection({ id: "boom", category: "data-backup", order: 1, component: Boom });
    registerSettingsSection({ id: "ok", category: "data-backup", order: 2, component: mk("sec-ok") });
    renderScreen();
    fireEvent.click((await screen.findAllByText("Daten und Backup")).at(0)!);
    await screen.findByText("builtin-backup");
    await screen.findByText("sec-ok");
    err.mockRestore();
  });
});
