// Issue #116 / Spec 0024, Abschnitt 2: Jeder für Nutzer sichtbare Text im
// Frontend läuft über die i18n-Schicht. Dieser Test ist das Sicherheitsnetz
// dagegen, dass wieder deutscher Text fest im Code landet: Er durchsucht
// alle Nicht-Test-`.ts`/`.tsx`-Dateien unter `src/` mit dem
// TypeScript-Parser und schlägt bei JSX-Text oder String-Literalen mit
// deutschen Sonderzeichen (ä ö ü Ä Ö Ü ß) oder einem deutschen
// Anführungszeichen („) an.
//
// Bewusst ignoriert: Kommentare (tauchen im Syntaxbaum gar nicht als Knoten
// auf), alles innerhalb eines `console.*`-Aufrufs (Entwickler-Ausgabe) und
// die Sprachdateien (JSON, ohnehin nicht im Suchraster).
//
// Der Test ist kein vollständiger Detektor: Deutsche Wörter ohne Umlaut
// ("Stopp", "Abbrechen") erkennt er nicht.
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import ts from "typescript";
import { describe, expect, it } from "vitest";

const SRC_DIR = path.dirname(fileURLToPath(import.meta.url));

const GERMAN_MARKERS = /[äöüÄÖÜß„]/;

/** Dateien (relativ zu `src/`), die der Test vorerst auslässt. Die Liste
 * darf nur schrumpfen, nie wachsen.
 *
 * - `components/FileBrowserPanel.tsx`: Die Dialoge des Dateibrowsers werden
 *   in einem eigenen Issue übersetzt (#91, "Translate the remaining
 *   hard-coded German texts in the file browser dialogs"). Ist das erledigt,
 *   fliegt der Eintrag hier raus. */
const ALLOWLIST: readonly string[] = ["components/FileBrowserPanel.tsx"];

interface GermanTextFinding {
  line: number;
  text: string;
}

function isConsoleCall(node: ts.Node): boolean {
  if (!ts.isCallExpression(node)) return false;
  const callee = node.expression;
  return (
    ts.isPropertyAccessExpression(callee) &&
    ts.isIdentifier(callee.expression) &&
    callee.expression.text === "console"
  );
}

function textOf(node: ts.Node): string | null {
  if (ts.isJsxText(node)) return node.text;
  if (ts.isStringLiteral(node) || ts.isNoSubstitutionTemplateLiteral(node)) return node.text;
  if (ts.isTemplateHead(node) || ts.isTemplateMiddle(node) || ts.isTemplateTail(node)) {
    return node.text;
  }
  return null;
}

/** Liefert alle JSX-Texte und String-Literale einer Quelldatei mit
 * deutschen Sonderzeichen, außer in Kommentaren und `console.*`-Aufrufen. */
function findGermanText(source: string, fileName: string): GermanTextFinding[] {
  const kind = fileName.endsWith(".tsx") ? ts.ScriptKind.TSX : ts.ScriptKind.TS;
  const sourceFile = ts.createSourceFile(fileName, source, ts.ScriptTarget.Latest, true, kind);
  const findings: GermanTextFinding[] = [];
  const visit = (node: ts.Node): void => {
    if (isConsoleCall(node)) return;
    const text = textOf(node);
    if (text !== null && GERMAN_MARKERS.test(text)) {
      const { line } = sourceFile.getLineAndCharacterOfPosition(node.getStart(sourceFile));
      findings.push({ line: line + 1, text: text.trim() });
    }
    ts.forEachChild(node, visit);
  };
  visit(sourceFile);
  return findings;
}

function sourceFiles(dir: string): string[] {
  return fs.readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) return sourceFiles(full);
    if (!/\.tsx?$/.test(entry.name)) return [];
    if (/\.test\.tsx?$/.test(entry.name) || entry.name.endsWith(".d.ts")) return [];
    return [full];
  });
}

function relative(file: string): string {
  return path.relative(SRC_DIR, file).split(path.sep).join("/");
}

describe("findGermanText", () => {
  it("findet deutschen JSX-Text", () => {
    const source = "export const X = () => <span>Schließen</span>;";
    expect(findGermanText(source, "x.tsx")).toEqual([{ line: 1, text: "Schließen" }]);
  });

  it("findet String-Literale, Attribute und Template-Strings", () => {
    const source = [
      'const a = "Später";',
      'const b = <button aria-label="Hinweis schließen" />;',
      "const c = `Datei ${name} gelöscht`;",
      'const d = "„zitiert“";',
    ].join("\n");
    expect(findGermanText(source, "x.tsx").map((f) => f.line)).toEqual([1, 2, 3, 4]);
  });

  it("ignoriert Kommentare", () => {
    const source = [
      "// Schließt den Dialog, wenn Ä gedrückt wird",
      "/* Mehrzeilig: Übernahme der Änderung */",
      "/** JSDoc: Größe */",
      "export const X = () => <div>{/* JSX-Kommentar: Lösung */}ok</div>;",
    ].join("\n");
    expect(findGermanText(source, "x.tsx")).toEqual([]);
  });

  it("ignoriert console.*-Aufrufe", () => {
    const source = [
      'console.error("Sprache konnte nicht gelesen werden — ungültig:", err);',
      "console.warn(`Größe ${n} überschritten`);",
      'console.log("Öffnen", { reason: "Lösung" });',
    ].join("\n");
    expect(findGermanText(source, "x.ts")).toEqual([]);
  });
});

describe("kein fest eingebauter deutscher Text im Frontend (Spec 0024)", () => {
  it("alle Nicht-Test-Quelldateien außerhalb der Allowlist sind frei davon", () => {
    const files = sourceFiles(SRC_DIR);
    const offenders = files
      .filter((file) => !ALLOWLIST.includes(relative(file)))
      .flatMap((file) =>
        findGermanText(fs.readFileSync(file, "utf8"), file).map(
          (f) => `${relative(file)}:${f.line}: ${f.text}`,
        ),
      );
    expect(offenders, offenders.join("\n")).toEqual([]);
  });

  it("jeder Allowlist-Eintrag existiert noch", () => {
    for (const entry of ALLOWLIST) {
      expect(fs.existsSync(path.join(SRC_DIR, entry)), entry).toBe(true);
    }
  });
});
