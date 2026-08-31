# Spec: Chat-Prompt-Historie (Pfeiltasten-Navigation)

Status: Entwurf
Modul: Erweiterung `persistence-sqlite`, `crates/app-tauri`, `frontend/`
Abhängigkeiten: Chat-Kernschleife (Spec 0007, Abschnitt 6)

## 1. Ziel

Im Chat-Eingabefeld lassen sich zuletzt an einen bestimmten Server gesendete
Prompts per Pfeiltaste-nach-oben durchblättern (jüngster zuerst), analog zum
Befehlsverlauf einer Shell — Pfeiltaste-nach-unten navigiert wieder zurück
Richtung aktuellem/neuestem Eintrag. Die Historie ist **pro Server**
gespeichert, nicht global und nicht nur pro laufender Session — sie
überlebt Verbindungstrennung und App-Neustart.

## 2. Schema-Erweiterung (`persistence-sqlite`)

```sql
-- migrations/0004_prompt_history.sql

CREATE TABLE prompt_history (
    id          TEXT PRIMARY KEY,
    server_id   TEXT NOT NULL REFERENCES servers(id) ON DELETE CASCADE,
    content     TEXT NOT NULL,
    created_at  TEXT NOT NULL
);

CREATE INDEX idx_prompt_history_server ON prompt_history(server_id, created_at);
```

`ON DELETE CASCADE`: Wird ein Server gelöscht, ist seine Prompt-Historie
mit ihm bedeutungslos, sie wird automatisch mitentfernt.

Kein neues Sicherheits-/Verschlüsselungsthema — Prompt-Inhalte fallen unter
dieselbe bereits getroffene Annahme wie Notizen und der Rest der DB (Spec
0004, Abschnitt 7: OS-Festplattenverschlüsselung vorausgesetzt, keine
zusätzliche Verschlüsselung dieser Tabelle).

## 3. Speicherung neuer Einträge

Kein eigener Speichern-Command nötig — jeder Aufruf von
`send_chat_message(session_id, text)` (Spec 0007) schreibt den gesendeten
Text zusätzlich in `prompt_history`, verknüpft mit dem `server_id` der
Session. Aufeinanderfolgende **identische** Prompts (z. B. zweimal
hintereinander exakt derselbe Text) werden nicht doppelt gespeichert,
sondern nur der bestehende jüngste Eintrag bleibt bestehen — vermeidet
unnötiges Aufblähen der Historie.

Begrenzung: pro Server werden maximal die letzten 200 Einträge behalten,
ältere werden beim Einfügen eines neuen Eintrags automatisch entfernt.

## 4. Tauri-Command

```
list_prompt_history(server_id: ServerId) -> Vec<String>
```

Liefert die gespeicherten Prompts in chronologischer Reihenfolge (älteste
zuerst) — das Frontend kehrt die Reihenfolge für die Navigation selbst um
oder greift von hinten zu, je nach Implementierungspräferenz.

## 5. Navigations-Verhalten im Chat-Eingabefeld

Das Eingabefeld ist ein mehrzeiliges Textfeld (Spec 0007), daher muss die
Pfeiltasten-Navigation gezielt von normaler Cursor-Bewegung innerhalb eines
mehrzeiligen Entwurfs unterschieden werden:

- **Pfeil-nach-oben** löst Historien-Navigation nur aus, wenn sich der
  Cursor an **Position 0** des Textfelds befindet (Anfang der ersten
  Zeile) — sonst bewegt sich der Cursor wie gewohnt innerhalb des Texts.
- **Pfeil-nach-unten** löst Historien-Navigation nur aus, wenn sich der
  Cursor am **Ende des Textfelds** befindet — sonst normale Cursor-Bewegung.
- Wird zum ersten Mal in der aktuellen Eingabe nach oben navigiert: der
  aktuell eingegebene, noch nicht gesendete Text (Entwurf) wird
  zwischengespeichert, das Feld zeigt den jüngsten Historieneintrag.
  Weiteres Pfeil-nach-oben zeigt jeweils den nächstälteren Eintrag, bis der
  älteste erreicht ist (dort bleibt es stehen, keine Fehlermeldung).
- Pfeil-nach-unten bewegt sich in umgekehrter Richtung; wird über den
  jüngsten Historieneintrag hinaus navigiert, erscheint wieder der
  ursprüngliche zwischengespeicherte Entwurf (auch falls dieser leer war).
- **Bewusste MVP-Vereinfachung**: Jede Texteingabe (Tippen) außerhalb der
  Pfeiltasten-Navigation selbst beendet den Navigations-Modus — ein erneutes
  Pfeil-nach-oben beginnt wieder beim jüngsten Eintrag, statt an der zuvor
  erreichten Position fortzusetzen. Das ist einfacher und vorhersehbarer als
  volles Readline-Verhalten (bei dem bearbeitete Historieneinträge sich wie
  ein neuer Zwischenzustand verhalten), für den beschriebenen Anwendungsfall
  aber ausreichend.
- Die Historie wird beim Öffnen/Wechseln zu einem Server-Chat einmalig über
  `list_prompt_history` geladen und im Frontend-State für die laufende
  Session gehalten — kein wiederholtes Nachladen bei jeder Pfeiltaste.

## 6. Offene Punkte

- Keine UI zum manuellen Löschen/Durchsuchen der Historie in dieser Spec —
  nur die Pfeiltasten-Navigation selbst. Eine durchsuchbare Historienansicht
  (ähnlich Shell `Ctrl+R`) wäre eine naheliegende spätere Ergänzung, aber
  nicht Teil dieses Schritts.
