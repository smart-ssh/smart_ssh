// Issue #7: Die Kennzeile der Drittlizenz-Datei (Spec 0099, A1.8) steht
// maßgeblich in scripts/generate-third-party-notices.mjs (`MARKER`). Die
// Frontend-Kopie und die Prüfung im Release-Workflow können sie nicht
// importieren (App ohne Abhängigkeit auf ein Build-Skript; YAML kann kein
// Modul laden). Dieser Test vergleicht deshalb beide Kopien mit der Quelle
// und schlägt fehl, sobald eine allein geändert wird.
//
// Die Quelle wird als Text gelesen statt importiert: ein Import der
// `.mjs`-Datei bräuchte eigene Typdeklarationen für `tsc -b`, und der Text
// genügt für eine einzige konstante Zeichenkette.
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { THIRD_PARTY_NOTICES_MARKER } from "./thirdPartyNoticesMarker";

const SRC_DIR = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(SRC_DIR, "../../../..");
const GENERATOR_PATH = path.join(REPO_ROOT, "scripts/generate-third-party-notices.mjs");
const RELEASE_WORKFLOW_PATH = path.join(REPO_ROOT, ".github/workflows/release.yml");

// Gemeinsamer Präfix aller denkbaren Versionen der Kennzeile — damit findet
// der Test auch Kopien, die schon auseinandergelaufen sind (z. B. "-V2").
const MARKER_FAMILY = /SMART-SSH-THIRD-PARTY-NOTICES[A-Za-z0-9-]*/g;

function generatorMarker(): string {
  const source = fs.readFileSync(GENERATOR_PATH, "utf8");
  const matches = [...source.matchAll(/^export const MARKER = "([^"\\]+)";$/gm)];
  expect(matches, "genau eine Definition `export const MARKER = \"…\";` im Skript").toHaveLength(1);
  return matches[0][1];
}

function listSourceFiles(dir: string): string[] {
  return fs.readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) return listSourceFiles(full);
    return /\.(ts|tsx)$/.test(entry.name) ? [full] : [];
  });
}

describe("Drittlizenzen-Kennzeile: eine Quelle (Issue #7)", () => {
  it("Frontend-Kopie stimmt mit `MARKER` im Generierungs-Skript überein", () => {
    expect(THIRD_PARTY_NOTICES_MARKER).toBe(generatorMarker());
  });

  it("das Skript verwendet die Kennzeile sonst nirgends als eigenes Literal", () => {
    const source = fs.readFileSync(GENERATOR_PATH, "utf8");
    expect(source.match(MARKER_FAMILY)).toEqual([generatorMarker()]);
  });

  it("der Release-Workflow prüft auf genau diese Kennzeile", () => {
    const workflow = fs.readFileSync(RELEASE_WORKFLOW_PATH, "utf8");
    const checks = [...workflow.matchAll(/grep -q "\^([^"]+)"/g)].map((m) => m[1]);
    expect(checks, "genau eine `grep -q \"^…\"`-Prüfung in release.yml").toHaveLength(1);
    expect(checks[0]).toBe(generatorMarker());
    // Keine weitere, abweichende Kopie irgendwo sonst im Workflow.
    expect(workflow.match(MARKER_FAMILY)).toEqual([generatorMarker()]);
  });

  it("im Frontend steht die Kennzeile nur in thirdPartyNoticesMarker.ts", () => {
    const owner = path.join(SRC_DIR, "thirdPartyNoticesMarker.ts");
    const offenders = listSourceFiles(SRC_DIR)
      .filter((file) => file !== owner && file !== fileURLToPath(import.meta.url))
      .filter((file) => fs.readFileSync(file, "utf8").match(MARKER_FAMILY) !== null)
      .map((file) => path.relative(SRC_DIR, file));
    expect(offenders).toEqual([]);
  });
});
