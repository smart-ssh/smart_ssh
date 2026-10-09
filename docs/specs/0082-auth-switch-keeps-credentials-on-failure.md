# Spec 0082 — Anmeldeart wechseln: kein Credential-Verlust bei gescheitertem Speichern

Status: umgesetzt
Zweck: Scheitert das Speichern eines bearbeiteten Servers, bleiben die Zugangsdaten der bisherigen Anmeldeart vollständig erhalten.
Bezüge: Spec 0003 (Credentials), Spec 0008 (Server-Formular), Spec 0018 (Sudo-Passwort), Spec 0076 (Schlüsseldatei), ADR 0081 (Reihenfolge beim Aufräumen).
Review-Priorität: ERHÖHT (Credential-Handling)

## 1. Verhalten im Überblick

Beim Bearbeiten eines Servers kann der Nutzer die Anmeldeart wechseln (etwa
Passwort → Zertifikat). Die Zugangsdaten der neuen Art werden in den
Credential-Speicher geschrieben, die der bisherigen Art danach entfernt. Das
Speichern kann an mehreren Stellen scheitern: an einem fehlenden Pflichtfeld
(leeres Zertifikat), an einem Schreibfehler im Credential-Speicher, am
Sudo-Passwort oder beim Schreiben des Profils.

Früher gingen dabei die bisherigen Zugangsdaten verloren, obwohl das Profil
weiter die alte Anmeldeart trug; der Server ließ sich danach nicht mehr
verbinden. Das gilt nicht mehr.

Beim Bearbeiten sind die Secret-Felder keine Pflichtfelder; ein leeres Feld
bedeutet „unverändert" (Spec 0008).

## 2. Ziel und Nicht-Ziele

Ziel: Schlägt das Bearbeiten an irgendeiner Stelle fehl, sind
Credential-Speicher und Profil für die **bisherige** Anmeldeart so
verbindbar wie vorher (Ausnahme: R1). Gelingt es, ist das Ergebnis dasselbe
wie ohne diese Zusage.

Nicht-Ziele:

- Anlegen, Löschen und die Umwandlung Schlüsseldatei → Schlüsselbund bleiben
  unverändert; das Sudo-Passwort bleibt ein eigener Slot.
- Kein Zurücksetzen eines im selben Vorgang **überschriebenen** Secrets auf
  den alten Wert (R1).
- Formular, Meldungstexte und Fehlercodes ändern sich nicht.

## 3. Anforderungen

- **A1 Nichts von der bisherigen Art geht bei einem Fehler verloren.**
  Scheitert das Bearbeiten an der Pflichtfeld-Prüfung, an einem
  Schreibfehler im Credential-Speicher, am Sudo-Passwort oder am Schreiben
  des Profils, ist jeder Eintrag, auf den die bisher gespeicherte Anmeldeart
  verweist, danach noch vorhanden und das Profil unverändert. Der Fehler
  erreicht die Oberfläche mit demselben Code wie sonst.
- **A2 Aufgeräumt wird erst nach erfolgreichem Speichern.** Einträge der
  bisherigen Art werden erst entfernt, wenn das Profil die neue Anmeldeart
  trägt. Ein Eintrag, auf den die **neue** Anmeldeart verweist, wird dabei nie
  gelöscht. Das betrifft insbesondere die Passphrase, die Private Key und
  Schlüsseldatei unter demselben Eintrag ablegen.
- **A3 Kein verwaister Eintrag nach einem Fehler.** Hat der Vorgang vor dem
  Fehler Einträge der neuen Art geschrieben, die zu keinem Eintrag der
  bisherigen Anmeldeart gehören, werden sie wieder entfernt. Einträge der
  bisherigen Art und das Sudo-Passwort werden dabei nie angefasst.
- **A4 Aufräumfehler sind im Log sichtbar.** Scheitert das Entfernen eines
  Eintrags (A2 oder A3), bleibt das Speichern erfolgreich bzw. der
  ursprüngliche Fehler der gemeldete; es entsteht eine Warnung mit der
  Bezeichnung des Eintrags und ohne Secret-Inhalt.
- **A5 Erfolgsweg.** Bei gleicher Art bleibt „leeres Feld = unverändert".
  Bei einem Wechsel sind nach dem Speichern alle Einträge der bisherigen Art
  entfernt, außer denen, die die neue Art weiterverwendet.
- **A6 Prüfungen vor dem Credential-Speicher.** Die Ablehnung eines lokalen
  Servers und eines lokalen Jump-Hosts geschieht vor jedem Lesen, Schreiben
  oder Löschen im Credential-Speicher.
- **A7 Testbar ohne Oberflächen-Laufzeit.** Der gesamte Ablauf des Bearbeitens
  lässt sich mit In-Memory-Speichern aufrufen; der Tauri-Command reicht ihn
  nur durch.

## 4. Verhalten der Reihenfolge

- Nach Erfolg entfernt wird genau: die Einträge der bisherigen Anmeldeart
  abzüglich der Einträge der neuen. Bei gleicher Art ist diese Menge leer.
- Geschrieben wird zuerst in den Credential-Speicher, dann ins Profil;
  umgekehrt wird nur das Löschen angeordnet.
- Das Sudo-Passwort gehört nicht zur Anmeldeart und wird auch beim Aufräumen
  nach Erfolg nie entfernt (siehe K1).

## 5. Sicherheitszusagen

- Kein Secret-Inhalt in Log, Fehlermeldung oder Ereignis; Bezeichnungen der
  Einträge dürfen stehen.
- Lokaler Server und lokaler Jump-Host werden wie bisher vor allem anderen
  abgelehnt (A6).
- Anlegen behält seinen vollständigen Rückweg; Filter, Risiko,
  Ausführungspfad und MCP sind nicht berührt (MCP kann keine Server bearbeiten).

## 6. Testfälle

Jeder Test fährt den **Bearbeiten-Ablauf als Ganzes**. Ausgangszustand, wo
nicht anders genannt: Server mit Passwort-Anmeldung, Passwort hinterlegt.

- **T1** → Zertifikat, beide Felder leer: Pflichtfeld-Fehler, Passwort
  vorhanden, Profil trägt weiter Passwort.
- **T2** → Private Key, Schreiben des Schlüssels scheitert: Fehler, Passwort
  vorhanden.
- **T3** → Agent, Schreiben des Sudo-Passworts scheitert: Fehler, Passwort
  vorhanden, Profil unverändert.
- **T4** → Agent, Schreiben des Profils scheitert: Fehler, Passwort vorhanden.
- **T5** → Zertifikat, nur Zertifikat angegeben: Pflichtfeld-Fehler für den
  Schlüssel, Passwort vorhanden, **kein** Eintrag für das Zertifikat.
- **T6** Private Key mit Passphrase → Schlüsseldatei mit **neuer**
  Passphrase, Erfolg: Passphrase mit neuem Wert vorhanden, Schlüssel entfernt,
  Profil trägt die Schlüsseldatei mit diesem Eintrag.
- **T7** wie T6 ohne neue Passphrase: Schlüssel und Passphrase entfernt.
- **T8** → Agent, Erfolg: Passwort-Eintrag entfernt (A5).
- **T9** Gleiche Art, Feld leer, Erfolg: Passwort unverändert.
- **T10** → Agent, Erfolg, Löschen scheitert: Erfolg, Profil trägt Agent,
  Warnung mit Bezeichnung, Passwort-Wert **nicht** im Log (A4).
- **T11** Schlüsseldatei mit Passphrase → Private Key mit neuer Passphrase,
  Erfolg: Passphrase mit neuem Wert, Profil trägt Private Key.
- **T12** Schlüsseldatei mit Passphrase → Agent, Erfolg: Passphrase entfernt.
- **T13** Passwort und Sudo-Passwort hinterlegt → Agent mit neuem
  Sudo-Passwort, Profil scheitert: Fehler, Passwort vorhanden, Sudo-Eintrag
  vorhanden (R1); der Rückweg räumt nicht wie beim Anlegen alle Slots ab.
- **T14** wie T5, zusätzlich scheitert jedes Löschen: Code bleibt der
  Pflichtfeld-Fehler, Passwort vorhanden, Warnung mit Bezeichnung.
- **T15** Bearbeiten mit lokalem Jump-Host und Wechsel auf Private Key mit
  Schlüssel und neuem Sudo-Passwort: Ablehnung; kein Zugriff auf den
  Credential-Speicher vor der Ablehnung. **T15b** dasselbe für die ID des
  lokalen Servers.
- **T16** Gleiche Art mit neuem Passwort, Profil scheitert: Eintrag trägt den
  neuen Wert, nichts gelöscht (R1).
- **T17** → Private Key mit Schlüssel, Profil scheitert: Passwort vorhanden,
  **kein** Eintrag für den Schlüssel. **T17b** dasselbe, aber das Schreiben
  des Sudo-Passworts scheitert.
- **T18** Private Key mit Passphrase → Schlüsseldatei mit **neuer**
  Passphrase, Profil scheitert: Schlüssel vorhanden, Passphrase vorhanden
  (neuer Wert, R1), Profil unverändert.

## 7. Grenzen

- **R1** Nutzen alte und neue Anmeldeart denselben Eintrag (gleiche Art mit
  neuem Wert, oder die gemeinsame Passphrase), überschreibt der Vorgang den
  alten Wert vor dem Schreiben des Profils; scheitert danach etwas, bleibt der
  neue Wert stehen. Bei gleicher Art ist das der gewollte Wert. Beim Wechsel
  Private Key ↔ Schlüsseldatei mit neuer Passphrase lässt sich die bisherige
  Anmeldung danach nicht mehr entsperren. Dasselbe gilt für ein neues
  Sudo-Passwort. Ein Zurücksetzen hieße, bei jedem Speichern den alten Wert
  vorher auszulesen.
- **R2** Zwei gleichzeitige Speichervorgänge desselben Servers rechnen mit
  ihrem Anfangsstand; der spätere könnte einen Eintrag löschen, den der frühere
  gerade geschrieben hat. Die Anmeldeart ändern nur das Speichern und die
  Umwandlung Schlüsseldatei → Schlüsselbund, beide aus demselben Formular.

## 8. Klarstellung

**K1** Das Sudo-Passwort ist auch beim Aufräumen nach Erfolg ausgenommen. Es
gehört nicht zur Anmeldeart, die Mengen sind normalerweise disjunkt; die
Ausnahme gilt trotzdem auf beiden Wegen, damit ein auf den Sudo-Slot
verweisender Eintrag (von Hand veränderte Zeile) das gerade geschriebene
Sudo-Passwort nicht entfernt. Die Kehrseite — ein verwaister Eintrag statt
eines fehlenden Credentials — ist gewollt (ADR 0081).
