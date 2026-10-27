// Issue #841: file-size ratchet. Source files may not grow past their recorded
// size, and new files stay below a fixed limit.
//
// Counted: tracked `*.rs`, `*.ts`, `*.tsx`, `*.mjs`, `*.kt` files of the root's
// own git repository (`git ls-files`; a submodule is a single gitlink entry, so
// its files are never counted), except `node_modules/`, `target/`, `dist/`,
// `gen/android/**/build/` and `*.d.ts` / `*.d.mts`.
//
// Limits: production files 1,200 lines, test files 2,000 lines. A file above
// its limit needs an entry in the allowlist (`scripts/file-size-allowlist.json`,
// `{ "<path>": <max lines> }`), and its size must not exceed that entry.
//
// Usage:
//   node scripts/check-file-size.mjs [--root <dir>] [--allowlist <path>]
//                                    [--update] [--base <git-ref>]
//   --update   lowers entries to the current size and removes entries of files
//              now within the limit or deleted; never raises or adds an entry.
//   --base     additionally fails if an entry is higher than at <git-ref> or
//              a new entry was added.
//
// No shebang, because a frontend test imports this file.

import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(SCRIPT_DIR, "..");
const DEFAULT_ALLOWLIST = path.join(SCRIPT_DIR, "file-size-allowlist.json");

export const PRODUCTION_LIMIT = 1200;
export const TEST_LIMIT = 2000;

const EXTENSIONS = [".rs", ".ts", ".tsx", ".mjs", ".kt"];
const RUST_TEST_NAMES = new Set([
  "tests.rs",
  "test_support.rs",
  "test_events.rs",
  "test_keys.rs",
  "test_server.rs",
]);

/** Whether a repo-relative path (forward slashes) is counted at all. */
export function isCounted(rel) {
  if (!EXTENSIONS.some((e) => rel.endsWith(e))) return false;
  if (rel.endsWith(".d.ts") || rel.endsWith(".d.mts")) return false;
  const segs = rel.split("/");
  if (segs.some((s) => s === "node_modules" || s === "target" || s === "dist")) return false;
  if (/(^|\/)gen\/android\/(.*\/)?build\//.test(rel)) return false;
  return true;
}

/** Whether a counted file is a test file (limit 2,000) rather than production. */
export function isTestFile(rel) {
  const segs = rel.split("/");
  const base = segs[segs.length - 1];
  if (/\.(test|spec)\.tsx?$/.test(base)) return true;
  if (segs.includes("e2e")) return true;
  if (base.endsWith(".rs")) {
    if (segs.slice(0, -1).includes("tests")) return true;
    if (RUST_TEST_NAMES.has(base)) return true;
    if (base.startsWith("tests_") || base.endsWith("_tests.rs")) return true;
  }
  return false;
}

export function limitFor(rel) {
  return isTestFile(rel) ? TEST_LIMIT : PRODUCTION_LIMIT;
}

/** Number of lines (a final line without newline counts). */
export function countLines(text) {
  if (text.length === 0) return 0;
  let n = 0;
  for (let i = 0; i < text.length; i++) if (text.charCodeAt(i) === 10) n++;
  return text.charCodeAt(text.length - 1) === 10 ? n : n + 1;
}

function git(root, args) {
  return execFileSync("git", args, { cwd: root, encoding: "utf8", maxBuffer: 1 << 28 });
}

/** Line counts of all counted, tracked files: Map<relPath, lines>. */
export function collectSizes(root) {
  let out;
  try {
    out = git(root, ["ls-files", "-s", "-z"]);
  } catch (e) {
    throw new Error(`cannot list tracked files in ${root}: ${e.message}`);
  }
  const sizes = new Map();
  for (const entry of out.split("\0")) {
    if (!entry) continue;
    const tab = entry.indexOf("\t");
    const mode = entry.slice(0, entry.indexOf(" "));
    const rel = entry.slice(tab + 1);
    if (mode === "160000" || !isCounted(rel)) continue; // gitlink = submodule
    const full = path.join(root, rel);
    let st;
    try {
      st = fs.lstatSync(full);
    } catch {
      continue; // tracked but deleted in the work tree
    }
    if (!st.isFile()) continue;
    sizes.set(rel, countLines(fs.readFileSync(full, "utf8")));
  }
  return sizes;
}

export function readAllowlist(file) {
  const data = JSON.parse(fs.readFileSync(file, "utf8"));
  if (data === null || typeof data !== "object" || Array.isArray(data)) {
    throw new Error(`${file}: allowlist must be a JSON object`);
  }
  for (const [k, v] of Object.entries(data)) {
    if (!Number.isInteger(v) || v < 0) throw new Error(`${file}: entry ${k} must be a line count`);
  }
  return data;
}

export function sortedObject(obj) {
  const out = {};
  for (const k of Object.keys(obj).sort((a, b) => (a < b ? -1 : a > b ? 1 : 0))) out[k] = obj[k];
  return out;
}

/** Problems of sizes vs. allowlist (without --base). */
export function findProblems(sizes, allowlist) {
  const problems = [];
  for (const [rel, lines] of sizes) {
    const limit = limitFor(rel);
    if (lines <= limit) continue;
    const entry = allowlist[rel];
    if (entry === undefined) {
      problems.push(
        `${rel}: ${lines} lines, limit ${limit} and no allowlist entry — split the file`,
      );
    } else if (lines > entry) {
      problems.push(
        `${rel}: ${lines} lines, allowlist entry ${entry} — split the file (do not raise the entry)`,
      );
    }
  }
  for (const [rel, entry] of Object.entries(allowlist)) {
    const lines = sizes.get(rel);
    if (lines === undefined) {
      problems.push(`${rel}: allowlist entry ${entry} but the file does not exist — stale entry, run --update`);
    } else if (lines <= limitFor(rel)) {
      problems.push(
        `${rel}: ${lines} lines, within limit ${limitFor(rel)} but has allowlist entry ${entry} — stale entry, run --update`,
      );
    }
  }
  return problems;
}

/** New allowlist for --update: lowers, removes, never raises or adds. */
export function updateAllowlist(sizes, allowlist) {
  const next = {};
  for (const [rel, entry] of Object.entries(allowlist)) {
    const lines = sizes.get(rel);
    if (lines === undefined || lines <= limitFor(rel)) continue;
    next[rel] = Math.min(entry, lines);
  }
  return sortedObject(next);
}

/** Problems of the allowlist compared with the one at the base ref. */
export function compareWithBase(allowlist, baseList) {
  const problems = [];
  for (const [rel, entry] of Object.entries(allowlist)) {
    const before = baseList[rel];
    if (before === undefined) {
      problems.push(`${rel}: new allowlist entry (${entry}) — split the file instead of adding an entry`);
    } else if (entry > before) {
      problems.push(`${rel}: allowlist entry raised from ${before} to ${entry} — split the file instead`);
    }
  }
  return problems;
}

function readBaseAllowlist(root, allowlistPath, ref) {
  const rel = path.relative(root, allowlistPath).split(path.sep).join("/");
  if (rel.startsWith("..") || path.isAbsolute(rel)) {
    throw new Error(`--base needs the allowlist inside the root (${allowlistPath})`);
  }
  let text;
  try {
    text = git(root, ["show", `${ref}:${rel}`]);
  } catch {
    // Verify the ref itself exists, so a typo is not mistaken for "no allowlist yet".
    git(root, ["rev-parse", "--verify", `${ref}^{commit}`]);
    return null; // allowlist does not exist at the base yet (introducing PR)
  }
  return JSON.parse(text);
}

export function parseArgs(argv) {
  const opts = { root: REPO_ROOT, allowlist: undefined, update: false, base: undefined };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    const val = () => {
      if (i + 1 >= argv.length) throw new Error(`${a} needs a value`);
      return argv[++i];
    };
    if (a === "--root") opts.root = path.resolve(val());
    else if (a === "--allowlist") opts.allowlist = path.resolve(val());
    else if (a === "--base") opts.base = val();
    else if (a === "--update") opts.update = true;
    else throw new Error(`unknown argument ${a}`);
  }
  opts.allowlist ??= DEFAULT_ALLOWLIST;
  return opts;
}

/** Runs the check; returns the exit code. */
export function main(argv, out = console) {
  let opts;
  try {
    opts = parseArgs(argv);
    const sizes = collectSizes(opts.root);
    let allowlist = readAllowlist(opts.allowlist);
    if (opts.update) {
      allowlist = updateAllowlist(sizes, allowlist);
      fs.writeFileSync(opts.allowlist, JSON.stringify(allowlist, null, 2) + "\n");
      out.log(`updated ${opts.allowlist}`);
    }
    const problems = findProblems(sizes, allowlist);
    if (opts.base !== undefined) {
      const baseList = readBaseAllowlist(opts.root, opts.allowlist, opts.base);
      if (baseList === null) out.log(`note: no allowlist at ${opts.base}, skipping base comparison`);
      else problems.push(...compareWithBase(allowlist, baseList));
    }
    if (problems.length > 0) {
      out.error(`check-file-size: ${problems.length} problem(s)`);
      for (const p of problems) out.error(`  ${p}`);
      return 1;
    }
    out.log(`check-file-size: ok (${sizes.size} files, ${Object.keys(allowlist).length} allowlisted)`);
    return 0;
  } catch (e) {
    out.error(`check-file-size: ${e.message}`);
    return 2;
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  process.exitCode = main(process.argv.slice(2));
}
