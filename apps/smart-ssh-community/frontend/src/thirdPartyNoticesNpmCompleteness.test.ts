// Spec 0099 (BL-0054), T5: zählt die produktiv genutzten npm-Pakete zur
// Laufzeit über `npm ls --omit=dev --all --json`, **unabhängig vom Code**
// von `scripts/generate-third-party-notices.mjs` (eigene Traversierung
// hier, keine Wiederverwendung von dessen `collectProdPackages`) — ein Bug
// in der Dedup-/Traversierungslogik des Skripts selbst würde sich sonst
// nicht zeigen können, weil Test und Skript denselben falschen Code teilen
// würden. Verlangt Gleichheit mit den in der Ausgabe gelisteten npm-Paketen
// (Produktionsabhängigkeiten + die devDependencies aus A1.1 c).
//
// Läuft nur, wenn `public/third-party-notices.txt` existiert (A2.4: ein
// Dev-Build/`npm test` ohne vorherigen `generate-notices`-Lauf hat die
// Datei nicht — das ist kein Fehler dieses Tests, s. A3.1).
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const FRONTEND_DIR = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const NOTICES_PATH = path.join(FRONTEND_DIR, "public/third-party-notices.txt");

// Muss A1.1(c) widerspiegeln (heute: tailwindcss, vite — s. Spec 0099,
// Ist-Stand). Bewusst hier dupliziert statt aus dem Skript importiert,
// s. Kommentar oben.
const BUNDLED_DEV_DEPS = ["tailwindcss", "vite"];

// Klarstellung Spec 0099 Abschnitt 9 (2026-10-01, Windows): `npm` ist unter
// Windows `npm.cmd`, kein `.exe` — `execFileSync` findet es ohne Shell
// nicht. Die Argumentliste bleibt fest (keine Nutzereingabe), nur unter
// `win32` kommt `shell: true` dazu, analog zu `spawnCommand` im
// Generierungs-Skript.
function runNpmLsJson(): string {
  const useShell = process.platform === "win32";
  try {
    return execFileSync("npm", ["ls", "--omit=dev", "--all", "--json"], {
      cwd: FRONTEND_DIR,
      encoding: "utf8",
      maxBuffer: 1024 * 1024 * 64,
      ...(useShell ? { shell: true } : {}),
    });
  } catch (err) {
    const stdout = (err as { stdout?: string }).stdout;
    if (stdout) return stdout;
    throw err;
  }
}

function independentProdPackageSet(): Set<string> {
  const tree = JSON.parse(runNpmLsJson()) as { dependencies?: Record<string, unknown> };
  const set = new Set<string>();
  function walk(node: { dependencies?: Record<string, unknown> }) {
    for (const [name, rawInfo] of Object.entries(node.dependencies ?? {})) {
      const info = rawInfo as { version?: string; dependencies?: Record<string, unknown> };
      if (info.version) set.add(`${name}@${info.version}`);
      walk(info);
    }
  }
  walk(tree);
  for (const name of BUNDLED_DEV_DEPS) {
    const pkgJsonPath = path.join(FRONTEND_DIR, "node_modules", name, "package.json");
    const pkg = JSON.parse(fs.readFileSync(pkgJsonPath, "utf8")) as { version: string };
    set.add(`${name}@${pkg.version}`);
  }
  return set;
}

function packagesInOutput(): Set<string> {
  const content = fs.readFileSync(NOTICES_PATH, "utf8");
  const sectionHeadings = [
    "## npm-Produktionsabhaengigkeiten",
    "## Build-Werkzeuge mit ausgeliefertem Code (devDependencies)",
  ];
  const set = new Set<string>();
  for (const heading of sectionHeadings) {
    const headingIndex = content.indexOf(heading);
    if (headingIndex === -1) continue;
    const nextHeadingIndex = content.indexOf("\n## ", headingIndex + 1);
    const body = content.slice(headingIndex, nextHeadingIndex === -1 ? undefined : nextHeadingIndex);
    for (const match of body.matchAll(/^Verwendet von: (.+)$/gm)) {
      for (const token of match[1].split(", ")) set.add(token.trim());
    }
  }
  return set;
}

describe("Drittlizenzen: npm-Seite vollständig (Spec 0099, T5)", () => {
  const hasOutput = fs.existsSync(NOTICES_PATH);

  it.skipIf(!hasOutput)(
    "jedes produktiv genutzte npm-Paket (inkl. A1.1 c) erscheint genau einmal in der Ausgabe, keine überzähligen",
    () => {
      const expected = independentProdPackageSet();
      const actual = packagesInOutput();

      const missing = [...expected].filter((p) => !actual.has(p)).sort();
      const extra = [...actual].filter((p) => !expected.has(p)).sort();

      expect(missing, `fehlt in der Ausgabe: ${missing.join(", ")}`).toEqual([]);
      expect(extra, `überzählig in der Ausgabe: ${extra.join(", ")}`).toEqual([]);
    },
  );
});
