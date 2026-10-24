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
vi.mock("../api", () => ({
  listAiProviders: vi.fn(() => Promise.resolve([])),
  discoverModels: vi.fn(),
  commandErrorMessage: (err: unknown) => String(err),
  commandErrorCode: () => null,
}));

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
