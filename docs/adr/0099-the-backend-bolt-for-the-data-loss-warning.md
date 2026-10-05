# ADR 0099 — Der Backend-Riegel für die Warnung aus A13/E10

Status: akzeptiert
Betrifft: Spec 0101 (A13, A20, E10, Klarstellung 12), ADR 0098 §6.1

Klarstellung 12 (Q-BL-0314-02) schließt den Punkt, der in ADR 0098 §6.1
offen blieb: Die ausdrückliche Bestätigung der Warnung „ohne Passwort sind
alle Daten verloren, keine Wiederherstellung" lag nur in der Oberfläche.
Länge und Wiederholung prüft das Backend seit Commit 10 doppelt, mit der
Begründung „eine Prüfung, die nur dort steht, umgeht ein Kommandoaufruf" —
für die Bestätigung galt dieselbe Begründung, und es gab keinen Riegel.
Dieses Dokument hält fest, wo er jetzt sitzt und welche Entscheidungen
dabei gefallen sind.

## 1. Ein Riegel, nicht drei

A13 nennt drei Einrichtungswege: aus den Einstellungen, aus D1, und im
Passwort-Modus aus A5/D4. Alle drei gehen durch **eine** Funktion,
`app_logic::master_password::set_up_master_password` — dort sitzt der
Riegel, als erste Anweisung, vor der Prüfung von Länge und Wiederholung und
vor jedem Schreibzugriff **dieser Funktion**.

Das genügt für zwei der drei Wege, aber nicht für den dritten: Auf den
Wegen über A5 und D4 liegt zwischen der Passworteingabe und dem Einrichten
ein `rename` — Datenbank und alte Verpackung werden zur Seite gelegt, weil
die Reihenfolge das verlangt (ADR 0095 §4). Ein Riegel erst im Einrichten
hätte dort abgelehnt, *nachdem* schon etwas verändert war, und zurück
geblieben wäre ein Datenverzeichnis ohne Verpackungsdatei — beim nächsten
Start also der Schlüsselbund-Modus, ein Moduswechsel, den niemand gewählt
hat (spec-reviewer Runde 2).

Deshalb gibt es `check_new_password_before_touching_files`: dieselben
beiden Prüfungen, aufrufbar **vor** dem ersten `rename`, und an beiden
Stellen aufgerufen (`database_startup::start_over` und
`database_startup::generate_key`, direkt hinter
`ask_for_new_master_password()`). Sie **ersetzt** den Riegel im Einrichten
nicht, sie kommt davor; beide rufen dieselben beiden Funktionen auf
(`check_loss_warning`, `check_new_password`), damit sie nicht
auseinanderlaufen können. Eine doppelte Prüfung kann per Konstruktion nicht
weniger erkennen als eine.

Die Alternative wäre gewesen, jeden der drei Wege einzeln zu prüfen. Sie
ist verworfen: Ein vierter Weg, der später entsteht, hätte dann einen
Riegel zu ergänzen, an den niemand denkt. So kann er es nicht — die
Funktion verlangt die Antwort als Parameter.

Das Kommando `set_up_master_password` in `app-shell` lehnt **zusätzlich**
schon vor dem Lesen des Schlüsselbunds ab. Das ist keine zweite Wahrheit,
sondern dieselbe: „verändert nichts" fängt beim Nichtstun an, und ohne
diese Vorprüfung hinge die Fehlermeldung eines unbestätigten Aufrufs davon
ab, ob der Schlüsselbund gerade erreichbar ist.

## 2. Ein Typ, kein `bool`

`LossWarning::{NotConfirmed, ConfirmedByTheUser}` statt eines `bool` — aus
demselben Grund wie `KeychainOverwrite` (Klarstellung 10b): An der
Aufrufstelle soll stehen, *was* bestätigt wurde, und die Voreinstellung
muss die vorsichtige sein. Die Bestätigung reist durch drei Schichten
(IPC → `NewMasterPassword` → `set_up_master_password`); ein `bool` wäre
dort an jeder Grenze ein Wert, den man versehentlich weglässt.

Am IPC bleibt es notwendigerweise ein `bool` — mehr kann über die Grenze
nicht kommen. Die Umwandlung liegt an genau einer Stelle
(`commands::master_password::loss_warning`), und der vorsichtige Fall ist
dort der Standardzweig: Ein fehlendes Feld in `answer_startup_prompt`
(`Option<bool>` → `unwrap_or(false)`) heißt *nicht bestätigt*.

## 3. Die Bestätigung liegt im selben Wert wie das Passwort

In `NewMasterPassword` steht sie als Feld, nicht als zweiter, getrennt
hinterlegter Wert in `WindowStartupPrompt`. Dadurch wird sie mit dem
Passwort zusammen gesetzt, zusammen geleert (Klarstellung 10e) und
zusammen entnommen — eine Bestätigung aus einem früheren, abgebrochenen
Versuch kann nicht an ein neues Passwort geraten. Zwei getrennte Felder
hätten genau diesen Weg eröffnet.

## 4. Ein eigener Fehlercode (A20)

`MASTER_PASSWORD_WARNING_NOT_CONFIRMED`, nicht
`MASTER_PASSWORD_REJECTED`. Der Text des letzteren nennt die Mindestlänge
und die Wiederholung; beide können hier in Ordnung sein, und eine Meldung,
die vom falschen Zustand spricht, ist genau der Fehler aus Klarstellung 9,
Punkt 5. Der Code steht in `MASTER_PASSWORD_ERROR_CODES` und hat DE- und
EN-Text, damit er nicht als roher Schlüssel erscheint.

Dass er über die Oberfläche unerreichbar ist (der Knopf bleibt ohne
Häkchen aus), ist kein Grund gegen den Text: Er beschreibt einen Aufruf,
der das Formular umgangen hat, und beim Nachsehen im Log soll
unterscheidbar sein, *was* abgelehnt wurde.

## 5. Was die Änderung nicht ist

Sie lockert nichts. Der Riegel ist eine zusätzliche Bedingung vor einer
bestehenden Prüfkette; `check_new_password` ist wörtlich unverändert, und
`change_master_password` — das **kein** Passwort einrichtet, sondern ein
bestehendes ersetzt, dessen Warnung längst bestätigt wurde — ist nicht
berührt. Jeder Pfad, der vorher abgelehnt hat, lehnt weiter ab.

## 6. Gegenbeweis der beiden Tests

Beide neuen Tests sind gegen den Stand ohne Riegel rot gesehen worden (die
Prüfung in `set_up_master_password` durch `let _ = warning;` ersetzt,
Signatur unverändert):

- `master_password::tests::test_k12_setting_up_without_the_confirmed_warning_changes_nothing`
  → „ohne Bestätigung muss genau dieser Fehler kommen, nicht
  PasswordRejected" scheitert,
- `database_startup::tests::test_k12_d1_setup_without_the_confirmed_warning_changes_nothing`
  → „ohne Bestätigung darf der Start nicht zu einer offenen Datenbank
  führen" scheitert.

Der dritte Weg hat seinen eigenen Test,
`database_startup::tests::test_k12_d4_setup_without_the_confirmed_warning_keeps_the_old_wrapping`.
Er ist gegen den Stand **mit** Riegel, aber **ohne** die Vorab-Prüfung rot
gesehen worden (beide Aufrufe von `check_new_password_upfront` entfernt):
Dann ist die alte Verpackungsdatei nach dem Abbruch verschwunden und die
Fehlerart `MasterPasswordSetupFailedAfterRename` statt
`MasterPasswordSetupFailed`.

Beide arbeiten mit einem **einwandfreien** Passwort (lang genug, beide
Eingaben gleich). Das ist Absicht: Mit einem zu kurzen Passwort wären sie
auch ohne Riegel grün und prüften dann eine der beiden alten Bedingungen.

Auf der Seite der Oberfläche prüfen vier Zusicherungen, dass die Zusage
tatsächlich abgeschickt wird (`MasterPasswordSettings`,
`StartupPromptDialog`, `StartupGate` — und dass eine Antwort *ohne*
Passwort auch keine Bestätigung trägt). Gegen den Stand davor scheitern
sie, weil die Oberfläche dort nur zwei bzw. drei Parameter übergibt.

## 7. Was offen bleibt

1. **Der Riegel im Kommando ist nicht als Kommando getestet.** Die
   Vorprüfung in `commands::master_password::set_up_master_password` liegt
   hinter `State<AppState>` und ist in einem Unit-Test nicht aufrufbar —
   dasselbe gilt für den Schlüssel-Vergleich daneben
   (`KEYCHAIN_KEY_MISMATCH`), der aus demselben Grund untestet ist.
   Geprüft ist der Riegel an der Stelle, durch die das Kommando ohnehin
   muss. Ein Weg, die Kommandoschicht zu testen, ist eine eigene Aufgabe
   und berührt mehr als diese Spec.
2. **Die Oberfläche zeigt den neuen Code nie.** Das ist der gewollte
   Zustand, heißt aber auch: Seinen Text liest niemand nachträglich
   gegen. Er ist deshalb so geschrieben, dass er allein verständlich ist.
3. **„Erneut versuchen" setzt die Zuhörer nicht neu auf.** Scheitert das
   Anmelden der Start-Ereignisse, ist der Fehler jetzt sichtbar (das
   fehlende `catch` an der Anmelde-IIFE in `StartupGate`, spec-reviewer
   Runde 2) — aber der Knopf daneben hilft in dieser Lage nicht weiter,
   weil `listening` falsch bleibt. Ein Neuaufsetzen bräuchte einen
   eigenen Zähler in den Abhängigkeiten des Effekts; zurückgestellt, weil
   die Lage sichtbar ist und ein Neustart der App sie löst.
