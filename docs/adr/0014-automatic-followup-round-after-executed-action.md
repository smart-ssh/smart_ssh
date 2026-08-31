# 0014-automatic-followup-round-after-executed-action

## Status
Accepted

## Kontext

Beim Verdrahten der Kernschleife aus `docs/specs/0007-tauri-app-mvp.md`
(Abschnitt 6) wurde `run_chat_turn` ursprünglich so gebaut, dass ein Aufruf
**genau eine** KI-Antwortrunde abdeckt (0..n `TextDelta`s, 0..n
`ActionProposed`s, dann `Done`) und danach zurückkehrt — auch wenn dabei
eine Aktion ausgeführt und ihr Ergebnis in `context.history` übernommen
wurde. Die Begründung damals: "für die nächste KI-Runde" (Abschnitt 6,
Punkt 5 der Spec-Skizze) wurde als "beim nächsten `send_chat_message`-Aufruf
des Nutzers" gelesen, nicht als impliziter Auto-Weiterlauf — aus Sorge, ein
automatischer Agenten-Loop könnte der im Projekt durchgehaltenen
Transparenz-/Bestätigungs-Philosophie (Spec 0002, Spec 0007 Abschnitt 5)
widersprechen.

In der Praxis führte das zu genau dem Verhalten, das ein Nutzer beim
Testen bemängelte: nachdem die KI ein Kommando vorgeschlagen hat und es
(automatisch oder nach Bestätigung) ausgeführt wurde, endete der Chat-Turn
sofort danach. Sichtbar war nur der rohe Kommando-Output
(`ActionResultPayload::Command`) — nie eine Antwort der KI, die dieses
Ergebnis tatsächlich interpretiert ("die Festplatte ist zu 80% voll, das
größte Verzeichnis ist ..."). Für ein Chat-Interface ist das nicht
nachvollziehbar: der Nutzer erwartet, dass die KI auf das Ergebnis ihres
eigenen Vorschlags eingeht, ohne dass er von sich aus nachfragen muss.

## Entscheidung

`run_chat_turn` (`crates/app-tauri/src/orchestration.rs`) läuft jetzt in
bis zu `MAX_AUTO_FOLLOWUP_ROUNDS` (ursprünglich 8, siehe Revision unten:
25) Runden. Jede Runde
(`run_one_round`) entspricht weiterhin genau einem `AiProvider::send()`-
Aufruf und gibt zurück, ob dabei **mindestens eine Aktion tatsächlich
ausgeführt** wurde (AutoExec, oder vom Nutzer per `respond_to_action` mit
`Approve`/`EditThenApprove` bestätigt — ausdrücklich **nicht** bei `Deny`,
weder dem direkten noch dem durch die Filter-Engine erneut blockierten
`EditThenApprove`-Fall, und nicht bei einem fehlgeschlagenen
`SshTransport::execute()`/`ProfileStore::record_note_revision()`, da der
Kontext dann unverändert bliebe). Nur wenn eine Runde tatsächlich etwas
ausgeführt hat, folgt automatisch eine weitere Runde mit dem inzwischen
erweiterten `SessionContext`; sonst kehrt `run_chat_turn` zurück, exakt wie
vorher.

Die ursprüngliche Sorge um die Transparenz-/Bestätigungs-Philosophie bleibt
gewahrt: der Automatismus betrifft ausschließlich den *Rückruf* an die KI,
nicht die Bestätigungspflicht. Jede in einer Folgerunde neu vorgeschlagene
Aktion durchläuft erneut dieselbe `FilterEngine`/Confirm-Logik wie jede
andere — eine KI kann sich in einer Folgerunde nicht selbst mehr
Berechtigungen verschaffen, als sie in der ersten Runde hätte. Die feste
Rundenobergrenze verhindert, dass eine KI, die immer wieder neue Aktionen
vorschlägt (fehlerhaft oder pathologisch), den Turn unbegrenzt am Laufen
hält; wird sie erreicht, bricht `run_chat_turn` mit einem `chat-error`-
Event ab, statt weiter zu warten oder endlos Aktionen vorzuschlagen.

## Konsequenzen

**Positiv:**
- Der Nutzer bekommt nach einem ausgeführten Kommando tatsächlich eine
  Antwort der KI, die das Ergebnis einordnet — nicht nur den rohen Output.
- Mehrstufige Aufgaben ("prüfe den Speicherplatz, und wenn er über 80% ist,
  räume `/var/log` auf") funktionieren jetzt über mehrere Kommandos hinweg,
  ohne dass der Nutzer nach jedem Schritt manuell nachfragen muss — jeder
  einzelne Schritt bleibt dabei genauso bestätigungspflichtig wie zuvor.
- Kein Verhaltensunterschied für Turns ohne ausgeführte Aktion (reiner Text,
  `Deny`, oder eine Aktion, die auf eine nie beantwortete Bestätigung
  wartet) — dort bricht die Schleife weiterhin nach der ersten Runde ab.

**Negativ / Trade-off:**
- Ein Chat-Turn kann jetzt spürbar länger laufen (bis zu
  `MAX_AUTO_FOLLOWUP_ROUNDS` aufeinanderfolgende KI-Aufrufe plus
  Ausführungen) als der Klick auf "Senden" — gemildert durch die in
  derselben Sitzung ergänzte Ladeanzeige (`docs/specs/0007-tauri-app-mvp.md`,
  Abschnitt 7).
- `MockAiProvider` in den Orchestrierungs-Tests musste von einer einzelnen,
  bei jedem `send()`-Aufruf wiederholten Event-Sequenz auf eine
  Runden-Warteschlange (mit `[Done]` als Fallback nach Erschöpfung)
  umgestellt werden, um Folgerunden gezielt testen zu können, ohne
  bestehende Single-Round-Tests anzupassen.

## Revision (nach erstem Praxiseinsatz)

Die ursprüngliche Grenze von 8 Runden erwies sich als zu niedrig: eine
völlig legitime, mehrstufige Admin-Aufgabe (mehrere aufeinanderfolgende
Diagnose-/Fix-Kommandos) lief dagegen und wurde mit der alarmierend
wirkenden Fehlermeldung "Abgebrochen nach 8 aufeinanderfolgenden Aktionen"
abgebrochen, obwohl nichts fehlgelaufen war. Die einzelnen Kommandos waren
dabei jeweils bereits durch die Filter-Engine/Bestätigungslogik abgesichert
(s. oben) — die Rundenzahl selbst ist kein primärer Sicherheitsmechanismus,
sondern nur ein zusätzliches Netz gegen eine KI, die (fehlerhaft)
unbegrenzt weiter automatisch ausführbare Aktionen vorschlägt. Die Grenze
wurde deshalb auf **25** angehoben — großzügig genug, um mehrstufige
Admin-Aufgaben nicht zu stören, aber weiterhin endlich, falls eine KI
tatsächlich in eine Wiederholungsschleife gerät. Bleibt der Wert weiterhin
zu niedrig, ist der nächste Schritt eine erkennungsbasierte Grenze (z. B.
Abbruch nach N identischen/sehr ähnlichen aufeinanderfolgenden Kommandos
statt einer festen Rundenzahl) statt eines erneuten reinen Zahlen-Bumps.
