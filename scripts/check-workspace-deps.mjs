// Workspace dependency guard (issue #897, AGENTS.md "Workspace dependencies"):
//  1. A third-party crate declared by two or more workspace crates must be in
//     the root `[workspace.dependencies]`.
//  2. A crate manifest must not pin a version (or path) for a crate that is in
//     `[workspace.dependencies]`; it uses `{ workspace = true }` instead.
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

/** Manifest paths of all workspace members (`crates/*`, `apps/*`). */
function memberManifests(root) {
  const out = [];
  for (const dir of ["crates", "apps"]) {
    const base = path.join(root, dir);
    if (!fs.existsSync(base)) continue;
    for (const e of fs.readdirSync(base, { withFileTypes: true })) {
      const m = path.join(base, e.name, "Cargo.toml");
      if (e.isDirectory() && fs.existsSync(m)) out.push(m);
    }
  }
  return out.sort();
}

/** Dependency entries `{ name, inline }` per section header of a manifest. */
export function parseDeps(text) {
  const deps = [];
  let section = null;
  let current = null;
  for (const raw of text.split("\n")) {
    const line = raw.trim();
    const header = /^\[(.+)\]$/.exec(line);
    if (header) {
      section = header[1];
      current = null;
      const tbl = /(?:^|\.)((?:dev-|build-)?dependencies)\.([A-Za-z0-9_-]+)$/.exec(section);
      if (tbl) deps.push((current = { name: tbl[2], section, body: "" }));
      continue;
    }
    if (current) {
      current.body += ` ${line}`;
      continue;
    }
    if (!section || !/(^|\.)(dev-|build-)?dependencies$/.test(section)) continue;
    const m = /^([A-Za-z0-9_-]+)\s*=\s*(.*)$/.exec(line);
    if (m) deps.push({ name: m[1], section, body: m[2] });
  }
  return deps;
}

export function workspaceDepNames(rootText) {
  const names = new Set();
  let inside = false;
  for (const raw of rootText.split("\n")) {
    const line = raw.trim();
    const header = /^\[(.+)\]$/.exec(line);
    if (header) {
      inside = header[1] === "workspace.dependencies";
      continue;
    }
    const m = inside && /^([A-Za-z0-9_-]+)\s*=/.exec(line);
    if (m) names.add(m[1]);
  }
  return names;
}

export function checkWorkspaceDeps(root = REPO_ROOT) {
  const problems = [];
  const wsNames = workspaceDepNames(fs.readFileSync(path.join(root, "Cargo.toml"), "utf8"));
  const users = new Map(); // third-party crate -> Set of member manifests
  for (const manifest of memberManifests(root)) {
    const rel = path.relative(root, manifest).split(path.sep).join("/");
    for (const d of parseDeps(fs.readFileSync(manifest, "utf8"))) {
      const inherited = /\bworkspace\s*=\s*true\b/.test(d.body);
      if (wsNames.has(d.name) && !inherited) {
        problems.push(`${rel}: \`${d.name}\` is in [workspace.dependencies]; use \`{ workspace = true }\``);
      }
      if (!/\bpath\s*=/.test(d.body) && !inherited) {
        if (!users.has(d.name)) users.set(d.name, new Set());
        users.get(d.name).add(rel);
      }
    }
  }
  for (const [name, set] of users) {
    if (set.size >= 2 && !wsNames.has(name)) {
      problems.push(
        `\`${name}\` is declared by ${set.size} crates (${[...set].join(", ")}) but is not in the root [workspace.dependencies]`,
      );
    }
  }
  return problems;
}

function main() {
  try {
    const problems = checkWorkspaceDeps();
    if (problems.length > 0) {
      console.error(`Workspace dependency check failed (${problems.length} problem(s)):`);
      for (const p of problems) console.error(`  - ${p}`);
      process.exitCode = 1;
      return;
    }
    console.log("Workspace dependencies ok.");
  } catch (err) {
    console.error(`Workspace dependency check failed to run: ${err.stack ?? err}`);
    process.exitCode = 1;
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main();
}
