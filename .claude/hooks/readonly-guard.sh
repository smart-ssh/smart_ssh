#!/usr/bin/env bash
# PreToolUse(Bash) für lesende Agenten (repo-research, spec-reviewer), als
# Frontmatter-Hook eingehängt. Diese Agenten haben kein Write und kein Edit;
# dieser Wächter schließt den Weg über die Shell.
#
# Ein Geländer gegen Versehen, keine Sandbox: Ein verschleiertes Kommando
# (Variablen, base64, ein Skriptaufruf) kommt durch. Die harte Linie ist die
# Werkzeugliste im Frontmatter.
#
# Schlägt GESCHLOSSEN fehl: fehlt jq, wird blockiert statt durchgelassen
# (Exit 1 blockiert in Claude Code nichts, nur Exit 2).
set -uo pipefail

command -v jq >/dev/null 2>&1 || {
  echo "readonly-guard: jq fehlt (brew install jq) — Aktion vorsorglich blockiert." >&2
  exit 2
}

INPUT="$(cat)"
j() { jq -r "$1 // empty" <<<"$INPUT"; }
block() {
  echo "BLOCKIERT: $1" >&2
  [[ $# -ge 2 ]] && echo "$2" >&2
  exit 2
}

cmd="$(j '.tool_input.command')"
[[ -z "$cmd" ]] && exit 0
flat="$(tr '\n' ' ' <<<"$cmd")"

why="Dieser Agent liest nur. Ergebnisse gibst du im Text zurück, nicht in
eine Datei. Ändern muss, wer dich beauftragt hat."

# --- Umleitungen in Dateien ------------------------------------------------
# Harmlose Formen zuerst entfernen: >/dev/null, 2>/dev/null, &>/dev/null, 2>&1.
scrub="$(sed -E 's#[0-9]*&?>>?[[:space:]]*/dev/(null|stdout|stderr)##g; s#[0-9]*>&[0-9-]##g' <<<"$flat")"
if grep -qE '>' <<<"$scrub"; then
  block "Umleitung in eine Datei" "$why"
fi

# --- Schreibende Kommandos -------------------------------------------------
if grep -qE '(^|[;&|[:space:]])(rm|mv|cp|mkdir|rmdir|touch|tee|dd|truncate|ln|chmod|chown|chgrp|install|shred|unlink)([[:space:]]|$)' <<<"$flat"; then
  block "schreibendes Kommando" "$why"
fi

# In-place-Editoren
if grep -qE '(^|[;&|[:space:]])(sed|perl|ruby)([[:space:]]+-[^[:space:]]*)*[[:space:]]+-[a-zA-Z]*i' <<<"$flat"; then
  block "In-place-Bearbeitung (-i)" "$why"
fi
if grep -qE '(^|[;&|[:space:]])(ed|ex|vi|vim|nano|emacs|patch)([[:space:]]|$)' <<<"$flat"; then
  block "Editor bzw. patch" "$why"
fi

# --- Git: alles außer Lesen ------------------------------------------------
if grep -qE '(^|[;&|[:space:]])git([[:space:]]+-C[[:space:]]+("[^"]+"|[^[:space:]]+))?[[:space:]]+(add|commit|checkout|switch|restore|reset|rebase|merge|cherry-pick|revert|clean|stash|rm|mv|apply|am|format-patch|push|pull|fetch|remote|init|clone|worktree|submodule|gc|prune|notes|update-ref|filter-branch|config)([[:space:]]|$)' <<<"$flat"; then
  block "verändernder git-Befehl" "$why
Erlaubt sind die lesenden: log, show, diff, status, blame, grep, ls-files,
rev-parse, rev-list, describe, shortlog, cat-file, branch --list, tag --list,
merge-base, merge-tree."
fi

# --- Beliebiger Code, der schreiben könnte ---------------------------------
if grep -qE '(^|[;&|[:space:]])(eval|source|exec)([[:space:]]|$)' <<<"$flat"; then
  block "eval / source / exec" "$why"
fi
if grep -qE '(^|[;&|[:space:]])(python3?|node|ruby|perl|php|osascript)([[:space:]]+-[^[:space:]]+)*[[:space:]]+-(c|e)([[:space:]]|$)' <<<"$flat"; then
  block "Inline-Skript (-c / -e)" "$why
Für Suche und Auswertung reichen grep, rg, awk, sed (ohne -i), jq, sort, wc."
fi

# --- Bauen und Installieren ------------------------------------------------
if grep -qE '(^|[;&|[:space:]])(cargo[[:space:]]+(build|run|test|install|fix|clippy|add|remove|update|clean)|npm[[:space:]]+(i|install|ci|run|exec|update)|pnpm|yarn|pip3?[[:space:]]+install|brew[[:space:]]+(install|upgrade|uninstall)|make|docker)([[:space:]]|$)' <<<"$flat"; then
  block "Bauen bzw. Installieren" "$why
Das kostet Zeit und verändert den Arbeitsbaum. Bauen und Testen ist Sache
der Sitzung, die dich beauftragt hat."
fi

exit 0
