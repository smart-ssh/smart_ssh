// Documentation path guard (issue #874).
//
// Reports backticked repository paths and quoted section references in the
// guidance documents that no longer exist.
//
// Heuristic (keep it simple; false positives go into the allowlist, not into
// special cases here):
//  * Path rule: a backticked token is a repository path when it contains a `/`
//    or ends in a known extension (.md .rs .ts .tsx .mjs .json .toml .yml
//    .yaml .sh .kt .sql), has no spaces and no placeholder characters
//    (`<` `>` `*` `{` `}` `NNNN` `X.Y.Z` `…`). It must exist relative to the
//    repository root, a configured extra base directory (default: the frontend
//    root and its `src/`) or the document's own directory. A trailing
//    `:<line>` / `:<line>-<line>` is stripped first. A bare file name without
//    any `/` is resolved like any other path (no search by file name). URLs and tokens starting
//    with `~`, `/`, `$` or `-` are ignored.
//  * Section rule: after a backticked `FILE.md`, a `section "Title"` /
//    `Abschnitt „Title“` on the same line (or directly `FILE.md`, „Title“), or `FILE.md#anchor` inside the
//    backticks, requires a heading with that title (case-insensitive) or
//    anchor (GitHub slug) in that file.
//  * Fenced code blocks are skipped.
//  * Allowlist (`{ "<doc path>": [{ "token": "...", "reason": "..." }] }`):
//    tokens that are intentionally not paths. Every entry needs a `reason`.
//
// Usage: node check-doc-paths.mjs [--root <dir>] [--config <json>]
// Config JSON: { "documents": [globs], "baseDirs": [dirs], "allowlist": "<json path>" }
// (relative to the root; every key optional).
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

export const KNOWN_EXTENSIONS = [".md", ".rs", ".ts", ".tsx", ".mjs", ".json", ".toml", ".yml", ".yaml", ".sh", ".kt", ".sql"];

export const DEFAULT_CONFIG = {
  documents: [
    "AGENTS.md",
    "CLAUDE.md",
    "README_DEV.md",
    ".claude/**/*.md",
    "docs/*.md",
    "docs/recipes/*.md",
    "docs/specs/README.md",
    "docs/adr/README.md",
    "changelog.d/README.md",
  ],
  baseDirs: [
    "apps/smart-ssh-community/frontend",
    "apps/smart-ssh-community/frontend/src",
  ],
  allowlist: "scripts/doc-paths-allowlist.json",
};

const SKIP_DIRS = new Set([".git", "node_modules", "target"]);
const PLACEHOLDER = /[<>*{}…]|NNNN|X\.Y\.Z/;

function globToRegExp(glob) {
  let re = "";
  for (let i = 0; i < glob.length; i++) {
    const c = glob[i];
    if (c === "*" && glob[i + 1] === "*") {
      re += glob[i + 2] === "/" ? "(?:.*/)?" : ".*";
      i += glob[i + 2] === "/" ? 2 : 1;
    } else if (c === "*") re += "[^/]*";
    else re += c.replace(/[.+?^${}()|[\]\\]/g, "\\$&");
  }
  return new RegExp(`^${re}$`);
}

function walk(root, rel = "") {
  const out = [];
  for (const e of fs.readdirSync(path.join(root, rel), { withFileTypes: true })) {
    if (SKIP_DIRS.has(e.name)) continue;
    const r = rel ? `${rel}/${e.name}` : e.name;
    if (e.isDirectory()) out.push(...walk(root, r));
    else out.push(r);
  }
  return out;
}

/** Lines of a markdown text outside fenced code blocks: [{ n, text }]. */
function proseLines(text) {
  const out = [];
  let fence = null;
  text.split(/\r?\n/).forEach((line, i) => {
    const m = /^\s*(`{3,}|~{3,})/.exec(line);
    if (m) {
      if (!fence) fence = m[1][0];
      else if (m[1][0] === fence) fence = null;
      return;
    }
    if (!fence) out.push({ n: i + 1, text: line });
  });
  return out;
}

function slug(heading) {
  return heading
    .toLowerCase()
    .replace(/[^\p{L}\p{N}\s_-]/gu, "")
    .trim()
    .replace(/\s/g, "-");
}

function plainHeading(h) {
  return h
    .replace(/\s+#+\s*$/, "")
    .replace(/\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/[`*_]/g, "")
    .trim()
    .toLowerCase();
}

function headingsOf(file) {
  const titles = new Set();
  const anchors = new Set();
  for (const { text } of proseLines(fs.readFileSync(file, "utf8"))) {
    const m = /^#{1,6}\s+(.*)$/.exec(text);
    if (!m) continue;
    titles.add(plainHeading(m[1]));
    anchors.add(slug(m[1].replace(/[`*]/g, "")));
  }
  return { titles, anchors };
}

function loadAllowlist(file) {
  if (!file || !fs.existsSync(file)) return { entries: {}, problems: [] };
  const entries = JSON.parse(fs.readFileSync(file, "utf8"));
  const problems = [];
  for (const [doc, list] of Object.entries(entries)) {
    for (const e of list) {
      if (typeof e?.token !== "string" || typeof e?.reason !== "string" || e.reason.trim() === "") {
        problems.push(`allowlist entry for ${doc} needs a "token" and a non-empty "reason": ${JSON.stringify(e)}`);
      }
    }
  }
  return { entries, problems };
}

/** Decide whether a backticked token is path-like; returns the path (suffix stripped) or null. */
function pathCandidate(token) {
  if (/\s/.test(token) || PLACEHOLDER.test(token)) return null;
  if (/^(https?:|[~/$-])/.test(token)) return null;
  const t = token.replace(/:\d+(?:-\d+)?$/, "");
  if (!t.includes("/") && !KNOWN_EXTENSIONS.some((e) => t.split("#")[0].endsWith(e))) return null;
  return t;
}

export function checkDocPaths(root = REPO_ROOT, config = DEFAULT_CONFIG) {
  const cfg = { ...DEFAULT_CONFIG, ...config };
  const problems = [];
  const { entries, problems: allowProblems } = loadAllowlist(path.resolve(root, cfg.allowlist));
  problems.push(...allowProblems);
  const matchers = cfg.documents.map(globToRegExp);
  const docs = walk(root).filter((f) => matchers.some((m) => m.test(f))).sort();
  const headingCache = new Map();
  const headings = (f) => {
    if (!headingCache.has(f)) headingCache.set(f, headingsOf(f));
    return headingCache.get(f);
  };

  for (const doc of docs) {
    const allowed = new Set((entries[doc] ?? []).map((e) => e.token));
    const docDir = path.dirname(path.join(root, doc));
    const bases = [root, ...cfg.baseDirs.map((d) => path.join(root, d)), docDir];
    const resolve = (p) => bases.map((b) => path.join(b, p)).find((c) => fs.existsSync(c));
    for (const { n, text } of proseLines(fs.readFileSync(path.join(root, doc), "utf8"))) {
      const re = /`([^`\n]+)`/g;
      let m;
      while ((m = re.exec(text))) {
        const token = m[1];
        if (allowed.has(token)) continue;
        const cand = pathCandidate(token);
        if (!cand) continue;
        const [filePart, anchor] = cand.split("#");
        const found = resolve(filePart);
        if (!found) {
          problems.push(`${doc}:${n}: \`${token}\` does not exist (repo root, frontend root or document directory)`);
          continue;
        }
        if (!filePart.endsWith(".md") || !fs.existsSync(found) || !fs.statSync(found).isFile()) continue;
        if (anchor && !headings(found).anchors.has(anchor.toLowerCase())) {
          problems.push(`${doc}:${n}: \`${token}\`: no heading with anchor #${anchor}`);
        }
        const after = text.slice(m.index + m[0].length);
        const s =
          /^[^`"„“.;]{0,60}?(?:section|Abschnitt)\s+["„“]([^"“”]+)["“”]/i.exec(after) ??
          /^\s*[,(]\s*["„“]([^"“”]+)["“”]/.exec(after);
        if (s && !headings(found).titles.has(s[1].trim().toLowerCase())) {
          problems.push(`${doc}:${n}: \`${filePart}\` has no section titled "${s[1]}"`);
        }
      }
    }
  }
  return problems;
}

function main() {
  try {
    const args = process.argv.slice(2);
    const opt = (name) => {
      const i = args.indexOf(name);
      return i >= 0 ? args[i + 1] : undefined;
    };
    const root = path.resolve(opt("--root") ?? REPO_ROOT);
    const cfgPath = opt("--config");
    const config = cfgPath ? JSON.parse(fs.readFileSync(path.resolve(root, cfgPath), "utf8")) : DEFAULT_CONFIG;
    const problems = checkDocPaths(root, config);
    if (problems.length > 0) {
      console.error(`Doc path check failed (${problems.length} problem(s)):`);
      for (const p of problems) console.error(`  - ${p}`);
      process.exitCode = 1;
      return;
    }
    console.log("Doc paths ok.");
  } catch (err) {
    console.error(`Doc path check failed to run: ${err.stack ?? err}`);
    process.exitCode = 1;
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main();
}
