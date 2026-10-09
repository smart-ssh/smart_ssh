# changelog.d — Changelog-Fragmente

Jede **nutzerrelevante** Änderung legt hier eine Datei an, statt
`CHANGELOG.md` direkt zu ändern. Nutzerrelevant ist alles, was jemand, der
die App nutzt, bemerkt: neues Verhalten, geändertes Verhalten, eine
Fehlerbehebung oder eine Sicherheitsänderung. Das gilt unabhängig vom
Spec-Impact der Änderung (`new`, `update NNNN` oder `none`): Auch ein
Bugfix ohne Spec-Änderung bekommt ein Fragment.

Kein Fragment bekommen interne Refactorings, Tests, CI und reine
Dokumentationsänderungen (dafür ist die Git-Historie da).

Grund: Parallele Coder würden sonst alle denselben `[Unreleased]`-Abschnitt
ändern, und jeder Merge wäre ein Konflikt.

## Dateiname

- Neue Fragmente: `changelog.d/issue-<issue-nummer>-<thema>.md`, z. B.
  `issue-69-tab-nach-schliessen.md`. Die Issue-Nummer ist die des Issues,
  das die Änderung umsetzt.
- Ältere Fragmente heißen `changelog.d/<spec-nummer>-<thema>.md` (vierstellig,
  z. B. `0101-master-passwort.md`). Diese Form bleibt gültig, bestehende
  Dateien werden nicht umbenannt.
- Bringt ein Issue (oder eine Spec) mehrere nutzerrelevante Änderungen, sind
  mehrere Fragmente erlaubt. Sie unterscheiden sich im `<thema>`, z. B.
  `0101-master-passwort.md` und `0101-entsperrmaske.md`.
- `<thema>`: kurz, Kleinbuchstaben, Wörter mit `-` getrennt.

## Inhalt

Deutsch, nutzerrelevant, gleiche Kategorien wie im CHANGELOG
(Neu / Geändert / Behoben / Sicherheit), z. B.:

```markdown
### Sicherheit
- Lesebefehle auf typische Secret-Pfade (SSH-Schlüssel, .env, …) verlangen
  jetzt immer eine Bestätigung, auch wenn eine Allow-Regel greift.
```

Einträge, die nur in einer kostenpflichtigen Edition verfügbar sind, tragen
`**(Pro)**`.

## Release

Beim Release (Versions-Bump) werden alle Fragmente in den neuen
Versionsabschnitt von `CHANGELOG.md` übernommen und die Dateien gelöscht.
