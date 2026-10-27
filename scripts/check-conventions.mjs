// Repository conventions guard (issue #858, AGENTS.md "Testing conventions"):
//  1. No new `*_tests.rs` files; the built-in list of existing ones may only shrink.
//  2. `LOCAL_SERVER_ID` / `local_server` must not appear in core/filter or core/risk.
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

/** Existing `*_tests.rs` files (repo-relative, forward slashes). Only ever remove entries. */
export const EXISTING_TESTS_RS = [
  "crates/app-logic/src/database_startup/instance_lock_tests.rs",
  "crates/app-shell/src/commands/sftp/local_path_tests.rs",
  "crates/app-shell/src/commands/sftp/revocation_tests.rs",
  "crates/app-shell/src/commands/sftp/size_limit_tests.rs",
  "crates/core/src/filter/adversarial_tests.rs",
  "crates/core/src/filter/property_tests.rs",
  "crates/core/src/risk/property_tests.rs",
];

export const GUARDED_DIRS = ["crates/core/src/filter", "crates/core/src/risk"];
export const FORBIDDEN_IDENTIFIERS = ["LOCAL_SERVER_ID", "local_server"];

const SKIP_DIRS = new Set(["target", "node_modules", ".git"]);

function walk(dir, out = []) {
  if (!fs.existsSync(dir)) return out;
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    if (SKIP_DIRS.has(entry.name)) continue;
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) walk(full, out);
    else out.push(full);
  }
  return out;
}

const rel = (root, file) => path.relative(root, file).split(path.sep).join("/");

export function checkConventions(root = REPO_ROOT, existing = EXISTING_TESTS_RS) {
  const problems = [];
  const crates = path.join(root, "crates");
  const found = walk(crates)
    .filter((f) => f.endsWith("_tests.rs"))
    .map((f) => rel(root, f));
  for (const f of found) {
    if (!existing.includes(f)) {
      problems.push(
        `${f}: new \`*_tests.rs\` file; use \`<module>/tests_<topic>.rs\` (AGENTS.md, Testing conventions)`,
      );
    }
  }
  for (const f of existing) {
    if (!fs.existsSync(path.join(root, f))) {
      problems.push(`${f}: listed in scripts/check-conventions.mjs but no longer exists; remove it from the list`);
    }
  }
  for (const dir of GUARDED_DIRS) {
    for (const file of walk(path.join(root, dir))) {
      if (!file.endsWith(".rs")) continue;
      const text = fs.readFileSync(file, "utf8");
      for (const id of FORBIDDEN_IDENTIFIERS) {
        if (text.includes(id)) {
          problems.push(
            `${rel(root, file)}: \`${id}\` must not appear in filter/risk code (AGENTS.md, "No special-casing the local pseudo-server")`,
          );
        }
      }
    }
  }
  return problems;
}

function main() {
  try {
    const problems = checkConventions();
    if (problems.length > 0) {
      console.error(`Convention check failed (${problems.length} problem(s)):`);
      for (const p of problems) console.error(`  - ${p}`);
      process.exitCode = 1;
      return;
    }
    console.log("Conventions ok.");
  } catch (err) {
    console.error(`Convention check failed to run: ${err.stack ?? err}`);
    process.exitCode = 1;
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main();
}
