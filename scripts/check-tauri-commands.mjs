// Issue #101: prüft, dass die drei Stellen, an denen ein Tauri-Kommando lebt,
// zueinander passen:
//
//   1. Definition   — `#[tauri::command]` vor einer `fn` unter
//                     `crates/app-shell/src/`
//   2. Registrierung — Eintrag in `tauri::generate_handler![...]` in
//                     `crates/app-shell/src/lib.rs` (letztes Pfadsegment)
//   3. Aufruf        — Zeichenkettenliteral als erstes Argument von
//                     `invoke(...)` / `invoke<...>(...)` in Nicht-Test-Dateien
//                     unter `apps/smart-ssh-community/frontend/src/`
//                     (ohne Plugin-Kommandos `plugin:...`)
//
// Fehler: Definition ohne Registrierung, Registrierung ohne Definition,
// Aufruf eines nicht registrierten Namens. Zusätzlich muss jeder Name in der
// Positivliste des Starttors (`ALLOWED_WHILE_LOCKED` in
// `crates/app-shell/src/startup_gate.rs`) registriert sein. Registriert, aber
// vom Frontend nicht aufgerufen, ist erlaubt (Editionen, Tests).
//
// Bewusst textbasiert und ohne Abhängigkeiten. Wo die Auslegung mehrdeutig
// wird (zweites `generate_handler!`, ein `#[tauri::command]` ohne erkennbare
// `fn`, ein nicht lesbarer Listeneintrag, doppelte Namen, ein Kommando-Attribut
// in anderer Form als `#[tauri::command]` oder ein `use tauri::command`),
// bricht das Skript mit einer Meldung ab, statt zu raten.
//
// Aufruf: `npm run check-commands` im Frontend; läuft außerdem als erster
// Teil von `npm run lint` und damit in jedem CI-Lauf.
//
// Ohne Shebang-Zeile, aus demselben Grund wie
// `generate-third-party-notices.mjs` (Issue #118): ein Frontend-Test
// importiert diese Datei.

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(SCRIPT_DIR, "..");

const APP_SHELL_SRC = "crates/app-shell/src";
const HANDLER_FILE = "crates/app-shell/src/lib.rs";
const STARTUP_GATE_FILE = "crates/app-shell/src/startup_gate.rs";
const FRONTEND_SRC = "apps/smart-ssh-community/frontend/src";

/** Strukturfehler: die Eingabe ist nicht eindeutig lesbar. */
class ParseError extends Error {}

function fail(message) {
  throw new ParseError(message);
}

const IDENT = "[A-Za-z_][A-Za-z0-9_]*";

/** Zeilennummer (1-basiert) einer Position im Text. */
function lineOf(text, index) {
  let line = 1;
  for (let i = 0; i < index && i < text.length; i++) if (text[i] === "\n") line++;
  return line;
}

/** Ersetzt Kommentarzeichen durch Leerzeichen, behält Zeilenumbrüche (damit
 * Zeilennummern stimmen). */
function blank(s) {
  return s.replace(/[^\n]/g, " ");
}

// ---------------------------------------------------------------------------
// Rust
// ---------------------------------------------------------------------------

/** Anfang eines Raw-Strings: Präfix (`r`, `br`, `cr`), `#`-Folge, `"`. */
const RAW_STRING_START = /^([bc]?r)(#*)"/;

/**
 * Entfernt `//`- und (verschachtelte) `/* *\/`-Kommentare aus Rust-Quelltext.
 * Zeichenketten (auch Raw-Strings `r#"…"#`, `br#"…"#`) und Zeichenliterale bleiben
 * erhalten; ein `'` ohne passendes Zeichenliteral ist eine Lifetime. Mit
 * `blankStrings` wird zusätzlich der Inhalt jeder Zeichenkette durch
 * Leerzeichen ersetzt (die Anführungszeichen bleiben), damit ein
 * `#[tauri::command]` in einem String nicht als Definition zählt.
 */
export function stripRustComments(src, { blankStrings = false } = {}) {
  const keep = (s) => (blankStrings ? s[0] + blank(s.slice(1, -1)) + s.slice(-1) : s);
  let out = "";
  let i = 0;
  const n = src.length;
  while (i < n) {
    const c = src[i];
    const next = src[i + 1];
    if (c === "/" && next === "/") {
      let j = src.indexOf("\n", i);
      if (j === -1) j = n;
      out += blank(src.slice(i, j));
      i = j;
    } else if (c === "/" && next === "*") {
      let depth = 1;
      let j = i + 2;
      while (j < n && depth > 0) {
        if (src[j] === "/" && src[j + 1] === "*") {
          depth++;
          j += 2;
        } else if (src[j] === "*" && src[j + 1] === "/") {
          depth--;
          j += 2;
        } else {
          j++;
        }
      }
      out += blank(src.slice(i, j));
      i = j;
    } else if ("rbc".includes(c) && !/[A-Za-z0-9_]/.test(src[i - 1] ?? "") && RAW_STRING_START.test(src.slice(i, i + 260))) {
      // Raw-String `r"…"`, Byte-Raw-String `br"…"`, C-Raw-String `cr"…"` —
      // jeweils mit beliebig vielen `#`. Das Präfix darf nicht Ende eines
      // Bezeichners sein (`xbr"` ist kein Raw-String-Anfang).
      const [whole, , hashes] = RAW_STRING_START.exec(src.slice(i));
      const close = `"${hashes}`;
      const start = i + whole.length;
      let j = src.indexOf(close, start);
      j = j === -1 ? n : j + close.length;
      out += blankStrings ? src.slice(i, start) + blank(src.slice(start, j)) : src.slice(i, j);
      i = j;
    } else if (c === '"') {
      let j = i + 1;
      while (j < n && src[j] !== '"') j += src[j] === "\\" ? 2 : 1;
      j = Math.min(j + 1, n);
      out += keep(src.slice(i, j));
      i = j;
    } else if (c === "'") {
      const m = /^'(?:\\(?:x[0-9a-fA-F]{2}|u\{[0-9a-fA-F]{1,6}\}|.)|[^\\'\n])'/u.exec(src.slice(i, i + 16));
      if (m) {
        out += m[0];
        i += m[0].length;
      } else {
        out += c;
        i++;
      }
    } else {
      out += c;
      i++;
    }
  }
  return out;
}

/** Index hinter der schließenden Klammer zu `text[open]` (`[`, `(` oder `{`). */
function matchingClose(text, open) {
  const pairs = { "[": "]", "(": ")", "{": "}" };
  const stack = [pairs[text[open]]];
  let i = open + 1;
  while (i < text.length && stack.length > 0) {
    const c = text[i];
    if (c === '"') {
      i++;
      while (i < text.length && text[i] !== '"') i += text[i] === "\\" ? 2 : 1;
    } else if (pairs[c]) {
      stack.push(pairs[c]);
    } else if (c === stack[stack.length - 1]) {
      stack.pop();
    } else if (c === "]" || c === ")" || c === "}") {
      return -1;
    }
    i++;
  }
  return stack.length === 0 ? i : -1;
}

/**
 * Issue #139: Die Prüfung erkennt eine Definition nur an der voll
 * qualifizierten Form `#[tauri::command]`. Jede andere Schreibweise würde die
 * `fn` unbemerkt durchlassen und bricht deshalb mit `Datei:Zeile` ab:
 *
 * - ein Attribut, dessen letztes Pfadsegment `command` ist, das aber nicht
 *   genau `tauri::command` lautet (`#[command]`, `#[command(...)]`,
 *   `#[::tauri::command]`);
 * - ein `use`, das `command` aus `tauri` importiert — auch umbenannt
 *   (`use tauri::command as cmd;`), denn `#[cmd]` ließe sich am Attribut
 *   allein nicht erkennen.
 *
 * Erwartet Code ohne Kommentare und mit geleerten Zeichenketten.
 */
function rejectShortCommandForms(code, file) {
  const attr = new RegExp(`#\\s*!?\\s*\\[\\s*((?:::\\s*)?(?:${IDENT}\\s*::\\s*)*)command\\b(?!\\s*::)`, "g");
  let m;
  while ((m = attr.exec(code)) !== null) {
    if (m[1].replace(/\s+/g, "") !== "tauri::") {
      fail(
        `${file}:${lineOf(code, m.index)}: Attribut \`${m[0].replace(/\s+/g, "")}\` — bitte voll qualifiziert \`#[tauri::command]\` schreiben, sonst erkennt die Prüfung die Definition nicht`,
      );
    }
  }
  const use = /\buse\s+(?:::\s*)?tauri\s*::([^;]*)/g;
  while ((m = use.exec(code)) !== null) {
    if (/(?<![A-Za-z0-9_])command(?![A-Za-z0-9_])(?!\s*::)/.test(m[1])) {
      fail(
        `${file}:${lineOf(code, m.index)}: \`use\` importiert \`tauri::command\` — bitte \`#[tauri::command]\` voll qualifiziert schreiben statt das Makro zu importieren`,
      );
    }
  }
}

/**
 * Definierte Kommandos einer Rust-Datei: der Name der `fn`, die auf
 * `#[tauri::command]` (mit oder ohne Argumente) folgt — gegebenenfalls nach
 * weiteren Attributen, mit `pub`/`pub(...)`, `async` usw.
 *
 * @returns {{ name: string, line: number }[]}
 */
export function parseDefinedCommands(src, file = "<rust>") {
  const code = stripRustComments(src, { blankStrings: true });
  rejectShortCommandForms(code, file);
  const result = [];
  const attr = /#\s*\[\s*tauri\s*::\s*command\b/g;
  let m;
  while ((m = attr.exec(code)) !== null) {
    const open = code.indexOf("[", m.index);
    let pos = matchingClose(code, open);
    if (pos === -1) fail(`${file}:${lineOf(code, m.index)}: \`#[tauri::command]\` ohne schließende Klammer`);
    // weitere Attribute zwischen `#[tauri::command]` und der `fn` überspringen
    for (;;) {
      const ws = /^\s*/.exec(code.slice(pos))[0].length;
      if (code[pos + ws] === "#" && /^#\s*!?\s*\[/.test(code.slice(pos + ws))) {
        const nextOpen = code.indexOf("[", pos + ws);
        const close = matchingClose(code, nextOpen);
        if (close === -1) fail(`${file}:${lineOf(code, pos + ws)}: Attribut ohne schließende Klammer`);
        pos = close;
      } else {
        break;
      }
    }
    const fnMatch = new RegExp(
      `^\\s*(?:pub\\s*(?:\\([^)]*\\))?\\s+)?(?:(?:async|unsafe|const|extern(?:\\s+"[^"]*")?)\\s+)*fn\\s+(?:r#)?(${IDENT})\\b`,
    ).exec(code.slice(pos));
    if (!fnMatch) {
      fail(`${file}:${lineOf(code, m.index)}: auf \`#[tauri::command]\` folgt keine erkennbare \`fn\``);
    }
    result.push({ name: fnMatch[1], line: lineOf(code, m.index) });
    attr.lastIndex = pos;
  }
  return result;
}

/**
 * Registrierte Kommandos: letztes Pfadsegment jedes Eintrags im **einzigen**
 * `generate_handler!`-Aufruf. Kommentare zählen nicht.
 *
 * @param {{ file: string, text: string }[]} rustFiles alle Dateien, in denen
 *   ein `generate_handler!` stehen könnte (um ein zweites zu erkennen)
 * @param {string} handlerFile die Datei, in der der Aufruf stehen muss
 * @returns {{ name: string, path: string, line: number }[]}
 */
export function parseRegisteredCommands(rustFiles, handlerFile = HANDLER_FILE) {
  const hits = [];
  for (const { file, text } of rustFiles) {
    const code = stripRustComments(text, { blankStrings: true });
    const re = /\bgenerate_handler\s*!/g;
    let m;
    while ((m = re.exec(code)) !== null) hits.push({ file, code, index: m.index, end: re.lastIndex });
  }
  if (hits.length === 0) fail(`kein \`generate_handler!\` gefunden (erwartet in ${handlerFile})`);
  if (hits.length > 1) {
    const where = hits.map((h) => `${h.file}:${lineOf(h.code, h.index)}`).join(", ");
    fail(`mehr als ein \`generate_handler!\` gefunden (${where}) — die Prüfung kennt nur eine Registrierungsliste`);
  }
  const [hit] = hits;
  if (hit.file !== handlerFile) {
    fail(`\`generate_handler!\` steht in ${hit.file}, erwartet in ${handlerFile}`);
  }
  const open = hit.end + /^\s*/.exec(hit.code.slice(hit.end))[0].length;
  if (!"[({".includes(hit.code[open] ?? "x")) {
    fail(`${hit.file}:${lineOf(hit.code, hit.index)}: \`generate_handler!\` ohne Klammer`);
  }
  const close = matchingClose(hit.code, open);
  if (close === -1) fail(`${hit.file}:${lineOf(hit.code, hit.index)}: \`generate_handler!\` ohne schließende Klammer`);
  const body = hit.code.slice(open + 1, close - 1);
  const result = [];
  let offset = open + 1;
  for (const raw of body.split(",")) {
    const entry = raw.trim();
    const entryIndex = offset + raw.indexOf(entry);
    offset += raw.length + 1;
    if (entry === "") continue;
    const pathRe = new RegExp(`^(?:${IDENT}\\s*::\\s*)*(?:r#)?(${IDENT})$`);
    const pm = pathRe.exec(entry);
    if (!pm) {
      fail(`${hit.file}:${lineOf(hit.code, entryIndex)}: Eintrag in \`generate_handler!\` nicht lesbar: ${JSON.stringify(entry)}`);
    }
    result.push({ name: pm[1], path: entry.replace(/\s+/g, ""), line: lineOf(hit.code, entryIndex) });
  }
  return result;
}

/** Namen in `ALLOWED_WHILE_LOCKED` (Positivliste des Starttors). */
export function parseAllowedWhileLocked(src, file = STARTUP_GATE_FILE) {
  const code = stripRustComments(src);
  const m = /\bconst\s+ALLOWED_WHILE_LOCKED\s*:[^=]*=\s*&\s*\[/.exec(code);
  if (!m) fail(`${file}: \`const ALLOWED_WHILE_LOCKED: … = &[…]\` nicht gefunden`);
  const open = m.index + m[0].length - 1;
  const close = matchingClose(code, open);
  if (close === -1) fail(`${file}: \`ALLOWED_WHILE_LOCKED\` ohne schließende Klammer`);
  const body = code.slice(open + 1, close - 1);
  const result = [];
  for (const raw of body.split(",")) {
    const entry = raw.trim();
    if (entry === "") continue;
    const sm = /^"([^"\\]*)"$/.exec(entry);
    if (!sm) fail(`${file}: Eintrag in \`ALLOWED_WHILE_LOCKED\` ist kein einfaches Zeichenkettenliteral: ${JSON.stringify(entry)}`);
    result.push(sm[1]);
  }
  return result;
}

// ---------------------------------------------------------------------------
// Frontend (TypeScript/JavaScript)
// ---------------------------------------------------------------------------

// Nach diesen Zeichen beginnt ein `/` ein Regex-Literal, kein Divisionszeichen.
const REGEX_PRECEDERS = new Set([..."(,=:[!&|?{};+-*%<>~^"]);

/**
 * Entfernt `//`- und `/* *\/`-Kommentare aus TS/JS-Quelltext. Zeichenketten,
 * Template-Literale (inkl. `${…}`) und Regex-Literale bleiben erhalten.
 * Gewöhnliche Zeichenketten enden spätestens am Zeilenende, damit ein
 * Apostroph in JSX-Text höchstens eine Zeile verschluckt.
 */
export function stripJsComments(src) {
  let out = "";
  let i = 0;
  const n = src.length;
  let lastSig = ""; // letztes Nicht-Leerzeichen im Code
  const templateDepth = []; // offene `${`: Klammertiefe je Ebene
  while (i < n) {
    const c = src[i];
    const next = src[i + 1];
    if (c === "/" && next === "/") {
      let j = src.indexOf("\n", i);
      if (j === -1) j = n;
      out += blank(src.slice(i, j));
      i = j;
      continue;
    }
    if (c === "/" && next === "*") {
      let j = src.indexOf("*/", i + 2);
      j = j === -1 ? n : j + 2;
      out += blank(src.slice(i, j));
      i = j;
      continue;
    }
    if (c === "/" && (lastSig === "" || REGEX_PRECEDERS.has(lastSig) || /\breturn\s*$/.test(out))) {
      let j = i + 1;
      let inClass = false;
      while (j < n && src[j] !== "\n") {
        if (src[j] === "\\") {
          j += 2;
          continue;
        }
        if (src[j] === "[") inClass = true;
        else if (src[j] === "]") inClass = false;
        else if (src[j] === "/" && !inClass) break;
        j++;
      }
      j = Math.min(j + 1, n);
      out += src.slice(i, j);
      lastSig = "/";
      i = j;
      continue;
    }
    if (c === '"' || c === "'") {
      let j = i + 1;
      while (j < n && src[j] !== c && src[j] !== "\n") j += src[j] === "\\" ? 2 : 1;
      j = Math.min(j + 1, n);
      out += src.slice(i, j);
      lastSig = c;
      i = j;
      continue;
    }
    if (c === "`" || (c === "}" && templateDepth.length > 0 && templateDepth[templateDepth.length - 1] === 0)) {
      if (c === "}") templateDepth.pop();
      // Template-Text bis zum schließenden ` oder zum nächsten `${`
      let j = i + 1;
      while (j < n && src[j] !== "`" && !(src[j] === "$" && src[j + 1] === "{")) j += src[j] === "\\" ? 2 : 1;
      if (src[j] === "$") {
        templateDepth.push(0);
        j += 2;
      } else {
        j = Math.min(j + 1, n);
      }
      out += src.slice(i, j);
      lastSig = "`";
      i = j;
      continue;
    }
    if (templateDepth.length > 0) {
      if (c === "{") templateDepth[templateDepth.length - 1]++;
      else if (c === "}") templateDepth[templateDepth.length - 1]--;
    }
    out += c;
    if (!/\s/.test(c)) lastSig = c;
    i++;
  }
  return out;
}

/**
 * Aufgerufene Kommandos einer TS/JS-Datei: Zeichenkettenliterale als erstes
 * Argument von `invoke(...)` bzw. `invoke<...>(...)`. Ein nicht-literales
 * erstes Argument (z. B. der Wrapper in `api.ts`, der `cmd` durchreicht) und
 * Plugin-Kommandos (`plugin:...`) zählen nicht.
 *
 * @returns {{ name: string, line: number }[]}
 */
export function parseInvokedCommands(src) {
  const code = stripJsComments(src);
  const result = [];
  const re = /(?<![A-Za-z0-9_$])invoke\b/g;
  let m;
  while ((m = re.exec(code)) !== null) {
    let i = re.lastIndex;
    const skipWs = () => {
      while (i < code.length && /\s/.test(code[i])) i++;
    };
    skipWs();
    if (code[i] === "<") {
      // Typargumente überspringen; `=>` innerhalb zählt nicht als Schließer
      let depth = 0;
      for (; i < code.length; i++) {
        if (code[i] === "<") depth++;
        else if (code[i] === ">" && code[i - 1] !== "=") {
          depth--;
          if (depth === 0) {
            i++;
            break;
          }
        } else if (code[i] === ";") {
          break;
        }
      }
      skipWs();
    }
    if (code[i] !== "(") continue;
    i++;
    skipWs();
    const lit = /^(["'`])([^"'`\\$\n]*)\1/.exec(code.slice(i));
    if (!lit) continue;
    const name = lit[2];
    if (name.startsWith("plugin:")) continue;
    result.push({ name, line: lineOf(code, m.index) });
  }
  return result;
}

/** Testdateien des Frontends, die nicht mitzählen. */
export function isFrontendTestFile(relPath) {
  const base = path.basename(relPath);
  return (
    /\.(test|spec)\.[cm]?[jt]sx?$/.test(base) ||
    relPath.split(/[\\/]/).includes("__tests__") ||
    /^test-setup\.[cm]?[jt]sx?$/.test(base)
  );
}

// ---------------------------------------------------------------------------
// Abgleich
// ---------------------------------------------------------------------------

/**
 * @param {{
 *   defined: { name: string, file: string, line: number }[],
 *   registered: { name: string, path: string, file: string, line: number }[],
 *   invoked: { name: string, file: string, line: number }[],
 *   allowedWhileLocked?: string[],
 * }} input
 * @returns {{ kind: string, name: string, where: string }[]}
 */
export function findMismatches({ defined, registered, invoked, allowedWhileLocked = [] }) {
  const problems = [];
  const at = (e) => `${e.file}:${e.line}`;

  const definedByName = new Map();
  for (const d of defined) {
    if (definedByName.has(d.name)) {
      problems.push({
        kind: "mehrfach definiert",
        name: d.name,
        where: `${at(definedByName.get(d.name))} und ${at(d)}`,
      });
    } else {
      definedByName.set(d.name, d);
    }
  }
  const registeredByName = new Map();
  for (const r of registered) {
    if (registeredByName.has(r.name)) {
      problems.push({
        kind: "mehrfach registriert",
        name: r.name,
        where: `${at(registeredByName.get(r.name))} und ${at(r)}`,
      });
    } else {
      registeredByName.set(r.name, r);
    }
  }
  for (const d of definedByName.values()) {
    if (!registeredByName.has(d.name)) {
      problems.push({ kind: "definiert, aber nicht in generate_handler! registriert", name: d.name, where: at(d) });
    }
  }
  for (const r of registeredByName.values()) {
    if (!definedByName.has(r.name)) {
      problems.push({ kind: "registriert, aber kein #[tauri::command] definiert", name: r.name, where: `${at(r)} (${r.path})` });
    }
  }
  for (const v of invoked) {
    if (!registeredByName.has(v.name)) {
      problems.push({ kind: "im Frontend aufgerufen, aber nicht registriert", name: v.name, where: at(v) });
    }
  }
  for (const name of allowedWhileLocked) {
    if (!registeredByName.has(name)) {
      problems.push({ kind: "in ALLOWED_WHILE_LOCKED, aber nicht registriert", name, where: STARTUP_GATE_FILE });
    }
  }
  return problems;
}

function walk(dir, filter) {
  const out = [];
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) out.push(...walk(full, filter));
    else if (filter(entry.name)) out.push(full);
  }
  return out.sort();
}

function rel(root, full) {
  return path.relative(root, full).split(path.sep).join("/");
}

/** Liest das Repo unter `root` und liefert alle Abweichungen. */
export function checkRepo(root = REPO_ROOT) {
  const rustFiles = walk(path.join(root, APP_SHELL_SRC), (n) => n.endsWith(".rs")).map((full) => ({
    file: rel(root, full),
    text: fs.readFileSync(full, "utf8"),
  }));
  const defined = rustFiles.flatMap(({ file, text }) =>
    parseDefinedCommands(text, file).map((d) => ({ ...d, file })),
  );
  const registered = parseRegisteredCommands(rustFiles).map((r) => ({ ...r, file: HANDLER_FILE }));
  const allowedWhileLocked = parseAllowedWhileLocked(
    fs.readFileSync(path.join(root, STARTUP_GATE_FILE), "utf8"),
  );
  const frontendFiles = walk(path.join(root, FRONTEND_SRC), (n) => /\.[cm]?[jt]sx?$/.test(n) && !n.endsWith(".d.ts"))
    .map((full) => rel(root, full))
    .filter((file) => !isFrontendTestFile(file));
  const invoked = frontendFiles.flatMap((file) =>
    parseInvokedCommands(fs.readFileSync(path.join(root, file), "utf8")).map((v) => ({ ...v, file })),
  );
  return {
    problems: findMismatches({ defined, registered, invoked, allowedWhileLocked }),
    counts: { defined: defined.length, registered: registered.length, invoked: new Set(invoked.map((v) => v.name)).size },
  };
}

/** `--root <dir>`: anderes Repo-Wurzelverzeichnis (für Tests). */
function parseRoot(argv) {
  const i = argv.indexOf("--root");
  if (i === -1) return REPO_ROOT;
  if (!argv[i + 1]) fail("`--root` ohne Verzeichnis");
  return path.resolve(argv[i + 1]);
}

function main() {
  try {
    const { problems, counts } = checkRepo(parseRoot(process.argv.slice(2)));
    if (problems.length > 0) {
      console.error(`Tauri-Kommandos passen nicht zusammen (${problems.length} Abweichung(en)):`);
      for (const p of problems) console.error(`  - ${p.name}: ${p.kind} [${p.where}]`);
      process.exitCode = 1;
      return;
    }
    console.log(
      `Tauri-Kommandos ok: ${counts.defined} definiert, ${counts.registered} registriert, ${counts.invoked} vom Frontend aufgerufen.`,
    );
  } catch (err) {
    const message = err instanceof ParseError ? err.message : `${err.stack ?? err}`;
    console.error(`Prüfung der Tauri-Kommandos fehlgeschlagen: ${message}`);
    process.exitCode = 1;
  }
}

// Nur ausführen, wenn direkt aufgerufen (nicht beim Import durch einen Test).
if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main();
}
