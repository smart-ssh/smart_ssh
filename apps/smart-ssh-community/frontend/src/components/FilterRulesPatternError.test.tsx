// Spec 0077 (BL-0249), T-6 und T-6e: Ein Muster, das sich nicht übersetzen
// lässt, muss im Regel-Formular verständlich UND mit der Stelle im Muster
// erscheinen (3.1.4) — und eine schon gespeicherte Regel mit einem solchen
// Muster muss in der Regelliste als wirkungslos erkennbar sein (3.2.3).
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { testI18n } from "../testI18n";
import type { RuleDto } from "../types";
import { FilterRulesView } from "./FilterRulesView";

/** Gekürzte Fassung dessen, was `regex` für `^systemctl stop (.*` liefert —
 * mit der Angabe, die den Text überhaupt nützlich macht. */
const LIBRARY_ERROR = "regex parse error: unclosed group";

const listRulesMock = vi.fn<() => Promise<RuleDto[]>>();
const createRuleMock = vi.fn();

vi.mock("../api", () => ({
  commandErrorMessage: (err: unknown) =>
    typeof err === "object" && err !== null && "message" in err
      ? String((err as { message: unknown }).message)
      : String(err),
  commandErrorCode: (err: unknown) =>
    typeof err === "object" && err !== null && "code" in err
      ? ((err as { code: string | null }).code ?? null)
      : null,
  createRule: (...args: unknown[]) => createRuleMock(...args),
  deleteRule: vi.fn(() => Promise.resolve()),
  evaluateExplained: vi.fn(() => Promise.resolve(null)),
  listHardBlacklist: vi.fn(() => Promise.resolve([])),
  listKnownTags: vi.fn(() => Promise.resolve([])),
  listRules: () => listRulesMock(),
  listServers: vi.fn(() => Promise.resolve([])),
  updateRule: vi.fn(() => Promise.resolve()),
}));

function validRule(): RuleDto {
  return {
    id: "rule-valid",
    patternType: "glob",
    patternValue: "ls *",
    action: "Allow",
    scope: "Global",
    priority: 0,
    patternError: null,
  };
}

function brokenRule(): RuleDto {
  return {
    id: "rule-broken",
    patternType: "regex",
    patternValue: "^systemctl stop (.*",
    action: "Deny",
    scope: "Global",
    priority: 100,
    patternError: LIBRARY_ERROR,
  };
}

function renderView() {
  return render(
    <I18nextProvider i18n={testI18n}>
      <FilterRulesView />
    </I18nextProvider>,
  );
}

const invalidPatternText = () =>
  testI18n.getFixedT("de")("errors.FILTER_RULE_PATTERN_INVALID");

beforeEach(() => {
  listRulesMock.mockReset();
  createRuleMock.mockReset();
});

describe("Regel-Formular bei ungültigem Muster (Spec 0077, T-6)", () => {
  // 3.1.4: `translateErrorCode` ersetzt bei bekanntem Code die `message`
  // vollständig. Zeigte das Formular nur den übersetzten Satz, verlöre der
  // Nutzer die Angabe, WO im Muster der Fehler sitzt; zeigte es nur die
  // `message`, stünde dort weiter der rohe englische Bibliothekstext.
  // Scheitert, wenn nur eines von beidem erscheint.
  it("zeigt den übersetzten Satz UND den Fehlertext der Bibliothek", async () => {
    listRulesMock.mockResolvedValue([]);
    createRuleMock.mockRejectedValue({
      message: LIBRARY_ERROR,
      code: "FILTER_RULE_PATTERN_INVALID",
    });
    renderView();

    fireEvent.click(await screen.findByRole("button", { name: /\+ Regel|\+ Rule/ }));
    fireEvent.change(screen.getByPlaceholderText("ls *"), {
      target: { value: "^systemctl stop (.*" },
    });
    fireEvent.click(screen.getByRole("button", { name: /^(Anlegen|Create)$/ }));

    await waitFor(() => expect(createRuleMock).toHaveBeenCalled());

    expect(await screen.findByText(invalidPatternText())).toBeInTheDocument();
    expect(screen.getByText(LIBRARY_ERROR)).toBeInTheDocument();
  });

  // Gegenprobe: Ein anderer Fehler behält das bisherige Verhalten (roher
  // `message`-Text, kein Detail darunter). Scheitert, wenn die neue
  // Sonderbehandlung pauschal auf jeden Fehler angewandt wird.
  it("lässt einen Fehler ohne diesen Code unverändert", async () => {
    listRulesMock.mockResolvedValue([]);
    createRuleMock.mockRejectedValue({ message: "Datenbankfehler: disk full", code: null });
    renderView();

    fireEvent.click(await screen.findByRole("button", { name: /\+ Regel|\+ Rule/ }));
    fireEvent.change(screen.getByPlaceholderText("ls *"), { target: { value: "ls *" } });
    fireEvent.click(screen.getByRole("button", { name: /^(Anlegen|Create)$/ }));

    expect(await screen.findByText("Datenbankfehler: disk full")).toBeInTheDocument();
  });
});

describe("Regelliste markiert ein ungültiges Muster (Spec 0077, T-6e)", () => {
  // 3.2.3: Eine Regel, die eine ältere Programmfassung gespeichert hat, kann
  // nicht greifen, soweit ihr Muster nicht übersetzt. Ohne Markierung sähe
  // sie in der Liste aus wie jede wirksame Regel — genau der stille Zustand,
  // den diese Spec beseitigt.
  it("zeigt Hinweis und Fehlertext an einer Regel mit patternError", async () => {
    listRulesMock.mockResolvedValue([brokenRule()]);
    renderView();

    const hint = await screen.findByRole("alert");
    expect(hint).toHaveTextContent(invalidPatternText());
    expect(hint).toHaveTextContent(LIBRARY_ERROR);
  });

  it("zeigt an einer gültigen Regel keinen Hinweis", async () => {
    listRulesMock.mockResolvedValue([validRule()]);
    renderView();

    await screen.findByText(/ls \*/);
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  // Bearbeiten und Löschen bleiben an einer markierten Regel möglich
  // (3.2.3) — die Markierung darf die Regel nicht unbedienbar machen, sonst
  // ließe sie sich nicht einmal reparieren.
  it("lässt Bearbeiten und Löschen an einer markierten Regel zu", async () => {
    listRulesMock.mockResolvedValue([brokenRule()]);
    renderView();

    await screen.findByRole("alert");
    expect(screen.getByRole("button", { name: /^(Bearbeiten|Edit)$/ })).toBeEnabled();
    expect(screen.getByRole("button", { name: /^(Löschen|Delete)$/ })).toBeEnabled();
  });
});
