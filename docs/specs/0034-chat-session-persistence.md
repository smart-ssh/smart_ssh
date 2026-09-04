# Spec: Persistente, fortsetzbare KI-Chat-Sitzungen

Status: Entwurf
Modul: Schema-Erweiterung `persistence-sqlite`, Erweiterung `crates/app-tauri`,
`frontend/`
Abhängigkeiten: Kernschleife (Spec 0007/0021), KI-Provider (Spec 0006,
Abschnitt 8 — löst den dortigen offenen Punkt zur Kontext-Kürzung),
Redactor (Spec 0006, Abschnitt 5), Notiz-Vorschlag-Trigger-Mechanismus
(Spec 0010, wiederverwendet für Titel-Generierung), MCP-Server (Spec 0028,
Abschnitt 3 — MCP-Sitzungen bleiben ausdrücklich ausgenommen)

## 1. Ziel

Chat-Verläufe werden dauerhaft gespeichert (nicht nur im Arbeitsspeicher der
laufenden `Session`, Spec 0007 Abschnitt 3) und lassen sich beim erneuten
Verbinden zu einem Server fortsetzen — nach demselben Grundprinzip, wie es
Claude Code selbst löst: **lokale Speicherung des vollständigen Verlaufs,
kein Provider-seitiger Session-Mechanismus** (recherchiert, siehe
Diskussion — Anthropic-, OpenAI- und kompatible APIs sind zustandslos, jede
Anfrage muss den Verlauf selbst mitschicken).

## 2. Schema-Erweiterung (`persistence-sqlite`)

```sql
-- migrations/000X_chat_sessions.sql

CREATE TABLE chat_sessions (
    id              TEXT PRIMARY KEY,
    server_id       TEXT NOT NULL REFERENCES servers(id) ON DELETE CASCADE,
    title           TEXT,             -- NULL bis automatisch generiert
    started_at      TEXT NOT NULL,
    ended_at        TEXT,             -- NULL während aktiv/laufend
    ai_provider_id  TEXT REFERENCES ai_provider_configs(id) ON DELETE SET NULL
);

CREATE INDEX idx_chat_sessions_server ON chat_sessions(server_id, started_at);

CREATE TABLE chat_messages (
    id              TEXT PRIMARY KEY,
    session_id      TEXT NOT NULL REFERENCES chat_sessions(id) ON DELETE CASCADE,
    role            TEXT NOT NULL CHECK (role IN ('user', 'assistant', 'action_result')),
    content_type    TEXT NOT NULL CHECK (
        content_type IN ('text', 'command_result', 'action_rejected', 'document')
    ),
    content         TEXT NOT NULL,    -- serialisierter MessageContent, siehe Abschnitt 3
    sequence        INTEGER NOT NULL, -- Reihenfolge innerhalb der Sitzung
    created_at      TEXT NOT NULL
);

CREATE INDEX idx_chat_messages_session ON chat_messages(session_id, sequence);
```

`ai_provider_id` ist rein informativ ("mit welchem Provider begann diese
Sitzung") — beim Fortsetzen ist **nicht** derselbe Provider erforderlich,
der Textverlauf ist providerunabhängig (Abschnitt 5).

## 3. Was gespeichert wird — redigiert, nicht roh

Der `content`-Wert entspricht exakt dem, was tatsächlich durch den
`OutputRedactor` (Spec 0006, Abschnitt 5) gelaufen ist, **nicht** dem
ungefilterten Rohinhalt — dieselbe Konsistenzregel wie bereits beim
strukturierten Logging (Spec 0016) festgelegt. Damit sammeln sich nicht
zusätzlich unredigierte Secrets in der lokalen DB an, über das hinaus, was
ohnehin schon an die KI ging.

## 4. Sitzungs-Lebenszyklus

Eine Sitzung beginnt bei `connect()`, endet bei `disconnect()` — analog zu
Claude Codes "eine Session pro Arbeitsblock"-Modell, nicht eine einzige
endlose Historie pro Server.

- Jede Nachricht (Nutzer-Text, KI-Antwort, Aktionsergebnis, Ablehnung gemäß
  Spec 0021) wird **fortlaufend** geschrieben, sobald sie entsteht — kein
  Sammeln bis zum Verbindungsende. Ein Absturz mitten in der Sitzung verliert
  damit höchstens die letzte, noch nicht abgeschlossene Nachricht.
- Beim Fortsetzen einer gespeicherten Sitzung (Abschnitt 6) wird **dieselbe**
  `chat_sessions`-Zeile weiterverwendet (`ended_at` wird auf `NULL`
  zurückgesetzt, `sequence` läuft weiter hoch) — kein Kopieren in eine neue
  Zeile. Der Verlauf bleibt ein durchgehender Thread über mehrere
  Verbinden/Trennen-Zyklen hinweg.

## 5. "Fortsetzbar" — bewusst ohne Ablaufdatum

Da es kein Provider-Konzept für Session-Ablauf gibt, verfällt eine Sitzung
**nicht automatisch nach Zeit**. Eine Sitzung gilt als fortsetzbar, wenn:

1. sich die gespeicherte Historie laden lässt (Integritätsprüfung, keine
   korrupten Daten), und
2. mindestens ein aktiver KI-Provider konfiguriert ist — nicht zwingend
   derselbe, der ursprünglich genutzt wurde.

Kein Alters-Grenzwert, der eine Sitzung blockiert. Stattdessen: eine
**optionale Aufbewahrungs-Einstellung** (`chat_session_retention_days:
Option<u32>`, Default `None` = niemals automatisch löschen), global über
`tauri-plugin-store` (Spec 0024-Muster) gespeichert, nicht pro Server. Ist
sie gesetzt, räumt ein Hintergrund-Job beim App-Start Sitzungen auf, deren
`ended_at` älter als der konfigurierte Zeitraum ist (samt zugehöriger
Nachrichten über `ON DELETE CASCADE`).

## 6. UI-Ablauf beim Verbinden

Beim Klick auf einen Server mit vorhandener Sitzungshistorie erscheint ein
leichter Auswahl-Screen statt direkt zu verbinden:

- **"Neue Unterhaltung"** — prominent, Standardaktion (Enter-Taste),
  erzeugt eine neue `chat_sessions`-Zeile.
- **Liste vergangener Sitzungen** darunter, neueste zuerst: Titel (Abschnitt
  7), Zeitpunkt, Nachrichtenanzahl. Klick lädt die gespeicherte Historie in
  den `SessionContext` und setzt die Sitzung fort (`resume_chat_session`,
  Abschnitt 8).
- Hat ein Server noch keine gespeicherte Sitzung: kein Auswahl-Screen,
  direktes Verbinden wie bisher.

## 7. Automatische Kurztitel

Beim Verbindungsende (`disconnect()`) wird — sofern die Sitzung mindestens
eine Nutzer-Nachricht enthält und noch keinen Titel hat — ein kurzer Titel
(2–4 Wörter) generiert. Wiederverwendet denselben Trigger-Mechanismus wie
der Notiz-Vorschlag beim Beenden (Spec 0010, Abschnitt 2): ein gezielter,
minimaler KI-Aufruf, hier aber ohne Tool-Schema — reine Textanfrage ("Fasse
den Zweck dieser Unterhaltung in 2–4 Worten zusammen"), Antworttext wird
direkt (defensiv auf sinnvolle Länge begrenzt) als Titel übernommen.

- Ein einmal automatisch gesetzter Titel wird **nicht** bei jedem weiteren
  Trennen erneut überschrieben — nur beim allerersten Mal, danach bleibt er
  stabil (kann sich sonst bei jedem Fortsetzen ändern, verwirrend für den
  Wiedererkennungswert).
- `rename_chat_session(session_id, new_title)` erlaubt manuelles
  Umbenennen, überschreibt den automatischen Titel dauerhaft.

## 8. Commands

```
list_chat_sessions(server_id) -> Vec<ChatSessionSummaryDto>
resume_chat_session(server_id, session_id) -> SessionId
rename_chat_session(session_id, new_title)
delete_chat_session(session_id)
```

`resume_chat_session` baut wie ein normaler `connect()` die SSH-Verbindung
auf (inkl. ggf. Host-Key-Bestätigung), lädt zusätzlich die gespeicherte
Historie in den `SessionContext` — inklusive der Kontext-Kürzung aus
Abschnitt 9, falls nötig.

## 9. Kontext-Kürzung beim Laden (löst Spec 0006, Abschnitt 8)

Eine über Tage/Wochen fortgesetzte Sitzung kann eine Historie ansammeln, die
das Kontextfenster/Budget sprengt. Beim Laden (und vor jedem
`AiProvider::send()`-Aufruf) gilt eine einfache, zeichenbasierte
Näherung statt eines exakten, providerspezifischen Tokenizers:
konfigurierbares Zeichen-Budget (Default z. B. 40.000 Zeichen), beim
Überschreiten werden die **ältesten** Nachrichten zuerst verworfen, bis der
verbleibende Verlauf unter das Budget passt. Kein Zusammenfassen/
Komprimieren älterer Nachrichten in dieser Spec — reines Kürzen, einfach
und vorhersehbar, auch wenn dadurch älterer Kontext verloren geht (bewusste
MVP-Vereinfachung, wie ursprünglich in Spec 0006 skizziert).

## 10. Abgrenzung zu MCP (Spec 0028)

MCP-ausgelöste Aktionen erzeugen **keine** `chat_sessions`-Einträge — sie
laufen bereits laut Spec 0028, Abschnitt 3 außerhalb der
Turn-Fortsetzungslogik und ohne eigenen Chatverlauf. Diese Abgrenzung bleibt
unverändert bestehen.

## 11. Offene Punkte

- Soll es einen "Neuer Chat"-Button **innerhalb** einer bereits verbundenen
  Session geben (ohne die SSH-Verbindung zu trennen), um mitten in der
  Nutzung das Thema zu wechseln? Naheliegend, aber nicht Teil dieser Spec —
  würde eine weitere `chat_sessions`-Zeile erzeugen, während die
  SSH-Verbindung/der Tab bestehen bleibt, technisch unproblematisch, aber
  bewusst zurückgestellt, bis der Bedarf sich zeigt.
- Zusammenfassen statt reinem Kürzen alter Nachrichten (Abschnitt 9) wäre
  eine spätere Verbesserung, sobald sich das reine Kürzen in der Praxis als
  zu verlustreich erweist.
