# Spec: Reiches Session-Modell (Ledger, Summary, Kompaktierung)

Status: Entwurf
Repo: **öffentlich** `smart_ssh`
Modul: `crates/core` (Session-Modell, Kompaktierung), `crates/persistence-sqlite`
(Ledger/Summary-Tabellen), `crates/app-shell` (Kontext-Aufbau, Notiz-Dialog),
Frontend (Notiz-Warnung)
Abhängigkeiten: SQLite-Persistenz + Migrationen (0004/0047), Chat-Session-
Persistenz (0034), Notizen + Scope (ADR 0003/0004), KI-Provider + Redaction
(0006), Rate-Limit-Handling (0051), Body-Timeout-Fix, Fencing (0039)

> **Der zentrale Architektur-Baustein vor 1.0.** Löst drei Probleme mit
> einem Modell: (1) den Kontext-Hänger bei langen Sitzungen / großen Notizen
> (Immich-Fall — bisher nur durch Rate-Limit-Handling *entschärft*, nicht
> gelöst), (2) den Verlust des Kommando-Protokolls bei Truncation, (3) die
> fehlende Grundlage für die wertvollsten Pro-Features (Historie, Report,
> Audit-Log — die bauen alle hierauf auf).
>
> **Priorität ERHÖHT** (Kern-Datenmodell, Redaction-relevant, Migration).
> Großer Umbau — in Etappen umzusetzen (siehe Reihenfolge am Ende).

## Getroffene Design-Entscheidungen (Stefan)

1. **Kompaktierungs-Auslöser**: Token-Grenze (proaktiv, ~70–80 % des
   Modell-Kontextfensters, **Post-Fencing gerechnet**) + **N letzte Runden
   als Minimum** (immer voll erhalten) + Einzel-Output-Kürzung für
   pathologische Fälle.
2. **Notiz**: Beim Kompaktieren **verkürzt gesendet**, die **gespeicherte
   Notiz bleibt vollständig**. Scope-priorisiert (server-spezifisch bleibt am
   längsten). Kürzung der Notiz erst als **letztes Mittel** nach der
   Chat-History. Am Sitzungsende bei großen Notizen **Vorschlag zur
   dauerhaften Kürzung** („Ja" / „mache ich selber").
3. **Ledger**: **Alles** rein (Kommandos, Ergebnisse, Freigaben/Ablehnungen,
   KI-Nachrichten). **Redigiert vor dem Schreiben.** Append-only, nie
   zusammengefasst.
4. **Summary**: Eigener KI-Aufruf beim Kompaktieren, **mit Fallback**
   (Ausfall → Runden abschneiden statt hängen). Nutzt Rate-Limit-Handling +
   Body-Timeout.
5. **Migration**: **Keine Daten-Migration.** Additive Schema-Migration (neue
   Tabellen), alte Sessions bleiben unberührt, nur neue nutzen das Modell.

## 1. Das Ledger (append-only, dauerhaft, redigiert)

Ein **append-only Protokoll** aller Vorgänge einer Session — die dauerhafte
Wahrheit, aus der später Audit-Log und Report schöpfen.

### 1.1 Was reinkommt (alles)
Pro Eintrag: Zeitstempel, Session-ID, **Quelle** (`user`/`ai`/`mcp-agent` —
für spätere Audit-„wer"-Unterscheidung), Typ und Inhalt:
- **Kommando vorgeschlagen** (von wem: KI/MCP/manuell) + der Kommandotext.
- **Freigabe-Entscheidung**: bestätigt / abgelehnt / per Regel automatisch
  (welche Regel — aus `evaluate_explained`).
- **Kommando ausgeführt** + **Ergebnis** (stdout/stderr/exit — **redigiert**).
- **KI-Nachricht** (die Antworten der KI).
- Später erweiterbar (Dateibrowser-Aktionen etc. — die 0054-Aktionen sind
  bereits „audit-erfassbar" gebaut).

### 1.2 Redaction — Pflicht
**Jeder Ledger-Eintrag wird redigiert, bevor er persistiert wird** — dieselbe
`redactor.redact()` wie beim KI-Kontext. Der Ledger persistiert dauerhaft;
ohne Redaction wäre er eine Klartextsammlung sensibler Ausgaben auf der
Platte. (Die Redaction-Härtung — Shadow, DB-Strings, Tokens — schützt damit
auch den Ledger.)

### 1.3 Verschlüsselung
Der Ledger wird wie die Chat-Historie **verschlüsselt** persistiert (Spec
0036, chat-content encryption key). Konsistent mit „sensibler Inhalt liegt
nie im Klartext in der DB".

### 1.4 Persistenz
Neue Tabelle(n) in `persistence-sqlite`, **additive** Migration. Append-only:
Einträge werden nie geändert/gelöscht (außer beim Löschen der ganzen Session
— Cascade wie bei anderen Session-Daten).

## 2. Die Summary (KI-Zusammenfassung, mit Fallback)

Wenn beim Kompaktieren alte Runden aus dem **KI-Kontext** entfernt werden,
ersetzt eine **Zusammenfassung** sie — damit die KI die Kontinuität behält,
ohne die vollen alten Runden mitzuschicken.

### 2.1 Erzeugung
- **Eigener KI-Aufruf** beim Kompaktieren: „Fasse die bisherige Konversation
  bündig zusammen (was wurde getan, welcher Stand)." Über den bestehenden
  Provider, mit **Rate-Limit-Handling (0051) + Body-Timeout**.
- Die neue Summary fasst die **bisherige Summary + die jetzt zu
  komprimierenden Runden** zusammen (rollierend — nicht jedes Mal von vorne).

### 2.2 Fallback — KRITISCH
Schlägt der Summary-Aufruf fehl (Rate-Limit trotz Retry, Timeout, Fehler),
**darf die Sitzung nicht hängen/abbrechen.** Fallback: die ältesten Runden
werden **ohne** Summary abgeschnitten, mit einem sichtbaren Hinweis im Kontext
(„ältere Konversation gekürzt"). Die Summary ist eine *Verbesserung*, ihr
Ausfall blockiert nie die Grundfunktion (Kontext klein genug halten).
„Fehler containen" — dieselbe Invariante wie beim Body-Timeout-Fix.

### 2.3 Persistenz
Die aktuelle Summary wird mit der Session persistiert (verschlüsselt), damit
sie bei Resume verfügbar ist.

## 3. Kompaktierung (der Auslöser + der Ablauf)

### 3.1 Auslöser
**Vor jedem Provider-Request** wird die geschätzte Request-Größe berechnet
(**Post-Fencing**, weil `<`/`>`/`&` zu 4–5-Zeichen-Entitäten expandieren —
das war ein Diagnose-Fund). Überschreitet sie **~70–80 % des
Modell-Kontextfensters** → kompaktieren, *bevor* das harte Limit kommt (Puffer
für die Antwort).

### 3.2 Ablauf (Kürzungs-Reihenfolge — wichtig)
Kompaktiere in dieser Reihenfolge, bis der Request unter der Grenze ist:
1. **Alte Chat-Runden** → durch die Summary ersetzen (§2). Die **letzten N
   Runden** (z. B. 3) bleiben IMMER voll erhalten.
2. **Einzelne Riesen-Ausgaben** in den erhaltenen Runden kürzen (mit Hinweis
   „Ausgabe gekürzt — vollständig im Ledger"). Das fängt den Fall, dass eine
   *einzelne* jüngste Runde schon zu groß ist (2-MB-Output), ohne die ganze
   Runde zu opfern.
3. **Die Notiz verkürzt senden** (§4) — erst als letztes Mittel.

### 3.3 Was NICHT kompaktiert wird
Der **Ledger** — der behält immer alles (er geht ja auch nicht in den
KI-Request, er ist die dauerhafte Wahrheit). Kompaktierung betrifft nur den
**an die KI gesendeten Kontext**, nie den Ledger.

## 4. Notiz-Handling

### 4.1 Verkürzt senden (automatisch, verlustfrei für die gespeicherte Notiz)
Kommt die Kompaktierung bis zur Notiz (Schritt 3.2.3), wird eine **gekürzte
Fassung an die KI gesendet** — die **gespeicherte Notiz bleibt unangetastet**.
Priorisierung nach **Scope**: server-spezifische Notiz bleibt am längsten,
gruppen-/globale werden zuerst gekürzt (ADR 0003/0004). Die Kürzung ist
lautlos (unterbricht die Arbeit nicht mit Dialogen).

### 4.2 Vorschlag zur dauerhaften Kürzung (am Sitzungsende, Nutzer entscheidet)
Ist die Notiz **groß** (Schwellwert), beim **Verbindungsende** ein Dialog:
„Deine Notiz für diesen Server ist sehr groß und kann bei langen Sitzungen
gekürzt werden müssen. Soll ich sie zusammenfassen?" mit **„Ja,
zusammenfassen"** / **„Mache ich selbst"** (→ öffnet die Notiz-Bearbeitung).
- „Ja" → ein KI-Aufruf fasst die **gespeicherte** Notiz zusammen, zeigt das
  Ergebnis im **Diff-Bestätigungsdialog** (Spec 0003/0023 — der Nutzer sieht,
  was die neue Notiz wird, bevor sie ersetzt wird). Kein automatisches
  Überschreiben.
- **Kein stilles Kürzen der gespeicherten Notiz** — immer Nutzer-Bestätigung.
- (Optional, Bonus: ein sanfter Hinweis schon **beim Bearbeiten** einer
  ungewöhnlich großen Notiz. Nicht zwingend für die erste Umsetzung.)

## 5. Migration

**Keine Daten-Migration.** Neue Tabellen (Ledger, Summary) werden **additiv**
angelegt (Schema-Migration wie 0004/0047 — vorwärts, datenerhaltend). Alte
Sessions in der bestehenden Struktur bleiben **unberührt und lesbar** (öffnen/
ansehen geht weiter). Nur **neue** Sessions führen ab dem Umstieg das Ledger
und nutzen die Kompaktierung. Kein Parsing/Umschreiben alter Daten.

## 6. Invarianten / Sicherheit

- **Ledger redigiert vor dem Persistieren** — kein Klartext-Secret dauerhaft
  auf der Platte.
- **Ledger verschlüsselt** (wie Chat-Historie, 0036).
- **Kompaktierung betrifft nur den KI-Kontext, nie den Ledger** (die
  dauerhafte Wahrheit bleibt vollständig).
- **Gespeicherte Notiz wird nie ohne Nutzer-Bestätigung verändert** (das
  verkürzte Senden ist verlustfrei; die dauerhafte Kürzung braucht den
  Diff-Dialog).
- **Summary-Ausfall blockiert die Sitzung nie** (Fallback: Runden
  abschneiden).
- Der Summary-KI-Aufruf unterliegt Rate-Limit-Handling + Body-Timeout (nicht
  ungeschützt).
- Additive Migration bricht keine bestehenden Sessions.

## 7. Testbarkeit

- Token-Schätzung inkl. Fencing-Expansion (Post-Fencing-Volumen korrekt).
- Kompaktierung greift bei ~70–80 %, letzte N Runden bleiben voll.
- Riesen-Einzel-Output → gekürzt mit Ledger-Hinweis, Runde nicht verworfen.
- Notiz verkürzt gesendet, gespeicherte Notiz unverändert (verifizieren).
- Summary-Erzeugung: rollierend; **Fallback bei Aufruf-Fehler** (Sitzung
  läuft weiter, Hinweis statt Hang) — Regressionstest.
- Ledger: alles erfasst, **redigiert** (Fake-Secret in einer Ausgabe → im
  Ledger redigiert), append-only, verschlüsselt.
- Sitzungsende-Notiz-Dialog: erscheint bei großer Notiz, „Ja" → Diff-Dialog,
  „Mache ich selbst" → Bearbeitung; gespeicherte Notiz nie ohne Bestätigung
  geändert.
- Migration: additiv, alte Sessions bleiben lesbar.

## 8. Reihenfolge der Umsetzung (Etappen)

Großer Umbau — in überprüfbaren Etappen, jede mit grünem Gate:

1. **Ledger-Grundgerüst**: Tabellen (additive Migration), Schreiben aller
   Ereignistypen, Redaction + Verschlüsselung. Noch ohne Kompaktierung/
   Summary — nur das dauerhafte Protokoll. (Testbar isoliert.)
2. **Token-Schätzung** (Post-Fencing) + der Kompaktierungs-Auslöser, zunächst
   mit **einfacher Kürzung** (Runden abschneiden ohne Summary, Notiz verkürzt
   senden). Löst schon den Immich-Hänger.
3. **Summary** (KI-Aufruf + Fallback) — ersetzt das simple Runden-Abschneiden
   durch die Zusammenfassung.
4. **Sitzungsende-Notiz-Dialog** (Vorschlag + Diff-Bestätigung).
5. (Später/separat) Bonus-Hinweis beim Bearbeiten großer Notizen.

**Etappe 1 + 2 lösen zusammen den akuten Kontext-Hänger** und legen die
Ledger-Grundlage — das ist der wertvollste erste Block. Summary (3) und
Notiz-Dialog (4) sind Verbesserungen darauf.

Sag mir, ob du das als **ein großes Paket** (alle Etappen, mehrere Commits)
oder **etappenweise** (erst 1+2, dann 3, dann 4) umgesetzt haben willst — bei
diesem Umfang und der Migration würde ich zu Etappen raten.
