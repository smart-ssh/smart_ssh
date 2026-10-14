// Issue #500 (Spec 0048): `release-notes/X.Y.Z.json` holds the Highlights of a
// release in English, German, French and Spanish. This check validates the
// schema, that `en` equals the `### Highlights` of the version in
// `CHANGELOG.md`, and that every released version after `CUTOFF_VERSION`
// has a file. Text based, no dependencies.

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { CUTOFF_VERSION, MAX_ITEMS, MIN_ITEMS, parseSections } from "./check-changelog-highlights.mjs";

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(SCRIPT_DIR, "..");

export const LANGUAGES = ["en", "de", "fr", "es"];
const VERSION_RE = /^\d+\.\d+\.\d+$/;

function compareVersions(a, b) {
  const pa = a.split(".").map(Number);
  const pb = b.split(".").map(Number);
  for (let i = 0; i < 3; i++) {
    const d = (pa[i] || 0) - (pb[i] || 0);
    if (d !== 0) return d;
  }
  return 0;
}

/** Highlight items (without the leading `- `) of one changelog section, or null. */
export function sectionHighlights(section) {
  const first = section.lines.findIndex((l) => l.trim() !== "");
  if (first === -1 || section.lines[first].trim() !== "### Highlights") return null;
  const items = [];
  for (let i = first + 1; i < section.lines.length; i++) {
    const line = section.lines[i];
    if (line.startsWith("#")) break;
    if (line.trim() === "") continue;
    if (/^- \S/.test(line)) items.push(line.slice(2).trim());
    else if (items.length > 0) items[items.length - 1] += ` ${line.trim()}`;
  }
  return items;
}

/** Returns a description of Markdown markup found in `item`, or null. */
export function markdownMarkup(item) {
  if (/^\s*(?:[-*+>#]|\d+[.)])\s/.test(item)) return "leading Markdown marker";
  if (item.includes("`")) return "backtick";
  if (item.includes("*")) return "asterisk";
  if (/(^|\s)_|_(\s|$)/.test(item)) return "underscore emphasis";
  if (/\[[^\]]*\]\([^)]*\)/.test(item)) return "Markdown link";
  return null;
}

/**
 * @param {Map<string, string>} files file name -> content
 * @param {string} changelog CHANGELOG.md text
 * @returns {{ file: string, message: string }[]}
 */
export function findReleaseNotesProblems(files, changelog, cutoff = CUTOFF_VERSION) {
  const problems = [];
  const sections = new Map(parseSections(changelog).map((s) => [s.version, s]));

  for (const [name, content] of files) {
    const fail = (message) => problems.push({ file: name, message });
    const m = /^(.+)\.json$/.exec(name);
    if (!m || !VERSION_RE.test(m[1])) {
      fail("file name must be <version>.json");
      continue;
    }
    const fileVersion = m[1];
    let data;
    try {
      data = JSON.parse(content);
    } catch (err) {
      fail(`invalid JSON: ${err.message}`);
      continue;
    }
    if (data === null || typeof data !== "object" || Array.isArray(data)) {
      fail("top level must be an object");
      continue;
    }
    const keys = Object.keys(data).sort();
    if (keys.join(",") !== "highlights,version") {
      fail(`top-level keys must be exactly "version" and "highlights", found: ${keys.join(", ") || "none"}`);
    }
    if (data.version !== fileVersion) {
      fail(`version ${JSON.stringify(data.version)} does not match the file name`);
    }
    if (!sections.has(fileVersion)) fail(`version ${fileVersion} has no section in CHANGELOG.md`);

    const hl = data.highlights;
    if (hl === null || typeof hl !== "object" || Array.isArray(hl)) {
      fail('"highlights" must be an object');
      continue;
    }
    const langKeys = Object.keys(hl);
    for (const l of LANGUAGES) if (!(l in hl)) fail(`missing language key "${l}"`);
    for (const l of langKeys) if (!LANGUAGES.includes(l)) fail(`unexpected language key "${l}"`);

    const counts = new Set();
    for (const l of LANGUAGES) {
      const list = hl[l];
      if (list === undefined) continue;
      if (!Array.isArray(list)) {
        fail(`"${l}" must be a list`);
        continue;
      }
      counts.add(list.length);
      if (list.length < MIN_ITEMS || list.length > MAX_ITEMS) {
        fail(`"${l}" has ${list.length} items, allowed are ${MIN_ITEMS} to ${MAX_ITEMS}`);
      }
      list.forEach((item, i) => {
        const where = `"${l}" item ${i + 1}`;
        if (typeof item !== "string" || item.trim() === "") return fail(`${where} is empty or not a string`);
        if (/[\r\n]/.test(item)) return fail(`${where} spans several lines`);
        if (item !== item.trim()) fail(`${where} has leading or trailing whitespace`);
        const markup = markdownMarkup(item);
        if (markup) fail(`${where} contains Markdown markup (${markup})`);
      });
    }
    if (counts.size > 1) fail("languages have different item counts");

    const section = sections.get(fileVersion);
    const expected = section ? sectionHighlights(section) : null;
    if (expected && Array.isArray(hl.en) && JSON.stringify(hl.en) !== JSON.stringify(expected)) {
      fail('"en" differs from the ### Highlights of CHANGELOG.md');
    }
    if (!content.endsWith("\n") || content !== `${JSON.stringify(data, null, 2)}\n`) {
      fail("file must be pretty-printed JSON (2 spaces) with a trailing newline");
    }
  }

  for (const s of sections.values()) {
    if (!VERSION_RE.test(s.version) || compareVersions(s.version, cutoff) <= 0) continue;
    if (!files.has(`${s.version}.json`)) {
      problems.push({ file: `release-notes/${s.version}.json`, message: "missing for a released version" });
    }
  }
  return problems;
}

export function checkReleaseNotes(root = REPO_ROOT) {
  const changelog = fs.readFileSync(path.join(root, "CHANGELOG.md"), "utf8");
  const dir = path.join(root, "release-notes");
  const files = new Map();
  if (fs.existsSync(dir)) {
    for (const name of fs.readdirSync(dir).sort()) {
      files.set(name, fs.readFileSync(path.join(dir, name), "utf8"));
    }
  }
  return findReleaseNotesProblems(files, changelog);
}

function main() {
  try {
    const problems = checkReleaseNotes();
    if (problems.length > 0) {
      console.error(`release-notes invalid (${problems.length} problem(s)):`);
      for (const p of problems) console.error(`  - ${p.file}: ${p.message}`);
      process.exitCode = 1;
      return;
    }
    console.log("release-notes ok.");
  } catch (err) {
    console.error(`release-notes check failed: ${err.stack ?? err}`);
    process.exitCode = 1;
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main();
}
