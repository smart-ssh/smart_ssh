# Recipe: a new UI text

## Purpose

A user-visible string is added or changed in the frontend. Every such text
goes through the i18n layer and exists in **all** locales.

## Files to touch

- [ ] `apps/smart-ssh-community/frontend/src/locales/en/common.json` – the
      source text (keys follow the existing nesting; plurals use the
      `_one`/`_other`-style suffixes already in the file).
- [ ] `apps/smart-ssh-community/frontend/src/locales/<lang>/common.json` for
      every other locale: `cs da de el es fi fr hi hu id it ja ko nb nl pl
      pt-BR ro ru sv tr uk vi zh-Hans`. Keep placeholders (`{{name}}`) and tags
      unchanged; use the terms in `docs/i18n-glossary.md`.
- [ ] `apps/smart-ssh-community/frontend/src/locales/identicalAllowlist.json`
      – only if a translation legitimately equals the English text (give a
      reason).
- [ ] `apps/smart-ssh-community/frontend/src/locales/lengthAllowlist.json` –
      only if a translation legitimately exceeds 2 × English + 10 characters.
- [ ] `docs/i18n-glossary.md` – only when you introduce a new product term.
- [ ] The component, using `t("key")` – no literal text in JSX.
- [ ] `docs/ui-guidelines.md` is the reference for wording and layout.

## Required tests

- The guard tests below run for every new key; no extra test is needed for
  the string itself.
- A component or browser test for the new UI that asserts the text by its
  role/label.
- If the text is long, check narrow layouts (800×560 in the browser tests).

## Guard that catches a forgotten step

- `apps/smart-ssh-community/frontend/src/locales/localeKeyParity.test.ts`
  reports a locale whose keys differ from English (missing or extra keys,
  plural forms per language), a length violation, lost placeholders/tags, and
  values identical to English (for the newer languages).
- `apps/smart-ssh-community/frontend/src/germanTextGuard.test.ts` reports
  German literal text (umlauts, `„`) in non-test `.ts`/`.tsx` files.
- No automated check: English text hard-coded in JSX without umlauts, and
  translation quality. The reviewer checks these.

## Example

PR #766: the copy-marker texts were added to `en` and to every other locale
(6 lines each) together with `apps/smart-ssh-community/frontend/src/components/ServerList.tsx`.
