// Issue #918: keeps the backend→frontend event names in sync.
//
//   1. Rust     — `pub const X: &str = "…";` items in
//                 `crates/app-logic/src/event_names.rs`: every event the core emits.
//   2. Frontend — `export const EVENT = { X: "…", … } as const` in
//                 `apps/smart-ssh-community/frontend/src/api/eventNames.ts`: the
//                 events the core UI listens to.
//   3. Listeners — `listen(` / `once(` / `payloadListener(` in non-test
//                 frontend files must take a constant, not a string literal.
//
// Fails when (a) a frontend constant has no Rust constant of the same name and
// value, (b) a listener uses a string literal, (c) a Rust constant is missing
// from the frontend list and not in NOT_CONSUMED_BY_CORE_UI (below).
// Payload types are not checked. Text based, no dependencies, like
// `check-tauri-commands.mjs`.
//
// Usage: `npm run check-events` in the frontend (part of `npm run lint`).
// No shebang, a frontend test imports this file.

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { isFrontendTestFile, stripJsComments, stripRustComments } from "./check-tauri-commands.mjs";

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(SCRIPT_DIR, "..");

const RUST_FILE = "crates/app-logic/src/event_names.rs";
const EVENT_NAMES_FILE = "apps/smart-ssh-community/frontend/src/api/eventNames.ts";
const FRONTEND_SRC = "apps/smart-ssh-community/frontend/src";

/** Rust constants the core UI does not listen to. Each needs a reason. */
export const NOT_CONSUMED_BY_CORE_UI = {
  DATA_CHANGED: "consumed by UI extensions only, the core UI refreshes through its own commands",
};

class ParseError extends Error {}
const fail = (message) => {
  throw new ParseError(message);
};

function lineOf(text, index) {
  let line = 1;
  for (let i = 0; i < index && i < text.length; i++) if (text[i] === "\n") line++;
  return line;
}

/** Constants of the names file (only `pub const X: &str = "…";` items). @returns {{ key: string, value: string, line: number }[]} */
export function parseRustNames(src, file = RUST_FILE) {
  const body = stripRustComments(src);
  const result = [];
  const re = /\bpub\s+const\s+([A-Z][A-Z0-9_]*)\s*:\s*&(?:'static\s+)?str\s*=\s*"([^"\\]*)"\s*;/g;
  let c;
  let consumed = 0;
  while ((c = re.exec(body)) !== null) {
    const gap = body.slice(consumed, c.index).trim();
    if (gap !== "") fail(`${file}:${lineOf(body, consumed)}: unreadable item: ${JSON.stringify(gap)}`);
    consumed = re.lastIndex;
    result.push({ key: c[1], value: c[2], line: lineOf(body, c.index) });
  }
  if (body.slice(consumed).trim() !== "") fail(`${file}: unreadable item: ${JSON.stringify(body.slice(consumed).trim())}`);
  if (result.length === 0) fail(`${file}: no event name constants found`);
  return result;
}

/** Entries of `export const EVENT = { … } as const`. @returns {{ key: string, value: string, line: number }[]} */
export function parseFrontendNames(src, file = EVENT_NAMES_FILE) {
  const code = stripJsComments(src);
  const m = /\bexport\s+const\s+EVENT\s*=\s*\{/.exec(code);
  if (!m) fail(`${file}: \`export const EVENT = { … }\` not found`);
  const start = m.index + m[0].length;
  const end = code.indexOf("}", start);
  if (end === -1) fail(`${file}: \`EVENT\` is not closed`);
  const body = code.slice(start, end);
  const result = [];
  for (const raw of body.split(",")) {
    const entry = raw.trim();
    if (entry === "") continue;
    const em = /^([A-Z][A-Z0-9_]*)\s*:\s*"([^"\\]*)"$/.exec(entry);
    if (!em) fail(`${file}: entry in \`EVENT\` not readable: ${JSON.stringify(entry)}`);
    result.push({ key: em[1], value: em[2], line: lineOf(code, start + body.indexOf(entry)) });
  }
  return result;
}

/** `listen("x")`, `once("x")`, `payloadListener("x")` with a string literal. @returns {{ call: string, line: number }[]} */
export function parseLiteralListeners(src) {
  const code = stripJsComments(src);
  const re = /(?<![A-Za-z0-9_$.])(listen|once|payloadListener)\s*(?:<[^()]*>)?\s*\(\s*["'`]/g;
  const result = [];
  let m;
  while ((m = re.exec(code)) !== null) result.push({ call: m[1], line: lineOf(code, m.index) });
  return result;
}

/**
 * @param {{
 *   rust: { key: string, value: string, line: number }[],
 *   frontend: { key: string, value: string, line: number }[],
 *   literals: { call: string, file: string, line: number }[],
 *   notConsumed?: Record<string, string>,
 * }} input
 * @returns {{ kind: string, name: string, where: string }[]}
 */
export function findMismatches({ rust, frontend, literals, notConsumed = NOT_CONSUMED_BY_CORE_UI }) {
  const problems = [];
  const rustByKey = new Map();
  for (const r of rust) {
    if (rustByKey.has(r.key)) problems.push({ kind: "defined twice in Rust", name: r.key, where: `${RUST_FILE}:${r.line}` });
    rustByKey.set(r.key, r);
  }
  const rustValues = new Map();
  for (const r of rust) {
    if (rustValues.has(r.value) && rustValues.get(r.value) !== r.key) {
      problems.push({ kind: "same event name under two Rust constants", name: r.value, where: `${RUST_FILE}:${r.line}` });
    }
    rustValues.set(r.value, r.key);
  }
  const frontendKeys = new Set();
  for (const f of frontend) {
    const at = `${EVENT_NAMES_FILE}:${f.line}`;
    if (frontendKeys.has(f.key)) problems.push({ kind: "defined twice in the frontend", name: f.key, where: at });
    frontendKeys.add(f.key);
    const r = rustByKey.get(f.key);
    if (!r) problems.push({ kind: "frontend constant has no Rust counterpart", name: f.key, where: at });
    else if (r.value !== f.value) {
      problems.push({ kind: `value differs from Rust ("${r.value}")`, name: `${f.key} ("${f.value}")`, where: at });
    }
  }
  for (const r of rustByKey.values()) {
    if (!frontendKeys.has(r.key) && !(r.key in notConsumed)) {
      problems.push({
        kind: "Rust constant is not in the frontend list (add it, or list it in NOT_CONSUMED_BY_CORE_UI with a reason)",
        name: r.key,
        where: `${RUST_FILE}:${r.line}`,
      });
    }
  }
  for (const key of Object.keys(notConsumed)) {
    if (!rustByKey.has(key)) problems.push({ kind: "stale NOT_CONSUMED_BY_CORE_UI entry (no such Rust constant)", name: key, where: "scripts/check-event-names.mjs" });
    else if (frontendKeys.has(key)) problems.push({ kind: "stale NOT_CONSUMED_BY_CORE_UI entry (frontend lists it)", name: key, where: "scripts/check-event-names.mjs" });
  }
  for (const l of literals) {
    problems.push({ kind: `${l.call}() with a string literal, use a constant from EVENT`, name: l.call, where: `${l.file}:${l.line}` });
  }
  return problems;
}

function walk(dir) {
  const out = [];
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) out.push(...walk(full));
    else if (/\.[cm]?[jt]sx?$/.test(entry.name) && !entry.name.endsWith(".d.ts")) out.push(full);
  }
  return out.sort();
}

/** Reads the repo under `root` and returns all problems. */
export function checkRepo(root = REPO_ROOT) {
  const rust = parseRustNames(fs.readFileSync(path.join(root, RUST_FILE), "utf8"));
  const frontend = parseFrontendNames(fs.readFileSync(path.join(root, EVENT_NAMES_FILE), "utf8"));
  const literals = [];
  for (const full of walk(path.join(root, FRONTEND_SRC))) {
    const file = path.relative(root, full).split(path.sep).join("/");
    if (isFrontendTestFile(file)) continue;
    for (const l of parseLiteralListeners(fs.readFileSync(full, "utf8"))) literals.push({ ...l, file });
  }
  return { problems: findMismatches({ rust, frontend, literals }), counts: { rust: rust.length, frontend: frontend.length } };
}

function main() {
  try {
    const i = process.argv.indexOf("--root");
    const root = i === -1 ? REPO_ROOT : path.resolve(process.argv[i + 1] ?? fail("`--root` without a directory"));
    const { problems, counts } = checkRepo(root);
    if (problems.length > 0) {
      console.error(`Event names do not match (${problems.length} problem(s)):`);
      for (const p of problems) console.error(`  - ${p.name}: ${p.kind} [${p.where}]`);
      process.exitCode = 1;
      return;
    }
    console.log(`Event names ok: ${counts.rust} in Rust, ${counts.frontend} in the frontend.`);
  } catch (err) {
    console.error(`Event name check failed: ${err instanceof ParseError ? err.message : (err.stack ?? err)}`);
    process.exitCode = 1;
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main();
}
