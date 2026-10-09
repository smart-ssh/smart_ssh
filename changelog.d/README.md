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

### Regeln für Einträge

- **Höchstens zwei Zeilen** je Eintrag.
- Beschreibe die **sichtbare Wirkung**, nicht den Mechanismus: was der Nutzer
  merkt, nicht wie es umgesetzt ist.
- **Nichts Internes:** keine internen Abläufe, kein Tooling, keine Issue- oder
  Spec-Buchhaltung, die der Eintrag nicht braucht.

Gut:

```markdown
- Beim Schließen eines Tabs bleibt der Fokus auf dem Nachbar-Tab.
```

Schlecht (Mechanismus, intern, zu lang):

```markdown
- `TabStrip` ruft nach `closeTab` jetzt `focusNeighbor()` auf, damit der
  Reducer-Zustand nach Spec 0063 (Issue #69) konsistent bleibt und die
  Fokus-Ref nicht mehr auf `null` fällt.
```

Einträge, die nur in einer kostenpflichtigen Edition verfügbar sind, tragen
`**(Pro)**`.

## Release

Beim Release (Versions-Bump) werden alle Fragmente in den neuen
Versionsabschnitt von `CHANGELOG.md` übernommen und die Dateien gelöscht.

Der neue Versionsabschnitt beginnt mit `### Highlights`: 3 bis 6
Listenpunkte, je genau eine Zeile, nutzerseitig und ohne Interna. Die
Highlights sind der **einzige englische Teil** des sonst deutschen
Changelogs, weil sie als Kurzfassung auf der (englischen) Download-Seite
erscheinen. Sie werden beim Release geschrieben, nicht als Fragment; die
übrigen Kategorien folgen danach unverändert. Ein CI-Check
(`scripts/check-changelog-highlights.mjs`, Teil von `npm run lint`) prüft
das für alle Versionen nach 0.5.2.
