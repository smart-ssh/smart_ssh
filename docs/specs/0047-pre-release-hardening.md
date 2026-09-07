# Spec: Pre-Release-Härtung für die 0.x-Testphase

Status: Entwurf
Modul: `crates/app-shell`, `crates/persistence-sqlite`,
`crates/credentials-keyring` — alles **öffentliches** Repo `smart_ssh`
Abhängigkeiten: Server-Verwaltung (0008), SQLite-Persistenz (0004),
strukturiertes Logging (0016), i18n/Fehler-Codes (0024)

> Bündelt die technischen Blocker aus der Pre-Release-Checkliste für die
> **macOS-Runde-1** der Testphase. Kein Windows/Linux-Anteil (→ Runde 2).
> **Priorität ERHÖHT für A2** (Credential-Handling), sonst NORMAL.
>
> **Wichtig — gemischte Art der Aufgaben**: Manche Punkte sind echtes
> Bauen (A2), manche sind **Verifikation + ggf. Nachschärfung** (A3, B1,
> B2, D2). Bei den Verifikations-Punkten gilt: erst prüfen, mir den
> Ist-Zustand berichten, dann nur reparieren, was tatsächlich fehlt — nicht
> blind neu bauen, was schon funktioniert.

## Fund A2 — `create_server`-Keychain-Rollback bei Fehler (ERHÖHT)

**Echtes Bauen.** Aktuell (Backlog-Fund): Schlägt `create_server` fehl,
*nachdem* bereits ein Secret (z. B. Sudo-Passwort) im Keychain gespeichert
wurde, bleibt ein **verwaister Keychain-Eintrag** für eine Server-ID zurück,
die es nicht (mehr) gibt.

**Fix**: `create_server` rollt bei einem Fehler **alle** in diesem Aufruf
bereits geschriebenen Keychain-Einträge zurück (Passwort, Sudo-Passwort,
Key-Passphrase — was auch immer angelegt wurde), bevor der Fehler
zurückgegeben wird. Kein verwaister Eintrag.

- Umsetzung sauber testbar: den echten Produktionspfad extrahieren (analog
  zum `delete_server`-Fix aus Spec 0046, der die Logik aus dem
  `#[tauri::command]`-Handler in eine testbare Funktion gezogen hat), nicht
  nur im Command-Handler.
- Test: Fehler nach dem Speichern des Sudo-Passworts provozieren (mit einem
  Mock-Store, der beim zweiten Write oder beim DB-Insert fehlschlägt) →
  danach ist **kein** Keychain-Eintrag für diese Server-ID vorhanden.
- Der Test muss gegen den **ungefixten** Stand fehlschlagen (verwaister
  Eintrag bleibt) — empirisch verifizieren.

## Fund A3 — Migration N→N+1 verifizieren (Datenverlust über Builds)

**Verifikation, ggf. Nachschärfung.** Die Tester behalten über mehrere
Wochenbuilds hinweg Daten — DB, Keychain-Referenzen, Sitzungen, Regeln
müssen den Versionssprung überleben. Es gibt versionierte SQLite-
Migrationen, aber ein echter N→N+1-Durchlauf *mit vorhandenen Daten* wurde
nie gezielt getestet.

**Aufgabe**:
1. Prüfe den Migrations-Mechanismus (persistence-sqlite): Laufen alle
   Migrationen sauber vorwärts auf einer DB, die mit einer früheren
   Migrations-Version **und echten Daten** (Server, Regeln, Sessions,
   Notizen) angelegt wurde?
2. **Schreibe einen Migrations-Integrationstest**, der genau das absichert:
   eine DB auf einem früheren Schema-Stand mit Testdaten anlegen, die
   aktuellen Migrationen anwenden, verifizieren dass alle Daten erhalten und
   lesbar sind. Das ist der eigentliche Wert — ein dauerhafter Regressions-
   schutz gegen "die nächste Migration frisst Testerdaten".
3. Falls dabei ein echtes Migrations-Problem auffällt: melden und beheben.
   Falls alles sauber ist: den Test als Absicherung dalassen und mir das
   berichten.

Berichte mir den Ist-Zustand des Migrations-Mechanismus, bevor/während du
den Test baust.

## Fund B1 — Logging als Allererstes beim Start

**Verifikation + Nachschärfung.** Strukturiertes Logging existiert (Spec
0016). Die Checkliste verlangt aber, dass die Logdatei **vor allem anderen**
geschrieben wird (Version, OS, Datenpfad, jeder Startschritt) — damit ein
"geht nicht" beim Tester diagnostizierbar ist, statt spurlos.

**Aufgabe**:
1. Prüfe: Wann genau im Startablauf wird das Log initialisiert? Steht es
   **vor** den Schritten, die scheitern könnten (DB öffnen, Migration,
   Keychain-Zugriff)? Oder könnte ein früher Fehler passieren, *bevor* das
   Log steht — sodass gar nichts geschrieben wird?
2. Falls Logging zu spät initialisiert wird: **so früh wie möglich**
   ziehen — als eine der allerersten Anweisungen im Start, bevor DB/Keychain/
   Migration. Die erste Logzeile enthält: App-Version, OS/Plattform,
   aufgelöster Datenpfad.
3. Jeder wesentliche Startschritt (Datenverzeichnis auflösen, DB öffnen,
   Migration starten/fertig, Keychain-Init, Fenster erzeugen) bekommt eine
   Logzeile, sodass man aus dem Log ablesen kann, **wie weit** der Start kam,
   bevor etwas schiefging.

Berichte mir, wie der Startablauf aktuell aussieht (Reihenfolge), bevor du
umbaust.

## Fund B2 — Keychain gesperrt/nicht verfügbar → Log-Meldung statt Panic (macOS)

**Verifikation, ggf. Fix.** Teilweise in Spec 0040 gehärtet (Chat-
Verschlüsselungsschlüssel). Für die Testphase relevant: Was passiert auf
**macOS mit gesperrter Keychain** beim Start und bei der ersten
Credential-Nutzung?

**Aufgabe**:
1. Gehe die Stellen durch, die beim Start/früh auf den Keychain zugreifen.
   Führt ein gesperrter/verweigerter Keychain irgendwo zu einem **Panic**
   (statt einer Log-Meldung + kontrolliertem Weiterlaufen bzw. sauberer
   Fehleranzeige)?
2. Wo ein Panic droht: in eine geloggte, behandelte Fehlermeldung umwandeln
   (der native Startfehler-**Dialog** bleibt geparkt/SPÄTER — hier reicht
   laut Checkliste die **Logdatei**). Die App soll nicht spurlos
   verschwinden.
3. Der Linux-ohne-Keyring-Fall ist **Runde 2**, hier nicht nötig — aber
   falls dir auffällt, dass der Fix beide Fälle billig mitnimmt, gern
   erwähnen.

Berichte den Ist-Zustand (welche Start-Keychain-Zugriffe, welche panicen).

## Fund D2 — Die fünf Fehlermeldungen auf dem Fünf-Minuten-Pfad verständlich

**Verifikation + Nachschärfung.** Die Fehlertypen existieren
(`SshError`/`AiError` mit Codes, Spec 0024). Aber ob jede der fünf für einen
Tester *verständlich* ist, wurde nie geprüft.

**Aufgabe**: Geh die fünf Fehlerfälle durch und prüfe je, welche Meldung der
Nutzer tatsächlich sieht (im Chat/UI, nicht nur im Log):
1. **Falscher API-Key** (Provider lehnt ab).
2. **Modell nicht gefunden** (falscher Modellname).
3. **Ollama nicht gestartet** (lokaler Provider nicht erreichbar).
4. **Host nicht erreichbar** (SSH-Verbindung scheitert).
5. **Host-Key unbekannt/geändert** (die zwei Host-Key-Dialoge).

Für jede: Ist die Meldung für einen Tester verständlich und **nennt sie den
nächsten Schritt** ("prüfe den API-Key", "starte Ollama", "Host erreichbar?")
— oder ist es rohe Technik (`AuthenticationFailed`, ein Stacktrace, eine
englische Lib-Meldung)? Wo unverständlich: die Meldung verständlich machen
(über die bestehende Fehler-Code→Übersetzung aus Spec 0024, DE/EN). Wo schon
gut: so lassen und berichten.

Berichte mir eine kurze Tabelle: die fünf Fälle, was der Nutzer aktuell
sieht, was du geändert hast.

## Sicherheits-Invarianten

- A2: Kein verwaister Keychain-Eintrag nach fehlgeschlagenem `create_server`.
- Keiner der Punkte lockert eine Filter-Engine-/Gating-Entscheidung.
- B1/B2: Ein Startfehler führt nie zu einem spurlosen Verschwinden — mindestens
  eine lesbare Logzeile.

## Reihenfolge / Hinweis

Jeder Fund ein eigener Commit. A2 zuerst (ERHÖHT, echtes Bauen). A3, B1, B2,
D2 sind Verifikations-lastig — bei jedem **erst berichten, dann fixen**.
Am Ende `spec-reviewer` (A2 mit ERHÖHT, Rest NORMAL). Regressionstests, wo
gebaut wird, gegen den ungefixten Stand verifizieren (CLAUDE.md).
