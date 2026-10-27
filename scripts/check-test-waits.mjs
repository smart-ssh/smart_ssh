#!/usr/bin/env node
// Issue #966, ADR 0122 R13: a `waitFor` that only waits for a mock call
// (`toHaveBeenCalled*`) must not be followed directly by a synchronous DOM
// query, an absence check or a `fireEvent` -- the DOM state may appear on a
// later render. Wait for the rendered state instead (`findBy*`, `waitFor` on
// the DOM). A site that provably does not depend on a later render gets a
// `// wait-ok: <reason>` comment on the line directly above the `waitFor`.
import { readFileSync, readdirSync, statSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const srcDir = join(root, "apps/smart-ssh-community/frontend/src");

function* walk(dir) {
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    if (statSync(p).isDirectory()) yield* walk(p);
    else if (/\.test\.tsx?$/.test(name)) yield p;
  }
}

/** Returns the index just after the statement that starts at `start`. */
function statementEnd(text, start) {
  let depth = 0;
  for (let i = start; i < text.length; i++) {
    const c = text[i];
    if (c === "(" || c === "{" || c === "[") depth++;
    else if (c === ")" || c === "}" || c === "]") depth--;
    else if (c === ";" && depth <= 0) return i + 1;
  }
  return text.length;
}

const DOM_USE = /\b(screen\.|fireEvent\.|within\(|document\.|container\.|dragLayer\()/;
const problems = [];
for (const file of walk(srcDir)) {
  const text = readFileSync(file, "utf8");
  const rel = relative(root, file).split("\\").join("/");
  const re = /^[ \t]*await waitFor\(/gm;
  let m;
  while ((m = re.exec(text))) {
    const end = statementEnd(text, m.index);
    const stmt = text.slice(m.index, end);
    if (!/toHaveBeenCalled/.test(stmt) || DOM_USE.test(stmt)) continue;
    const rest = text.slice(end).replace(/^(\s|\/\/[^\n]*\n)+/, "");
    if (/^await\b/.test(rest)) continue;
    const next = rest.slice(0, statementEnd(rest, 0));
    if (!DOM_USE.test(next) || /\bawait\b/.test(next)) continue;
    const before = text.slice(0, m.index).trimEnd().split("\n").pop().trim();
    if (/^\/\/ wait-ok: \S/.test(before)) continue;
    const line = text.slice(0, m.index).split("\n").length;
    problems.push(`${rel}:${line}: waitFor on a mock call is followed by a synchronous DOM step (ADR 0122 R13)`);
  }
}
if (problems.length) {
  console.error(problems.join("\n"));
  process.exit(1);
}
console.log("check-test-waits: ok");
