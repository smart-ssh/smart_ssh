# Spec 0017 — Mehrere Sitzungen als Tabs

Status: umgesetzt
Zweck: Mehrere Server-Verbindungen gleichzeitig offen halten und per Tab-Leiste wechseln — jede Sitzung mit eigenem Terminal, eigenem Chat und eigener wartender Bestätigung, ohne dass sich Sitzungen gegenseitig stören.
Bezüge: Spec 0005 (Verbindung), Spec 0007 (Sitzung, Bestätigung), Spec 0010 (Notiz-Vorschlag beim Beenden), Spec 0014 (Titelleiste), Spec 0015 (Prompt-Historie), Spec 0032 (Localhost als Sitzung), Spec 0092 (Verschärfung auf Bestätigung), Spec 0104 (MCP-Sitzungen), ADR 0020, ADR 0038, ADR 0109.

## 1. Ziel

Der Nutzer kann mehrere Server zugleich offen halten. Jede Sitzung hat ihren
eigenen Terminal-Inhalt, ihren eigenen Chat-Verlauf, ihren eigenen KI-Kontext
und ihre eigene wartende Bestätigung. Es ist immer erkennbar, in welcher
Sitzung man sich befindet und wo etwas auf eine Entscheidung wartet.

## 2. Sitzungsliste und Status

**2.1 Das Backend führt Buch.** Welche Sitzungen offen sind, weiß das
Backend; die Oberfläche fragt es beim Start und nach einem Neuladen
(Entwicklungsmodus, Hot-Reload) ab und baut die Tab-Leiste daraus wieder auf.
Je Sitzung stehen dort: die Sitzung, der Server samt Name, der Status, ob eine
Bestätigung wartet und — bei einer MCP-Sitzung — der Name des Clients
(Spec 0104).

**2.2 Status.** Eine Sitzung ist

- *verbunden*,
- *getrennt* — wenn die Terminal-Verbindung endet oder die Sitzung ausdrücklich
  getrennt wurde; der Tab bleibt dann stehen, bis der Nutzer ihn schließt, oder
- *wartet auf Host-Key* — solange ein Verbindungsaufbau auf die Entscheidung
  zum Host-Key wartet (Spec 0005). Diesen Status sieht man nur in der
  Sitzungsliste, etwa nach einem Neuladen der Oberfläche; er löst kein
  Statusereignis aus.

**2.3 Sitzungen bremsen sich nicht.** Eine langsame KI-Antwort oder ein
laufendes Kommando in einer Sitzung verzögert weder die Eingabe noch die
Antworten in einer anderen Sitzung. Jede Sitzung hat ihre eigene
Synchronisation; die gemeinsame Sitzungsübersicht wird nur kurz und nie über
ein Warten hinweg gehalten.

## 3. Tab-Leiste

- Die Leiste sitzt in der Titelleiste (Spec 0014), rechts neben dem
  App-Namen. Tabs und ihre Schaltflächen sind Klickflächen und ziehen das
  Fenster nicht; die Leerfläche dazwischen tut es weiterhin.
- Ganz links steht eine feste Kachel „Übersicht". Sie führt zu den Server- und
  Verwaltungsansichten, ohne offene Sitzungen zu schließen. Sie erscheint,
  sobald mindestens eine Sitzung offen ist.
- Ein Tab zeigt den Servernamen (bei langen Namen gekürzt, der volle Name steht
  im Tooltip), einen Statuspunkt (grün verbunden, grau getrennt, gelb wartet
  auf Host-Key) und eine Schließen-Schaltfläche.
- Ein Klick auf einen Server in der Serverliste öffnet einen **neuen** Tab. Ist
  für diesen Server schon ein Nutzer-Tab offen, wird stattdessen zu ihm
  gewechselt, bevor irgendeine Verbindung aufgebaut wird. Das gilt auch für
  einen getrennten Tab: er muss erst geschlossen werden, bevor man neu
  verbinden kann.
- **Ausnahme MCP (Spec 0104).** Ein externer MCP-Client bekommt je Server und
  Client einen eigenen Tab „Client @ Server" mit MCP-Abzeichen und
  eigener Verbindung, zusätzlich zu einem eventuell offenen Nutzer-Tab
  desselben Servers. Ein Klick auf einen Server wechselt nie zu einem
  MCP-Tab, und ein MCP-Tab wird nie von selbst aktiv, auch nicht nach dem
  Schließen eines anderen.
- **Tastatur.** `Cmd`/`Ctrl+W` schließt den aktiven Tab (nicht die Übersicht).
  `Cmd`/`Ctrl+Tab` wechselt zum nächsten Tab, nach dem letzten wieder zum
  ersten. `Cmd`/`Ctrl+1` bis `9` springt zum n-ten Tab. Unter macOS fängt das
  System `Cmd+Tab` ab; dort bleiben `Cmd+W` und `Cmd+1..9`.
- Nach dem Schließen des aktiven Tabs wird der zuletzt geöffnete verbliebene
  Nutzer-Tab aktiv, sonst die Übersicht.
- Localhost (Spec 0032) ist ein Tab wie jeder andere.

## 4. Zustand je Sitzung

Jede Sitzung behält, auch im Hintergrund und über einen Tab-Wechsel hinweg:

- ihren Chat-Verlauf samt laufender Antwort-Streams,
- ihr Terminal samt Scrollback,
- ihren wartenden Bestätigungsdialog,
- ihren Entwurf und die Navigation durch die Prompt-Historie (Spec 0015), damit
  ein Tab-Wechsel keinen halbfertigen Entwurf verwirft,
- ihren Dateibrowser-Zustand (ADR 0020).

Hintergrund-Sitzungen laufen weiter: Terminal-Ausgabe wird empfangen und
aufgenommen, KI-Antworten laufen zu Ende, Bestätigungen bleiben erhalten.

**Zuordnung.** Jedes Ereignis des Backends trägt die Kennung seiner Sitzung
und wird strikt dieser Sitzung zugeordnet — nie dem gerade sichtbaren Tab.
Ausgabe einer Hintergrund-Sitzung landet also nie im Terminal oder Chat einer
anderen.

## 5. Wartende Bestätigungen in Hintergrund-Tabs

Legt eine Hintergrund-Sitzung ein Kommando zur Bestätigung vor, erscheint
**kein** Dialog im gerade sichtbaren Tab: der Nutzer könnte sonst eine
Bestätigung geben, ohne zu sehen, auf welchem Server sie wirkt.

- Der betroffene Tab trägt einen auffälligen Hinweis: einen pulsierenden gelben
  Punkt (Tooltip „Wartet auf Bestätigung"); bei einem MCP-Tab ein
  beschriftetes Abzeichen „Bestätigung nötig".
- Der Dialog erscheint erst, wenn der Nutzer zu diesem Tab wechselt, also im
  Kontext des Servers, auf dem er wirkt.
- Der Hinweis erscheint auch dann, wenn eine zunächst automatisch erlaubte
  Aktion nachträglich auf Bestätigung verschärft wird (Spec 0092).
- Der Hinweis verschwindet, sobald die Aktion entschieden oder beendet ist.
- **Schließen lehnt ab.** Wird ein Tab mit wartender Bestätigung geschlossen,
  fragt die Oberfläche zuvor nach („Für „<Name>" wartet noch eine Bestätigung.
  Tab wirklich schließen? Die wartende Aktion gilt dann als abgelehnt."). Nach
  Zustimmung gilt die Aktion als **abgelehnt**, nie als genehmigt. Das
  Ablehnen geschieht im Backend beim Trennen jeder Sitzung selbst, bevor die
  Verbindung getrennt wird; es hängt nicht an der Oberfläche und funktioniert
  deshalb auch nach einem Neuladen, bei dem die Oberfläche die Aktion nicht
  mehr kennt.
- In einer MCP-Sitzung wartet höchstens eine Bestätigung zugleich
  (Spec 0104); der Hinweis gehört immer zu dieser einen Aktion, und das
  Schließen lehnt genau sie ab. Der MCP-Client erfährt „Sitzung geschlossen",
  nicht „vom Nutzer abgelehnt".

Ein Bestätigungsdialog läuft nach spätestens einer Stunde ohne Entscheidung
ab und gilt dann als abgelehnt (Spec 0007, ADR 0038).

## 6. Schließen und Verbindungsabbau

Schließen-Schaltfläche und `Cmd`/`Ctrl+W` trennen die Sitzung. Dabei:

- wird eine wartende Bestätigung abgelehnt (Abschnitt 5),
- wird die Verbindung getrennt; ein Fehler dabei hindert das Schließen nicht,
- wird der Tab entfernt und die Sitzung aus der Sitzungsliste genommen,
- läuft im Hintergrund der Notiz-Vorschlag beim Beenden (Spec 0010), der
  unabhängig vom Tab als Benachrichtigung erscheint und nicht an den
  geschlossenen Tab gebunden ist.

## 7. Sicherheitszusagen

- Eine Bestätigung wird nur in dem Tab gezeigt und entschieden, zu dem sie
  gehört; eine Bestätigung im falschen Server-Kontext ist nicht möglich.
- Schließen, Ablaufen und Trennen lehnen eine wartende Aktion ab und genehmigen
  nie.
- Ereignisse werden nur über die Sitzungskennung zugeordnet.

## 8. Grenzen

- **Ein Nutzer-Tab je Server.** Zwei parallele Shells zum selben Server als
  Nutzer-Tabs gibt es nicht; die Frage, ob sie Kontext und Prompt-Historie
  teilen würden, ist bewusst nicht entschieden. Nur MCP-Clients bekommen einen
  zusätzlichen Tab je Server.
- **Keine Wiederherstellung nach App-Neustart.** Offene Tabs überstehen einen
  Neustart nicht (die Verbindungen enden mit der App); es gibt keinen
  automatischen Wiederaufbau mehrerer Server beim Start.
- **„Getrennt" nur über das Terminal.** Der Status wird auf „getrennt" gesetzt,
  wenn das Terminal der Sitzung endet oder der Nutzer trennt. Eine Sitzung ohne
  geöffnetes Terminal (etwa eine MCP-Sitzung) zeigt weiter „verbunden", auch
  wenn die Verbindung gerissen ist; Kommandos scheitern dann mit „Sitzung
  verloren" (Spec 0005).
- **Kein Zuordnungs-Hinweis nach Reload.** Nach einem Neuladen kennt die
  Oberfläche die konkrete wartende Aktion nicht mehr, zeigt aber den Hinweis
  und die Rückfrage weiterhin an; das Ablehnen übernimmt das Backend.
