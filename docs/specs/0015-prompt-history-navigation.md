# Spec 0015 — Chat-Prompt-Historie (Pfeiltasten-Navigation)

Status: umgesetzt
Zweck: Im Chat-Eingabefeld lassen sich zuletzt an einen Server gesendete Prompts mit den Pfeiltasten durchblättern, wie der Befehlsverlauf einer Shell. Die Historie gehört zum Server und überlebt Verbindungstrennung und App-Neustart.
Bezüge: Spec 0007 (Kernschleife, Chat-Eingabe), Spec 0017 (Tabs, Entwurf und Navigation je Tab), Spec 0036 und Spec 0101 (Schutz gespeicherter Inhalte durch die verschlüsselte Datenbank), Spec 0040 (Eingabe-Historie als sensibler Text).

## 1. Ziel

Pfeil-nach-oben im Chat-Eingabefeld zeigt den jüngsten zuvor an diesen Server
gesendeten Prompt, weiteres Pfeil-nach-oben den jeweils älteren.
Pfeil-nach-unten blättert zurück Richtung neuestem Eintrag und schließlich zum
eigenen, noch nicht gesendeten Entwurf. Die Historie ist **pro Server**
gespeichert, nicht global und nicht nur pro laufender Sitzung.

## 2. Speicherung und Löschung

Die Historie liegt in der lokalen Datenbank, verknüpft mit dem Server. Wird der
Server gelöscht, wird seine Historie mit entfernt.

Prompt-Inhalte sind freier, womöglich sensibler Text. Sie sind wie Notizen und
der Chat-Verlauf durch die Verschlüsselung der ganzen Datenbankdatei geschützt
(Spec 0036, Spec 0101); eine eigene Verschlüsselung dieser Daten gibt es nicht.

## 3. Speicherung neuer Einträge

Es gibt keine eigene Speichern-Aktion: Jede an den KI-Chat gesendete
Nachricht wird zusätzlich in der Historie des Servers der Sitzung abgelegt.
Das gilt auch für eine Nachricht, die während eines laufenden Durchgangs
eingereiht wird.

- Ein Prompt, der exakt dem jüngsten gespeicherten Eintrag dieses Servers
  entspricht, wird nicht doppelt gespeichert. Der bestehende Eintrag gilt dann
  als jüngster.
- Pro Server bleiben höchstens die letzten 200 Einträge. Beim Speichern eines
  neuen Eintrags werden darüber hinaus die ältesten entfernt.
- Die Historie ist eine Zusatzfunktion: Lässt sich ein Prompt nicht speichern,
  wird die Nachricht trotzdem gesendet; der Fehler erscheint nur im Log.

## 4. Abruf

Das Frontend kann die gespeicherten Prompts eines Servers abrufen. Sie kommen
in chronologischer Reihenfolge, älteste zuerst; die Navigation greift von
hinten zu.

## 5. Navigation im Eingabefeld

Das Eingabefeld ist mehrzeilig (Spec 0007). Die Pfeiltasten-Navigation muss
sich deshalb von der normalen Cursor-Bewegung in einem mehrzeiligen Entwurf
unterscheiden:

- **Pfeil-nach-oben** löst die Historien-Navigation nur aus, wenn der Cursor
  an Position 0 steht (Anfang der ersten Zeile) und nichts markiert ist. Sonst
  bewegt sich der Cursor wie gewohnt.
- **Pfeil-nach-unten** löst sie nur aus, wenn der Cursor am Ende des Textes
  steht und nichts markiert ist. Sonst bewegt sich der Cursor wie gewohnt.
- Beim ersten Pfeil-nach-oben wird der aktuelle, noch nicht gesendete Entwurf
  zwischengespeichert, und das Feld zeigt den jüngsten Eintrag. Jedes weitere
  Pfeil-nach-oben zeigt den nächstälteren, bis der älteste erreicht ist; dort
  bleibt es stehen, ohne Fehlermeldung. Gibt es keine Einträge, passiert
  nichts.
- Pfeil-nach-unten geht in die Gegenrichtung. Nach dem jüngsten Eintrag
  erscheint wieder der zwischengespeicherte Entwurf, auch wenn er leer war.
  Außerhalb der Navigation bewirkt Pfeil-nach-unten nichts Besonderes.
- Nach dem Einsetzen eines Eintrags steht der Cursor am Ende des Textes.
- Jede normale Texteingabe beendet die Navigation. Ein erneutes
  Pfeil-nach-oben beginnt dann wieder beim jüngsten Eintrag, statt an der
  zuvor erreichten Stelle fortzusetzen. Auch das Senden setzt die Navigation
  zurück. Bearbeitete Historieneinträge verhalten sich nicht wie ein neuer
  Zwischenzustand (kein volles Readline-Verhalten).
- Die Historie wird beim Öffnen des Server-Chats (und beim Wechsel auf einen
  anderen Server) geladen und nicht bei jeder Pfeiltaste neu. Nach jedem
  erfolgreich gesendeten Prompt wird sie erneut geladen: Ein im geöffneten
  Chat gesendeter Prompt ist sofort per Pfeil-nach-oben erreichbar. Doppelte
  aufeinanderfolgende Prompts und die Grenze von 200 Einträgen gelten dabei
  unverändert (Abschnitt 3). Kann sie
  nicht geladen werden, ist sie leer. Der Navigationszustand wird beim
  Serverwechsel zurückgesetzt.

## 6. Grenzen

- Es gibt keine Oberfläche zum Löschen oder Durchsuchen der Historie, nur die
  Pfeiltasten-Navigation.
- Der lokale Pseudo-Server (Localhost-Sitzung) hat keinen gespeicherten Server
  und damit keine Historie.
