// License compatibility guard (issue #1583, ADR 0163): the project is
// GPL-3.0-only, so every license allowed for dependencies in `deny.toml`
// (`[licenses] allow` and `exceptions`) must be GPL-3.0-compatible. The
// fixed list below is the one reviewer-visible place; a new entry in
// `deny.toml` needs a matching entry (with a reason) here.
import path from "node:path";
import { fileURLToPath } from "node:url";
import { parseAllowList, parseExceptions } from "./generate-third-party-notices.mjs";

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

/** SPDX identifier -> why it can be combined into a GPL-3.0-only work. */
export const GPL3_COMPATIBLE = {
  MIT: "permissive, GPL-compatible",
  "Apache-2.0": "compatible with GPL-3.0 (not with GPL-2.0)",
  "BSD-2-Clause": "permissive, GPL-compatible",
  "BSD-3-Clause": "permissive, GPL-compatible",
  ISC: "permissive, GPL-compatible",
  Zlib: "permissive, GPL-compatible",
  "Unicode-DFS-2016": "permissive data/software license, GPL-compatible",
  "Unicode-3.0": "permissive data/software license, GPL-compatible",
  "Apache-2.0 WITH LLVM-exception": "Apache-2.0 with a further permission, compatible with GPL-3.0",
  "CDLA-Permissive-2.0": "permissive data license, no copyleft",
  "MPL-2.0": "file-level copyleft; section 3.3 allows combining with GPL-3.0 unless marked Incompatible With Secondary Licenses (checked, ADR 0163)",
  "BSL-1.0": "Boost license, permissive, GPL-compatible (not BUSL)",
  blessing: "public-domain style blessing, no restrictions",
  WTFPL: "permissive, GPL-compatible",
};

/** Problems for the entries of a `deny.toml` (path), one string each. */
export function checkLicenseCompat(denyTomlPath = path.join(REPO_ROOT, "deny.toml")) {
  const entries = [
    ...parseAllowList(denyTomlPath).map((id) => ({ id, where: "[licenses] allow" })),
    ...parseExceptions(denyTomlPath).flatMap((e) =>
      e.allow.map((id) => ({ id, where: `[licenses] exceptions (${e.crate})` })),
    ),
  ];
  return entries
    .filter((e) => !Object.hasOwn(GPL3_COMPATIBLE, e.id))
    .map(
      (e) =>
        `\`${e.id}\` in ${e.where} is not on the GPL-3.0-compatible list of scripts/check-license-compat.mjs` +
        (e.id.startsWith("LicenseRef-") ? " (LicenseRef-* is never accepted)" : ""),
    );
}

function main() {
  try {
    const problems = checkLicenseCompat();
    if (problems.length > 0) {
      console.error(`License compatibility check failed (${problems.length} problem(s)):`);
      for (const p of problems) console.error(`  - ${p}`);
      process.exitCode = 1;
    }
  } catch (e) {
    console.error(`License compatibility check failed: ${e instanceof Error ? e.message : e}`);
    process.exitCode = 1;
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main();
}
