// Issue #146: prüft, dass jede `jsx-a11y`-Regel, die oxlint aktiviert, in
// `apps/smart-ssh-community/frontend/.oxlintrc.json` ausdrücklich mit
// `error` steht (Spec 0100 A6.1, ADR 0116).
//
// oxlint kennt keinen Schweregrad je Plugin. Eine Regel, die eine neuere
// oxlint-Version dem Plugin hinzufügt, meldet deshalb nur eine Warnung, bis
// sie in die Liste kommt, und eine Warnung lässt CI nicht scheitern. Dieses
// Skript schließt die Lücke:
//
//   1. Welche Regeln aktiv sind, sagt oxlint selbst: `oxlint --print-config`
//      gibt die wirksame Konfiguration als JSON aus, mit jeder aktiven Regel
//      (Plugin-Präfix `jsx_a11y/`) samt Schweregrad (`deny`, `warn`, `allow`).
//      Eine neue Regel aus einer Standardkategorie steht dort, auch wenn sie
//      in `.oxlintrc.json` fehlt.
//   2. Jede dieser Regeln und jede `jsx-a11y`-Regel, die `.oxlintrc.json`
//      selbst nennt, muss dort mit `error` stehen (als Zeichenkette oder als
//      erstes Element der Array-Form `["error", { … }]`). Fehlt sie oder steht
//      ein anderer Schweregrad dort (`warn`, `off`, …), ist das ein Fund.
//
// Lässt sich oxlint nicht aufrufen oder seine Ausgabe nicht lesen, bricht
// das Skript mit einer Meldung ab, statt durchzuwinken. Dasselbe gilt, wenn
// oxlint gar keine `jsx_a11y`-Regel meldet (z. B. weil sich das Format
// geändert hat).
//
// Aufruf: `npm run check-a11y-rules` im Frontend; läuft außerdem als Teil von
// `npm run lint` und damit in jedem CI-Lauf.
//
// Ohne Shebang-Zeile, aus demselben Grund wie
// `generate-third-party-notices.mjs` (Issue #118): ein Frontend-Test
// importiert diese Datei.

import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(SCRIPT_DIR, "..");
const FRONTEND_DIR = path.join(REPO_ROOT, "apps/smart-ssh-community/frontend");
const CONFIG_FILE = ".oxlintrc.json";

/** Präfix, unter dem `oxlint --print-config` die Regeln des Plugins nennt. */
const PRINTED_PREFIX = "jsx_a11y/";
/** Präfixe, unter denen `.oxlintrc.json` die Regeln nennen darf. */
const CONFIG_PREFIXES = ["jsx-a11y/", "jsx_a11y/"];

/** Strukturfehler: eine Eingabe ist nicht eindeutig lesbar. */
export class ParseError extends Error {}

function fail(message) {
  throw new ParseError(message);
}

/**
 * Schweregrad eines Regelwerts: die Zeichenkette (oder Zahl) selbst bzw. das
 * erste Element der Array-Form. Alles andere ergibt `undefined`.
 */
export function severityOf(value) {
  if (typeof value === "string" || typeof value === "number") return value;
  if (Array.isArray(value) && value.length > 0) return severityOf(value[0]);
  return undefined;
}

function isOff(severity) {
  return severity === "off" || severity === "allow" || severity === 0 || severity === "0";
}

function parseJsonObject(text, what) {
  let parsed;
  try {
    parsed = JSON.parse(text);
  } catch (err) {
    fail(`${what} ist kein gültiges JSON (${err.message})`);
  }
  if (parsed === null || typeof parsed !== "object" || Array.isArray(parsed)) {
    fail(`${what} ist kein JSON-Objekt`);
  }
  return parsed;
}

/**
 * Liest die Ausgabe von `oxlint --print-config` und liefert die Namen (ohne
 * Präfix, sortiert) aller `jsx_a11y`-Regeln, die dort nicht ausgeschaltet
 * sind. Bricht ab, wenn die Ausgabe kein JSON ist, kein `rules`-Objekt hat
 * oder keine einzige `jsx_a11y`-Regel nennt.
 */
export function parseEnabledA11yRules(printConfigText) {
  const config = parseJsonObject(printConfigText, "Die Ausgabe von `oxlint --print-config`");
  const rules = config.rules;
  if (rules === null || typeof rules !== "object" || Array.isArray(rules)) {
    fail("Die Ausgabe von `oxlint --print-config` enthält kein `rules`-Objekt");
  }
  const enabled = [];
  let seen = 0;
  for (const [key, value] of Object.entries(rules)) {
    if (!key.startsWith(PRINTED_PREFIX)) continue;
    seen++;
    const severity = severityOf(value);
    if (severity === undefined) fail(`Schweregrad von \`${key}\` in der oxlint-Ausgabe ist nicht lesbar`);
    if (!isOff(severity)) enabled.push(key.slice(PRINTED_PREFIX.length));
  }
  if (seen === 0) {
    fail(
      "Die Ausgabe von `oxlint --print-config` nennt keine `jsx_a11y`-Regel. " +
        "Ist das Plugin `jsx-a11y` in `.oxlintrc.json` nicht mehr aktiv, oder hat sich das Ausgabeformat geändert?",
    );
  }
  return enabled.sort();
}

/**
 * Liest `.oxlintrc.json` und liefert je `jsx-a11y`-Regel (Name ohne Präfix)
 * den dort eingetragenen Rohwert.
 */
export function parseConfiguredA11yRules(configText) {
  const config = parseJsonObject(configText, CONFIG_FILE);
  const rules = config.rules ?? {};
  if (rules === null || typeof rules !== "object" || Array.isArray(rules)) {
    fail(`\`rules\` in ${CONFIG_FILE} ist kein Objekt`);
  }
  const configured = new Map();
  for (const [key, value] of Object.entries(rules)) {
    const prefix = CONFIG_PREFIXES.find((p) => key.startsWith(p));
    if (!prefix) continue;
    const name = key.slice(prefix.length);
    if (configured.has(name)) fail(`\`${name}\` steht in ${CONFIG_FILE} mehrfach (mit \`jsx-a11y/\` und \`jsx_a11y/\`)`);
    configured.set(name, value);
  }
  return configured;
}

/**
 * Vergleicht die aktiven Regeln mit der Konfiguration. Geprüft wird jede
 * aktive Regel und jede Regel, die die Konfiguration nennt (damit auch ein
 * `off` auffällt, das oxlint selbst nicht mehr als aktiv meldet). Fund:
 * Regel fehlt in der Konfiguration oder hat dort einen anderen Schweregrad
 * als `error`. Ergebnis nach Regelname sortiert.
 */
export function findA11yRuleProblems({ enabled, configured }) {
  const names = [...new Set([...enabled, ...configured.keys()])].sort();
  const problems = [];
  for (const name of names) {
    if (!configured.has(name)) {
      problems.push({ name, kind: "missing", severity: undefined });
      continue;
    }
    const severity = severityOf(configured.get(name));
    if (severity !== "error") problems.push({ name, kind: "severity", severity });
  }
  return problems;
}

/** Ruft das installierte oxlint mit `--print-config` auf und liefert stdout. */
export function runOxlintPrintConfig(frontendDir = FRONTEND_DIR) {
  const bin = path.join(frontendDir, "node_modules/oxlint/bin/oxlint");
  if (!fs.existsSync(bin)) fail(`oxlint ist nicht installiert (${bin} fehlt); erst \`npm ci\` im Frontend ausführen`);
  const result = spawnSync(process.execPath, [bin, "--print-config", "-c", CONFIG_FILE], {
    cwd: frontendDir,
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
  });
  if (result.error) fail(`\`oxlint --print-config\` ließ sich nicht starten: ${result.error.message}`);
  if (result.status !== 0) {
    fail(`\`oxlint --print-config\` endete mit Exit-Code ${result.status}: ${(result.stderr || result.stdout || "").trim()}`);
  }
  return result.stdout;
}

/**
 * Prüft das Frontend unter `frontendDir`. `printConfig` liefert die Ausgabe
 * von `oxlint --print-config` (in Tests ersetzbar).
 */
export function checkA11yRules({ frontendDir = FRONTEND_DIR, printConfig = runOxlintPrintConfig } = {}) {
  const enabled = parseEnabledA11yRules(printConfig(frontendDir));
  const configured = parseConfiguredA11yRules(fs.readFileSync(path.join(frontendDir, CONFIG_FILE), "utf8"));
  return { problems: findA11yRuleProblems({ enabled, configured }), enabledCount: enabled.length };
}

/** Meldungszeile für einen Fund, mit Regelname und Abhilfe. */
export function describeProblem(p) {
  const key = `jsx-a11y/${p.name}`;
  const what =
    p.kind === "missing"
      ? `${key} ist aktiv, steht aber nicht in ${CONFIG_FILE}`
      : `${key} steht in ${CONFIG_FILE} mit Schweregrad ${JSON.stringify(p.severity)} statt "error"`;
  return `${what} — in ${CONFIG_FILE} unter "rules" als "${key}": "error" eintragen`;
}

function main() {
  try {
    const { problems, enabledCount } = checkA11yRules();
    if (problems.length > 0) {
      console.error(
        `jsx-a11y-Regeln müssen Fehler sein (Spec 0100 A6.1, ADR 0116); ${problems.length} Abweichung(en):`,
      );
      for (const p of problems) console.error(`  - ${describeProblem(p)}`);
      process.exitCode = 1;
      return;
    }
    console.log(`jsx-a11y-Regeln ok: alle ${enabledCount} aktiven Regeln stehen mit "error" in ${CONFIG_FILE}.`);
  } catch (err) {
    const message = err instanceof ParseError ? err.message : `${err.stack ?? err}`;
    console.error(`Prüfung der jsx-a11y-Regeln fehlgeschlagen: ${message}`);
    process.exitCode = 1;
  }
}

// Nur ausführen, wenn direkt aufgerufen (nicht beim Import durch einen Test).
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main();
}
