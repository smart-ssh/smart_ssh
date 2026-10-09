// Issue #106 (Spec 0048 §2): jeder veröffentlichte Versionsabschnitt in
// `CHANGELOG.md` ab der Version nach `CUTOFF_VERSION` beginnt mit
// `### Highlights`: 3 bis 6 Listenpunkte, je genau eine Zeile. Ältere
// Abschnitte und `[Unreleased]` sind ausgenommen.
//
// Überschrift: `## [X.Y.Z] — YYYY-MM-DD` (Gedankenstrich; ein einfacher
// Bindestrich wird toleriert). Textbasiert und ohne Abhängigkeiten.

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(SCRIPT_DIR, "..");

/** Letzte Version ohne Highlights; geprüft werden nur höhere Versionen. */
export const CUTOFF_VERSION = "0.5.2";
export const MIN_ITEMS = 3;
export const MAX_ITEMS = 6;

const SECTION_RE = /^##\s+\[([^\]]+)\]\s*(?:[—–-]\s*(.*))?$/;

function compareVersions(a, b) {
  const pa = a.split(".").map((n) => Number.parseInt(n, 10));
  const pb = b.split(".").map((n) => Number.parseInt(n, 10));
  for (let i = 0; i < 3; i++) {
    const d = (pa[i] || 0) - (pb[i] || 0);
    if (d !== 0) return d;
  }
  return 0;
}

/** Zerlegt den Changelog in Versionsabschnitte (`[Unreleased]` inklusive). */
export function parseSections(text) {
  const sections = [];
  let current = null;
  for (const line of text.split(/\r?\n/)) {
    if (line.startsWith("## ")) {
      const m = SECTION_RE.exec(line.trimEnd());
      current = m ? { version: m[1], lines: [] } : null;
      if (current) sections.push(current);
    } else if (current) {
      current.lines.push(line);
    }
  }
  return sections;
}

/** Liefert Abweichungen als `{ version, message }`. */
export function findHighlightProblems(text, cutoff = CUTOFF_VERSION) {
  const problems = [];
  for (const s of parseSections(text)) {
    if (!/^\d+\.\d+\.\d+$/.test(s.version)) continue; // Unreleased
    if (compareVersions(s.version, cutoff) <= 0) continue;
    const fail = (message) => problems.push({ version: s.version, message });

    const first = s.lines.findIndex((l) => l.trim() !== "");
    if (first === -1 || s.lines[first].trim() !== "### Highlights") {
      fail("`### Highlights` fehlt oder steht nicht als erste Überschrift");
      continue;
    }
    const items = [];
    for (let i = first + 1; i < s.lines.length; i++) {
      const line = s.lines[i];
      if (line.startsWith("#")) break;
      if (line.trim() === "") continue;
      if (/^- \S/.test(line)) items.push(line);
      else fail(`Highlights-Eintrag über mehrere Zeilen oder kein Listenpunkt: "${line.trim()}"`);
    }
    if (items.length < MIN_ITEMS || items.length > MAX_ITEMS) {
      fail(`Highlights haben ${items.length} Einträge, erlaubt sind ${MIN_ITEMS} bis ${MAX_ITEMS}`);
    }
  }
  return problems;
}

export function checkChangelog(root = REPO_ROOT) {
  const text = fs.readFileSync(path.join(root, "CHANGELOG.md"), "utf8");
  return findHighlightProblems(text);
}

function main() {
  try {
    const problems = checkChangelog();
    if (problems.length > 0) {
      console.error(`CHANGELOG-Highlights fehlerhaft (${problems.length} Abweichung(en)):`);
      for (const p of problems) console.error(`  - [${p.version}] ${p.message}`);
      process.exitCode = 1;
      return;
    }
    console.log(`CHANGELOG-Highlights ok (geprüft ab Version nach ${CUTOFF_VERSION}).`);
  } catch (err) {
    console.error(`Prüfung der CHANGELOG-Highlights fehlgeschlagen: ${err.stack ?? err}`);
    process.exitCode = 1;
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main();
}
