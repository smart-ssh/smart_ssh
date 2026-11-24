// Issue #864: a pull request that touches a security-critical path
// (.github/security-paths.txt) must change THREAT-MODEL.md or carry the line
// `Threat model: no change – <reason>` in its description.
//
// Usage: node scripts/check-threat-model-note.mjs --body-file <file> \
//          [--changed-files <file>]   (default: changed files from stdin)
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

// En dash or hyphen; the reason needs at least 10 characters.
export const NOTE_RE = /^Threat model: no change [–-] \S.{9,}/m;

export function parseSecurityPaths(text) {
  return text
    .split(/\r?\n/)
    .map((l) => l.trim())
    .filter((l) => l !== "" && !l.startsWith("#"));
}

export function isTestFile(file) {
  const base = file.split("/").pop() ?? "";
  return (
    file.split("/").slice(0, -1).includes("tests") ||
    base === "tests.rs" ||
    /^tests_.*\.rs$/.test(base) ||
    /_tests\.rs$/.test(base) ||
    /\.test\.tsx?$/.test(base)
  );
}

/** Returns null when fine, else { matches, message }. */
export function findThreatModelProblems(changedFiles, body, prefixes) {
  const files = changedFiles.map((f) => f.trim()).filter(Boolean);
  const matches = files.filter(
    (f) => !isTestFile(f) && prefixes.some((p) => f.startsWith(p)),
  );
  if (matches.length === 0) return null;
  if (files.includes("THREAT-MODEL.md")) return null;
  if (NOTE_RE.test((body ?? "").replace(/\r\n/g, "\n"))) return null;
  return {
    matches,
    message:
      "This pull request touches security-critical paths:\n" +
      matches.map((m) => `  - ${m}`).join("\n") +
      "\nFix it in one of two ways:\n" +
      "  1. update THREAT-MODEL.md in this pull request, or\n" +
      "  2. add a line to the description: `Threat model: no change – <reason>` (reason of at least 10 characters), then edit/save the description to re-run.",
  };
}

export function loadSecurityPaths(root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..")) {
  return parseSecurityPaths(fs.readFileSync(path.join(root, ".github", "security-paths.txt"), "utf8"));
}

function arg(name) {
  const i = process.argv.indexOf(name);
  return i >= 0 ? process.argv[i + 1] : undefined;
}

function main() {
  try {
    const changedFile = arg("--changed-files");
    const bodyFile = arg("--body-file");
    const changed = fs.readFileSync(changedFile ?? 0, "utf8").split(/\r?\n/);
    const body = bodyFile ? fs.readFileSync(bodyFile, "utf8") : "";
    const problem = findThreatModelProblems(changed, body, loadSecurityPaths());
    if (problem) {
      console.error(problem.message);
      process.exitCode = 1;
      return;
    }
    console.log("threat-model note ok.");
  } catch (err) {
    console.error(`threat-model note check failed: ${err.stack ?? err}`);
    process.exitCode = 1;
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main();
}
