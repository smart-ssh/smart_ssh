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

Fragmente sind auf Englisch geschrieben, nutzerrelevant und verwenden
dieselben Kategorien wie `CHANGELOG.md` (`### Added`, `### Changed`,
`### Fixed`, `### Security`), z. B.:

```markdown
### Security
- Read commands on typical secret paths (SSH keys, .env, …) now always
  require confirmation, even if an allow rule matches.
```

### Regeln für Einträge

- **Höchstens zwei Zeilen** je Eintrag.
- Beschreibe die **sichtbare Wirkung**, nicht den Mechanismus: was der Nutzer
  merkt, nicht wie es umgesetzt ist.
- **Nichts Internes:** keine internen Abläufe, kein Tooling, keine Issue- oder
  Spec-Buchhaltung, die der Eintrag nicht braucht.

Gut:

```markdown
- When a tab is closed, focus moves to the neighbouring tab.
```

Schlecht (Mechanismus, intern, zu lang):

```markdown
- `TabStrip` now calls `focusNeighbor()` after `closeTab` so that the
  reducer state stays consistent per Spec 0063 (issue #69) and the focus
  ref no longer falls back to `null`.
```

Einträge, die nur in einer kostenpflichtigen Edition verfügbar sind, tragen
`**(Pro)**`.

## Release

Beim Release (Versions-Bump) werden alle Fragmente in den neuen
Versionsabschnitt von `CHANGELOG.md` übernommen und die Dateien gelöscht.

Der neue Versionsabschnitt beginnt mit `### Highlights`: 3 bis 6
Listenpunkte, je genau eine Zeile, nutzerseitig und ohne Interna. Sie
erscheinen als Kurzfassung auf der Download-Seite. Sie werden beim Release geschrieben, nicht als Fragment; die
übrigen Kategorien folgen danach unverändert. Ein CI-Check
(`scripts/check-changelog-highlights.mjs`, Teil von `npm run lint`) prüft
das für alle Versionen nach 0.5.2.
