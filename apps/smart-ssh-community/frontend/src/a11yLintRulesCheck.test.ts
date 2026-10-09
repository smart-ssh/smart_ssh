// @vitest-environment node
//
// Issue #146: `scripts/check-a11y-lint-rules.mjs` verlangt, dass jede
// `jsx-a11y`-Regel, die oxlint aktiviert, in `.oxlintrc.json` mit `error`
// steht (Spec 0100 A6.1, ADR 0116). Die Vergleichslogik läuft hier mit
// festen Eingaben; ein Fall ruft das installierte oxlint gegen die echte
// Konfiguration auf.
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import {
  checkA11yRules,
  describeProblem,
  findA11yRuleProblems,
  ParseError,
  parseConfiguredA11yRules,
  parseEnabledA11yRules,
  severityOf,
} from "../../../../scripts/check-a11y-lint-rules.mjs";

const FRONTEND_DIR = path.resolve(__dirname, "..");

/** Ausgabe im Format von `oxlint --print-config`. */
function printed(rules: Record<string, unknown>): string {
  return JSON.stringify({ plugins: ["jsx-a11y"], categories: {}, rules });
}

/** `.oxlintrc.json` mit den gegebenen Regeln. */
function config(rules: Record<string, unknown>): string {
  return JSON.stringify({ plugins: ["react", "jsx-a11y"], rules });
}

function problemsFor(enabled: string[], rules: Record<string, unknown>) {
  return findA11yRuleProblems({ enabled, configured: parseConfiguredA11yRules(config(rules)) });
}

const ALL_ERROR = {
  "react/rules-of-hooks": "error",
  "jsx-a11y/alt-text": "error",
  "jsx-a11y/no-autofocus": ["error", { ignoreNonDOM: true }],
  "jsx-a11y/label-has-associated-control": ["error", { depth: 3 }],
};
const ENABLED = ["alt-text", "label-has-associated-control", "no-autofocus"];

describe("findA11yRuleProblems", () => {
  it("passes when every enabled rule is configured as error", () => {
    expect(problemsFor(ENABLED, ALL_ERROR)).toEqual([]);
  });

  it("accepts the array form with options as error", () => {
    expect(problemsFor(["no-autofocus"], { "jsx-a11y/no-autofocus": ["error", { ignoreNonDOM: true }] })).toEqual([]);
  });

  it("reports an enabled rule that is missing from the config", () => {
    const rules: Record<string, unknown> = { ...ALL_ERROR };
    delete rules["jsx-a11y/alt-text"];
    expect(problemsFor(ENABLED, rules)).toEqual([{ name: "alt-text", kind: "missing", severity: undefined }]);
  });

  it("reports a new rule that oxlint enables but the config does not list", () => {
    expect(problemsFor([...ENABLED, "brand-new-rule"], ALL_ERROR)).toEqual([
      { name: "brand-new-rule", kind: "missing", severity: undefined },
    ]);
  });

  it("reports a rule configured as warn", () => {
    expect(problemsFor(ENABLED, { ...ALL_ERROR, "jsx-a11y/alt-text": "warn" })).toEqual([
      { name: "alt-text", kind: "severity", severity: "warn" },
    ]);
  });

  it("reports a rule configured as warn in the array form", () => {
    expect(
      problemsFor(ENABLED, { ...ALL_ERROR, "jsx-a11y/no-autofocus": ["warn", { ignoreNonDOM: true }] }),
    ).toEqual([{ name: "no-autofocus", kind: "severity", severity: "warn" }]);
  });

  it("reports a rule configured as off, even when oxlint no longer lists it as enabled", () => {
    const enabledWithoutAltText = ENABLED.filter((r) => r !== "alt-text");
    expect(problemsFor(enabledWithoutAltText, { ...ALL_ERROR, "jsx-a11y/alt-text": "off" })).toEqual([
      { name: "alt-text", kind: "severity", severity: "off" },
    ]);
  });

  it("does not accept the numeric or deny aliases, only the literal error", () => {
    expect(
      problemsFor(["alt-text", "lang"], { "jsx-a11y/alt-text": 2, "jsx-a11y/lang": "deny" }).map((p) => p.name),
    ).toEqual(["alt-text", "lang"]);
  });

  it("names the rule and the fix in the message", () => {
    const [problem] = problemsFor(["alt-text"], { "jsx-a11y/alt-text": "warn" });
    const message = describeProblem(problem);
    expect(message).toContain("jsx-a11y/alt-text");
    expect(message).toContain('"warn"');
    expect(message).toContain('"jsx-a11y/alt-text": "error"');
    expect(describeProblem({ name: "scope", kind: "missing", severity: undefined })).toContain(
      '"jsx-a11y/scope": "error"',
    );
  });
});

describe("severityOf", () => {
  it("reads the string form and the first element of the array form", () => {
    expect(severityOf("error")).toBe("error");
    expect(severityOf(["error", { depth: 3 }])).toBe("error");
    expect(severityOf(["warn"])).toBe("warn");
    expect(severityOf([])).toBeUndefined();
    expect(severityOf({ severity: "error" })).toBeUndefined();
  });
});

describe("parseEnabledA11yRules", () => {
  it("returns the enabled jsx_a11y rules, skipping other plugins and switched-off rules", () => {
    const text = printed({
      "react/rules-of-hooks": "deny",
      "jsx_a11y/scope": "deny",
      "jsx_a11y/no-autofocus": ["deny", [{ ignoreNonDOM: true }]],
      "jsx_a11y/new-rule": "warn",
      "jsx_a11y/lang": "allow",
    });
    expect(parseEnabledA11yRules(text)).toEqual(["new-rule", "no-autofocus", "scope"]);
  });

  it.each([
    ["not JSON", "this is not json"],
    ["empty output", ""],
    ["a JSON array", "[]"],
    ["no rules object", JSON.stringify({ plugins: [] })],
    ["rules as an array", JSON.stringify({ rules: [] })],
    ["no jsx_a11y rule at all", printed({ "react/rules-of-hooks": "deny" })],
    ["an unreadable severity", printed({ "jsx_a11y/scope": { level: "deny" } })],
  ])("fails instead of passing on %s", (_label, text) => {
    expect(() => parseEnabledA11yRules(text)).toThrow(ParseError);
  });
});

describe("parseConfiguredA11yRules", () => {
  it("collects jsx-a11y rules under both prefixes and ignores other plugins", () => {
    const configured = parseConfiguredA11yRules(
      config({ "react/rules-of-hooks": "error", "jsx-a11y/scope": "error", "jsx_a11y/lang": "warn" }),
    );
    expect([...configured.entries()]).toEqual([
      ["scope", "error"],
      ["lang", "warn"],
    ]);
  });

  it("rejects a rule listed under both prefixes", () => {
    expect(() => parseConfiguredA11yRules(config({ "jsx-a11y/scope": "error", "jsx_a11y/scope": "warn" }))).toThrow(
      ParseError,
    );
  });

  it("rejects an unparseable config", () => {
    expect(() => parseConfiguredA11yRules("{ not json")).toThrow(ParseError);
  });
});

describe("checkA11yRules", () => {
  const tmpDirs: string[] = [];
  afterEach(() => {
    for (const d of tmpDirs.splice(0)) fs.rmSync(d, { recursive: true, force: true });
  });

  function frontendWith(rules: Record<string, unknown>): string {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), "a11y-rules-"));
    tmpDirs.push(dir);
    fs.writeFileSync(path.join(dir, ".oxlintrc.json"), config(rules));
    return dir;
  }

  it("fails when the oxlint call itself fails", () => {
    const frontendDir = frontendWith(ALL_ERROR);
    const printConfig = () => {
      throw new ParseError("`oxlint --print-config` endete mit Exit-Code 1");
    };
    expect(() => checkA11yRules({ frontendDir, printConfig })).toThrow(ParseError);
  });

  it("fails when the oxlint output cannot be parsed", () => {
    const frontendDir = frontendWith(ALL_ERROR);
    expect(() => checkA11yRules({ frontendDir, printConfig: () => "garbage" })).toThrow(ParseError);
  });

  it("compares the oxlint output with the config file", () => {
    const frontendDir = frontendWith({ ...ALL_ERROR, "jsx-a11y/alt-text": "warn" });
    const printConfig = () =>
      printed({ "jsx_a11y/alt-text": "warn", "jsx_a11y/no-autofocus": "deny", "jsx_a11y/scope": "warn" });
    const { problems, enabledCount } = checkA11yRules({ frontendDir, printConfig });
    expect(enabledCount).toBe(3);
    expect(problems.map((p) => [p.name, p.kind])).toEqual([
      ["alt-text", "severity"],
      ["scope", "missing"],
    ]);
  });

  it("finds no problem in the real config with the installed oxlint", () => {
    const { problems, enabledCount } = checkA11yRules({ frontendDir: FRONTEND_DIR });
    expect(problems).toEqual([]);
    expect(enabledCount).toBeGreaterThan(0);
  });
});
