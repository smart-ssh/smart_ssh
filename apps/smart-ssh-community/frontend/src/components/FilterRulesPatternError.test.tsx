// Spec 0077 (BL-0249), T-6 und T-6e: Ein Muster, das sich nicht übersetzen
// lässt, muss im Regel-Formular verständlich UND mit der Stelle im Muster
// erscheinen (3.1.4) — und eine schon gespeicherte Regel mit einem solchen
// Muster muss in der Regelliste als wirkungslos erkennbar sein (3.2.3).
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { testI18n } from "../testI18n";
import type { EvaluationTraceDto, RuleDto } from "../types";
import { FilterRulesView } from "./FilterRulesView";

/** Gekürzte Fassung dessen, was `regex` für `^systemctl stop (.*` liefert —
 * mit der Angabe, die den Text überhaupt nützlich macht. */
const LIBRARY_ERROR = "regex parse error: unclosed group";

const listRulesMock = vi.fn<() => Promise<RuleDto[]>>();
const createRuleMock = vi.fn();
// Rest-Parameter statt fester Arity (wie bei `createRuleMock` oben, das ganz
// ohne Implementierung bleibt): Erst dadurch hat der Mock eine
// `(...args: unknown[]) => …`-Signatur, in die sich `...args` weiter unten
// spreaden lässt — mit `vi.fn(() => Promise.resolve())` (feste 0-Arity)
// scheiterte das an TS2556 (spec-reviewer-Fund).
const updateRuleMock = vi.fn((..._args: unknown[]) => Promise.resolve());
const swapRulePrioritiesMock = vi.fn((..._args: unknown[]) => Promise.resolve());
const evaluateExplainedMock = vi.fn<(...args: unknown[]) => Promise<EvaluationTraceDto | null>>(
  () => Promise.resolve(null),
);

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
  evaluateExplained: (...args: unknown[]) => evaluateExplainedMock(...args),
  swapRulePriorities: (...args: unknown[]) => swapRulePrioritiesMock(...args),
  listHardBlacklist: vi.fn(() => Promise.resolve([])),
  listKnownTags: vi.fn(() => Promise.resolve([])),
  listRules: () => listRulesMock(),
  listServers: vi.fn(() => Promise.resolve([])),
  updateRule: (...args: unknown[]) => updateRuleMock(...args),
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
  updateRuleMock.mockReset();
  swapRulePrioritiesMock.mockReset();
  swapRulePrioritiesMock.mockResolvedValue(undefined);
  evaluateExplainedMock.mockReset();
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

// Spec 0077, 3.2.3: Die Pfeiltasten an einer Regel mit
// ungültigem Muster dürfen die Priorität nicht verschieben — weder die
// eigenen Pfeile noch die einer Nachbarregel, deren Verschieben die
// markierte Regel mitbeträfe. `updateRule` darf in keinem der beiden Fälle
// aufgerufen werden.
describe("Prioritäts-Pfeile an einer Regel mit ungültigem Muster (Spec 0077, 3.2.3)", () => {
  function ruleAt(id: string, priority: number, patternError: string | null): RuleDto {
    return {
      id,
      patternType: "glob",
      patternValue: `${id}-pattern *`,
      action: "Allow",
      scope: "Global",
      priority,
      patternError,
    };
  }

  it("deaktiviert beide Pfeile an der markierten Regel selbst, mit Begründung im title", async () => {
    // Drei Regeln im selben Scope, die markierte in der Mitte (Prio 100),
    // damit "deaktiviert" nicht bloß der triviale Rand-Fall (erste/letzte
    // Regel) ist, sondern wirklich an `patternError` hängt.
    listRulesMock.mockResolvedValue([
      ruleAt("rule-top", 200, null),
      ruleAt("rule-broken", 100, LIBRARY_ERROR),
      ruleAt("rule-bottom", 0, null),
    ]);
    renderView();

    await screen.findByText(/rule-broken-pattern/);
    const brokenRow = screen.getByText(/rule-broken-pattern/).closest("li")!;
    const up = within(brokenRow).getByRole("button", { name: /Priorität erhöhen|Increase priority/ });
    const down = within(brokenRow).getByRole("button", {
      name: /Priorität senken|Decrease priority/,
    });

    expect(up).toBeDisabled();
    expect(down).toBeDisabled();
    expect(up).toHaveAttribute("title", expect.stringMatching(/./));

    fireEvent.click(up);
    fireEvent.click(down);
    expect(updateRuleMock).not.toHaveBeenCalled();
  });

  it("deaktiviert den Pfeil einer gültigen Regel, wenn die Nachbarregel patternError trägt, mit Begründung", async () => {
    listRulesMock.mockResolvedValue([
      ruleAt("rule-top", 200, null),
      ruleAt("rule-broken", 100, LIBRARY_ERROR),
      ruleAt("rule-bottom", 0, null),
    ]);
    renderView();

    await screen.findByText(/rule-top-pattern/);
    const neighbourReason = testI18n.getFixedT("de")("filterRules.priorityDisabledNeighbourPatternError");
    const topRow = screen.getByText(/rule-top-pattern/).closest("li")!;
    const topDown = within(topRow).getByRole("button", {
      name: /Priorität senken|Decrease priority/,
    });
    expect(topDown).toBeDisabled();
    expect(topDown).toHaveAttribute("title", neighbourReason);

    const bottomRow = screen.getByText(/rule-bottom-pattern/).closest("li")!;
    const bottomUp = within(bottomRow).getByRole("button", {
      name: /Priorität erhöhen|Increase priority/,
    });
    expect(bottomUp).toBeDisabled();
    expect(bottomUp).toHaveAttribute("title", neighbourReason);

    fireEvent.click(topDown);
    fireEvent.click(bottomUp);
    expect(swapRulePrioritiesMock).not.toHaveBeenCalled();
    expect(updateRuleMock).not.toHaveBeenCalled();
  });

  // Positivkontrolle (spec-reviewer-Fund, Runde 2): Die beiden Tests oben
  // sind rein negativ — ohne diesen Fall bliebe eine Guard-Bedingung, die
  // versehentlich *jeden* Klick abfängt (nicht nur die mit `patternError`
  // beteiligten), unbemerkt grün.
  it("verschiebt zwei gültige Nachbarregeln weiterhin per Prioritätentausch", async () => {
    listRulesMock.mockResolvedValue([
      ruleAt("rule-top", 200, null),
      ruleAt("rule-bottom", 100, null),
    ]);
    renderView();

    await screen.findByText(/rule-top-pattern/);
    const topRow = screen.getByText(/rule-top-pattern/).closest("li")!;
    const topDown = within(topRow).getByRole("button", {
      name: /Priorität senken|Decrease priority/,
    });

    fireEvent.click(topDown);

    await waitFor(() => expect(swapRulePrioritiesMock).toHaveBeenCalledTimes(1));
    expect(swapRulePrioritiesMock).toHaveBeenCalledWith("rule-top", "rule-bottom");
    expect(updateRuleMock).not.toHaveBeenCalled();
    // Danach wird neu geladen: einmal beim Start, einmal nach dem Tausch.
    await waitFor(() => expect(listRulesMock).toHaveBeenCalledTimes(2));
  });
});

describe("Testansicht markiert ein ungültiges Muster (Spec 0077, 3.2.3)", () => {
  it("zeigt Hinweis und Fehlertext für eine Regel mit patternError neben dem Ergebnis", async () => {
    listRulesMock.mockResolvedValue([validRule(), brokenRule()]);
    evaluateExplainedMock.mockResolvedValue({
      decision: "AutoExec",
      matchedRule: "rule-valid",
      matchedHardBlacklistEntry: null,
      subCommandTraces: [],
    });
    renderView();

    await screen.findByText(/systemctl stop/);
    fireEvent.change(screen.getByPlaceholderText("ls -la && rm -rf /tmp/x"), {
      target: { value: "systemctl stop nginx" },
    });
    fireEvent.click(screen.getByRole("button", { name: /^(Testen|Test)$/ }));

    const marker = await screen.findByTestId("test-invalid-pattern-rules");
    expect(marker).toHaveTextContent("^systemctl stop (.*");
    expect(marker).toHaveTextContent(invalidPatternText());
    expect(marker).toHaveTextContent(LIBRARY_ERROR);
    expect(marker).not.toHaveTextContent("ls *");
  });
});
