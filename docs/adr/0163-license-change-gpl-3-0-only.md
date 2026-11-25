# ADR 0163 — License change Apache-2.0 → GPL-3.0-only

## Status

Accepted · Issue: #1583 · Spec 0099 · Supersedes ADR 0060

## Context

The maintainer decided that, starting with release 0.8.0, Smart SSH is licensed under the GNU General Public License, version 3 only. Versions up to and including 0.7.0 stay under Apache-2.0, because a granted Apache license cannot be withdrawn. The copyright holder owns the copyright to all project code in this repository (no outside contributions so far), so no further consent is needed. Third-party code under `vendor/` (e.g. the patched `russh`, Apache-2.0) keeps its own license; Apache-2.0 code may be combined into a GPL-3.0 work.

## Decision

- License: `GPL-3.0-only` (SPDX), explicitly not "or later". `LICENSE` holds the unmodified GPL-3.0 text.
- Declared in the workspace manifest, the frontend manifest, the Tauri bundle metadata (`bundle.license`, no `licenseFile`, so no installer click-through page) and the README.
- No per-file license headers, same reasoning as ADR 0060.
- `NOTICE` keeps name and copyright line and adds the license statement, including that the copyright holder may also offer the code under other terms.

## Consequences

- `[licenses.private] ignore = true` in `deny.toml` stays (with `publish = false`), so cargo-deny and cargo-about keep checking only third-party code. `GPL-3.0-only` is not added to the allow list.
- Every entry in the allow list and in `exceptions` must be GPL-3.0-compatible. `scripts/check-license-compat.mjs` (part of `npm run lint`) fails on any entry that is not on its fixed list of compatible SPDX identifiers, each with a reason. The list lives as a constant in the script.
- MPL-2.0 Exhibit B check ("Incompatible With Secondary Licenses"): the notice is absent in all of the following; the only occurrences are the standard definition and Exhibit B text in the unmodified MPL-2.0 `LICENSE` files, and none in source files. Crates (from `Cargo.lock`): `cssparser` 0.37.0, `cssparser-macros` 0.7.1, `dtoa-short` 0.3.5, `option-ext` 0.2.0, `selectors` 0.38.0 (no license file in the crate, no notice anywhere). npm (frontend lockfile): `@axe-core/playwright` 4.13.0, `axe-core` 4.13.0, `lightningcss` 1.33.0 and 1.32.0 (nested under `@tailwindcss/node`) and its platform packages `lightningcss-*`. So MPL-2.0 section 3.3 applies and combination with GPL-3.0 is allowed.
- Spec 0099 L1–L3, Spec 0001, Spec 0090 A12 and Spec 0070 are updated; ADR 0060 is marked superseded.
