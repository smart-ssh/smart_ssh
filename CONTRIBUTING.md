# Contributing to Smart SSH

Thanks for helping. This page is the short version; it links to the files that hold the details.

## Before you start

- Open an issue first, or find an existing one, and say you want to work on it. Use the forms in [`.github/ISSUE_TEMPLATE/`](.github/ISSUE_TEMPLATE/) (bug, feature, task).
- **Never report a security problem as an issue.** Follow [SECURITY.md](SECURITY.md).
- Product principles and architecture rules are in [AGENTS.md](AGENTS.md); [docs/codemap.md](docs/codemap.md) helps you find the right file.

## How a change is made

Spec first: a spec describes behaviour, and it ships in the **same pull request** as the code. Every change has a spec impact:

- `new` — behaviour in an area no spec covers yet.
- `update NNNN` — behaviour of an existing spec changes or is added (preferred over a new spec).
- `none` — a bug fix of already specified behaviour, refactoring, tests, CI.

Architecture decisions that cut across a single spec, or non-obvious implementation choices, get an ADR in the same pull request. Details: [docs/specs/README.md](docs/specs/README.md) and [docs/adr/README.md](docs/adr/README.md). After adding a spec or ADR, regenerate the indexes with `node scripts/check-spec-index.mjs --write-index`. Recurring change checklists (new command, new UI text, new migration): [docs/recipes/README.md](docs/recipes/README.md). UI rules: [docs/ui-guidelines.md](docs/ui-guidelines.md).

## Local checks

Run the gate before you push; it mirrors CI ([`community.yml`](.github/workflows/community.yml)):

```bash
scripts/gate.sh            # everything, in CI order
scripts/gate.sh rust       # cargo fmt --check, clippy -D warnings, tests, build
scripts/gate.sh frontend   # tsc, npm run lint, vitest, npm run build
```

The individual commands (`cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `npm run lint`, `npm test`) and the setup are in [README_DEV.md](README_DEV.md) and [docs/dev-environment.md](docs/dev-environment.md). CI also runs `cargo deny` and `cargo audit` and the browser tests (`npm run test:e2e` in `apps/smart-ssh-community/frontend`). A regression test must fail against the un-fixed code.

## Changelog

A user-visible change (new or changed behaviour, fix, security) adds one fragment in `changelog.d/`, written in English. Do not edit `CHANGELOG.md` directly. Naming and format: [changelog.d/README.md](changelog.d/README.md).

## Pull requests

- Base branch is `develop`.
- One issue per pull request; reference it (`Closes #<n>`).
- Use [Conventional Commits](https://www.conventionalcommits.org/), scoped to the area, e.g. `fix(app-shell): … (#<n>)`.
- CI must be green.
- Changes under `.github/` need a review by a code owner ([`.github/CODEOWNERS`](.github/CODEOWNERS)).
- A change that touches security-critical code updates [THREAT-MODEL.md](THREAT-MODEL.md), or says `Threat model: no change – <reason>` in the description.

## Translations

German and English are maintained by hand; all other languages are machine-translated. Corrections are welcome as an issue or pull request. Terminology: [docs/i18n-glossary.md](docs/i18n-glossary.md).

## Contribution terms

Smart SSH is licensed under GPL-3.0-only ([LICENSE](LICENSE)). Every contributor signs the [Contributor License Agreement](CLA.md) once, in their first pull request, by posting the comment the CLA bot asks for; the signature covers all later contributions. The CLA exists so that the copyright holder can keep relicensing the whole code base.

## Code of conduct

Be respectful: to contributors, maintainers and users.
