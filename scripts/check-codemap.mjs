// Issue #867: `docs/codemap.md` names files and symbols; this check keeps them real.
// Notation (also documented at the top of the map): a backticked token with a `/`
// and no spaces/wildcards is a repo path and must exist. A backticked identifier
// (optionally `Type::name` or `name()`) in the same table row or list item AFTER a
// path is a symbol of that path: the file must define it. Fenced code blocks are
// ignored. Usage: node scripts/check-codemap.mjs [--file <map.md>] [--root <dir>]
// Text based, no dependencies.

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(SCRIPT_DIR, "..");

const TOKEN_RE = /`([^`\n]+)`/g;
const SYMBOL_RE = /^[A-Za-z_][A-Za-z0-9_]*(?:::[A-Za-z_][A-Za-z0-9_]*)*(?:\(\))?!?$/;
const CODE_EXT = /\.(rs|ts|tsx|mts|mjs|js)$/;

const escapeRe = (s) => s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

function isPathToken(t) {
  return t.includes("/") && !/[\s*{}<>|]/.test(t);
}

/** Entries `{ line, path, symbols }`; paths without symbols have an empty list. */
export function parseCodemap(markdown) {
  const entries = [];
  let fenced = false;
  markdown.replace(/\r\n/g, "\n").split("\n").forEach((text, i) => {
    if (/^\s*(```|~~~)/.test(text)) {
      fenced = !fenced;
      return;
    }
    if (fenced) return;
    let current = null;
    for (const m of text.matchAll(TOKEN_RE)) {
      const token = m[1].trim();
      if (isPathToken(token)) {
        current = { line: i + 1, path: token, symbols: [] };
        entries.push(current);
      } else if (current && SYMBOL_RE.test(token)) {
        current.symbols.push(token.replace(/\(\)$/, "").replace(/!$/, ""));
      }
    }
  });
  return entries;
}

function definitionRegex(file, name) {
  const n = escapeRe(name);
  if (/\.rs$/.test(file)) {
    return new RegExp(`\\b(?:fn|struct|enum|trait|type|const|static|mod)\\s+${n}\\b|\\bmacro_rules!\\s+${n}\\b`);
  }
  return new RegExp(`\\b(?:function\\*?|const|let|class|interface|type|enum)\\s+${n}\\b`);
}

function filesUnder(dir) {
  const out = [];
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    if (e.name === "node_modules" || e.name === "target" || e.name.startsWith(".")) continue;
    const p = path.join(dir, e.name);
    if (e.isDirectory()) out.push(...filesUnder(p));
    else if (CODE_EXT.test(e.name)) out.push(p);
  }
  return out;
}

/** Problems as `{ line, message }`. */
export function findCodemapProblems(markdown, root = REPO_ROOT) {
  const problems = [];
  for (const e of parseCodemap(markdown)) {
    const rel = e.path.replace(/\/$/, "");
    const abs = path.resolve(root, rel);
    if (!abs.startsWith(path.resolve(root) + path.sep) || !fs.existsSync(abs)) {
      problems.push({ line: e.line, message: `path does not exist: ${e.path}` });
      continue;
    }
    if (e.symbols.length === 0) continue;
    const isDir = fs.statSync(abs).isDirectory();
    const files = isDir ? filesUnder(abs) : [abs];
    const texts = files.map((f) => ({ f, text: fs.readFileSync(f, "utf8") }));
    for (const sym of e.symbols) {
      const name = sym.split("::").pop();
      const found = texts.some(({ f, text }) =>
        CODE_EXT.test(f) ? definitionRegex(f, name).test(text) : new RegExp(`\\b${escapeRe(name)}\\b`).test(text),
      );
      if (!found && !(!isDir && !CODE_EXT.test(abs) && new RegExp(`\\b${escapeRe(name)}\\b`).test(texts[0].text))) {
        problems.push({ line: e.line, message: `no definition of \`${sym}\` in ${e.path}` });
      }
    }
  }
  return problems;
}

/** Check a map file against a repo root. */
export function checkCodemap({ file = path.join(REPO_ROOT, "docs", "codemap.md"), root = REPO_ROOT } = {}) {
  return findCodemapProblems(fs.readFileSync(file, "utf8"), root);
}

function main() {
  const args = process.argv.slice(2);
  const opts = {};
  for (const flag of ["--file", "--root"]) {
    const at = args.indexOf(flag);
    if (at >= 0) {
      if (!args[at + 1]) {
        console.error(`${flag} needs a value`);
        process.exitCode = 2;
        return;
      }
      opts[flag.slice(2)] = path.resolve(args[at + 1]);
    }
  }
  const problems = checkCodemap(opts);
  for (const p of problems) console.error(`codemap.md:${p.line}: ${p.message}`);
  if (problems.length > 0) process.exitCode = 1;
  else console.log("Codemap check passed.");
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main();
}
