# Spec: Versionierung & Changelog (0.4.0-Launch)

Status: Entwurf
Repo: **öffentlich** `smart_ssh` — `docs/specs/`
Abhängigkeiten: App-Shell (0038), Backlog-Grundsatzentscheidung
"einheitliche Versionierung"

> Führt die verbindliche Versionsnummer und ein gepflegtes Changelog ein.
> Die maßgebliche Produktversion lebt in diesem Repo; die
> Versionierungs-**Konvention** steht in der CLAUDE.md.

## Getroffene Entscheidungen

- **Dieses Repo ist die Single Source of Truth für die Produktversion.**
- **Aktueller Stand: 0.3.0** — kein Umstellen, nur Konvention festschreiben
  + Bump auf 0.4.0.
- **0.4.0 = erste öffentliche Testversion.** 0.3.0 wird im Changelog als
  "Initial Early Access" abgeschlossen (Ist-Zustand zusammengefasst, keine
  rekonstruierte 0.1/0.2-Historie).
- **Changelog: ein Changelog in diesem Repo, das den vollen
  Produktumfang abdeckt.** Funktionen, die nur in einer kostenpflichtigen
  Edition verfügbar sind, werden mit `**(Pro)**` markiert, damit Nutzer der
  frei baubaren Edition sehen, was ihre Edition nicht enthält.

## A. Versionierung festschreiben

### A.1 Single Source of Truth
Die maßgebliche Produktversion steht in diesem Repo (die Stelle, die Tauri
als Produktversion nutzt: `tauri.conf.json` → `version`, ggf. Workspace-
`Cargo.toml`, Frontend-`package.json`). Sie ist der verbindliche Bezugspunkt
für jeden Build des Produkts.

### A.2 Konvention (gehört in die CLAUDE.md)
- Version wird **bewusst von Stefan** gebumpt, nicht vom Coder automatisch,
  nicht pro Feature. SemVer: Feature → Minor, Fix → Patch.
- Beim Bump: Version ändern, Changelog `[Unreleased]` → `[X.Y.Z]`, taggen.

### A.3 Bump auf 0.4.0
Öffentlich auf `0.4.0` setzen — alle Stellen, die die Version tragen
(`tauri.conf.json`, ggf. Workspace-`Cargo.toml`, Frontend-`package.json`),
konsistent.

## B. Changelog (Modell A)

### B.1 Format
`CHANGELOG.md` im Repo-Root, **Keep a Changelog** (keepachangelog.com):
umgekehrt chronologisch, pro Version ein Abschnitt mit Datum, Kategorien
**Added / Changed / Fixed / Security**. Funktionen, die nur in einer
kostenpflichtigen Edition verfügbar sind, tragen `**(Pro)**`.

### B.2 Erster Inhalt
- `## [Unreleased]` — leerer Sammelabschnitt oben.
- `## [0.4.0] — <Datum>` — erste öffentliche Testversion. Added: Word-Export
  als erstes Pro-Modul **(Pro)**, Lizenz-Eingabe-UI mit Live-Aktivierung
  **(Pro)**. Fixed: Härtungsrunde (verwaiste Keychain-Einträge bei
  create_server-Fehler, verständlichere Fehlermeldungen mit nächsten
  Schritten, Startup-Logging/Panic-Hook), Ressourcen-Caps
  (Output-Streaming-Cap, Rekursions-Cap), Fence-Integrität. Aus den Specs
  0043–0047 ableiten, **nur Nutzerrelevantes**.
- `## [0.3.0] — Initial Early Access` — **kompakte Ist-Zustands-
  Zusammenfassung**: SSH-Client mit KI-Copilot, Filter-/Policy-Engine mit
  Bestätigungs-Workflow, Server-/Gruppen-/Regel-/Notizverwaltung,
  MCP-Server, persistente Sessions, Risiko-Indikatoren, Multi-Provider-KI
  mit Redaction, macOS signiert+notarisiert.

### B.3 Pflege-Konvention (gehört in die CLAUDE.md)
- Nutzerrelevante Änderungen kommen in `[Unreleased]`, während sie entstehen.
- Beim Bump: `[Unreleased]` → `[X.Y.Z] — Datum`, neuer leerer `[Unreleased]`.
- **Nur Nutzerrelevantes** — interne Refactorings/Test-Infrastruktur nicht
  (dafür ist die Git-Historie da). Das Changelog ist für den, der die App
  nutzt.
- Pro-Features `**(Pro)**` markieren.

### B.4 Anzeige in der App — nicht hier
Ob das Changelog auch **in der App** gezeigt wird ("Was ist neu"), ist
Backlog, nicht Teil dieser Spec.

## C. Invarianten

- Die bestehende CI (`community.yml`) bleibt grün (die Versionsänderung darf
  nichts brechen).

## D. Reihenfolge

1. Konvention in CLAUDE.md (A.2 + B.3).
2. Bump auf 0.4.0 (A.3).
3. `CHANGELOG.md` (B).
