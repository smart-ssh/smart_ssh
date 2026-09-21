# changelog.d — Changelog-Fragmente

Jede umgesetzte Spec legt hier **eine** Datei an, statt `CHANGELOG.md` direkt
zu ändern: `changelog.d/<spec-nummer>-<thema>.md`.

Grund: Parallele Coder würden sonst alle denselben `[Unreleased]`-Abschnitt
ändern, und jeder Merge wäre ein Konflikt.

Inhalt: deutsch, nutzerrelevant, gleiche Kategorien wie im CHANGELOG
(Neu / Geändert / Behoben / Sicherheit), z. B.:

```markdown
### Sicherheit
- Lesebefehle auf typische Secret-Pfade (SSH-Schlüssel, .env, …) verlangen
  jetzt immer eine Bestätigung, auch wenn eine Allow-Regel greift.
```

Beim Release werden alle Fragmente in `CHANGELOG.md` übernommen und die
Dateien gelöscht.
