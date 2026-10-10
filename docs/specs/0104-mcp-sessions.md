# Spec 0104 — Eigene Sitzung und eigener Tab für MCP-Anfragen

Status: umgesetzt
Zweck: Aktionen, die ein externer MCP-Client (z. B. ein Coding-Agent)
anfragt, laufen in einer eigenen Sitzung mit eigener SSH-Verbindung und
eigenem Tab, getrennt vom Tab, in dem der Nutzer arbeitet. Terminal, Chat
und Fokus des Nutzers bleiben von MCP-Aktivität unberührt.
Bezüge: Spec 0005 (Verbindungsaufbau, Host-Key), Spec 0017 (Tabs), Spec 0028
(lokaler MCP-Server), Spec 0034/0040 (Chat-Persistenz: MCP-Sitzungen werden
nicht gespeichert), Spec 0039 (Fencing), Spec 0057 (Kompaktierung:
MCP-Einträge fließen nicht in die Zusammenfassung), Spec 0067 (erhöhter
Dateibrowser), ADR 0109, ADR 0111.
Review-Priorität: ERHÖHT (MCP-Vertrauensgrenze, Ausführungspfad,
Sitzungsaufbau)

## 1. (entfallen)

Früher Ist-Stand vor dieser Spec; kein Verhalten.

## 2. MCP-Sitzung

- **Schlüssel:** (Server, MCP-Client). Der Client ist der beim
  MCP-Verbindungsaufbau übermittelte Name (`clientInfo.name`), ohne
  Leerraum am Rand. Clients ohne Namen teilen sich je Server eine Sitzung,
  weil die App sie nicht unterscheiden kann.
- **Länge des Client-Namens:** höchstens 64 Zeichen. Ein längerer Name wird
  an einer Zeichengrenze gekürzt (nie mitten in einem Multi-Byte-Zeichen),
  danach wird Leerraum am Ende entfernt. Der gekürzte Name gilt für den
  Schlüssel und für jede Anzeige: Tab-Beschriftung, OS-Benachrichtigung,
  Aktionskarte. Zwei Namen, die sich erst hinter Zeichen 64 unterscheiden,
  sind derselbe Client.
- **Höchstzahl je Server:** höchstens 4 offene MCP-Sitzungen je Server,
  fest, nicht einstellbar. Es zählt jede eingetragene Sitzung, auch eine
  abgerissene, deren Tab noch offen ist. Eine Anfrage, die eine weitere
  Sitzung bräuchte, scheitert mit „Für diesen Server sind bereits 4
  MCP-Sitzungen offen (Höchstzahl). Schließe in der App einen MCP-Tab
  dieses Servers und versuche es erneut." Es wird weder eine Verbindung
  aufgebaut noch ein Tab angelegt. Prüfen und Eintragen geschehen atomar,
  auch über verschiedene Clients hinweg. Die Wiederverwendung einer
  verbundenen Sitzung desselben Schlüssels ist davon nicht betroffen.
  Schließen eines MCP-Tabs gibt einen Platz frei.
- **Eigene SSH-Verbindung** auf demselben Weg wie ein Klick in der
  Seitenleiste: gleiche gespeicherte Zugangsdaten, gleicher Host-Key-Ablauf
  (unbekannter oder geänderter Schlüssel → Abfrage wie sonst). Der Verlauf
  der Sitzung wird nicht gespeichert (Spec 0040, Abschnitt 4). Dem
  MCP-Client wird dadurch nichts Neues offengelegt.
- **Nur MCP-Sitzungen.** Eine MCP-Anfrage verwendet nie eine
  Nutzer-Sitzung desselben Servers, egal ob diese verbunden ist.
- **Wiederverwendung:** Eine MCP-Anfrage nimmt die Sitzung ihres
  Schlüssels, solange sie verbunden ist. Ist die Verbindung abgerissen oder
  die Sitzung weg, legt die nächste Anfrage eine neue an. Ein noch offener
  alter Tab bleibt bis zum Schließen als MCP-Tab gekennzeichnet.
- **Gleichzeitige Anfragen** desselben Schlüssels bauen genau eine
  Verbindung auf; die spätere wartet auf die erste. Andere Schlüssel
  (anderer Server oder Client) warten nicht, auch nicht auf einen offenen
  Host-Key-Dialog. Die App merkt sich dafür nichts über eine Anfrage
  hinaus, auch nicht nach einem abgebrochenen Warten; die Menge der je
  gesehenen Client-Namen lässt keinen Speicher wachsen.
- Die Sitzung gilt schon **vor** dem Verbindungsaufbau als MCP-Sitzung. So
  ist der Tab schon während eines Host-Key-Dialogs gekennzeichnet, und
  Nutzer-Eingaben sind von Anfang an gesperrt. Scheitert der Aufbau, wird
  sie wieder ausgetragen; die nächste Anfrage verbindet neu.

## 3. Tab und Fokus

- **Beschriftung** „<Client> @ <Server>", ohne Namen
  „Externes Tool @ <Server>", plus MCP-Abzeichen am Tab und im Kopf der
  Ansicht. Auch nach einem Neuladen der Oberfläche erscheinen MCP-Tabs
  wieder als solche.
- **Kein Fokus-Wechsel:** Der MCP-Tab erscheint im Hintergrund. Der aktive
  Tab des Nutzers bleibt aktiv, ebenso die Übersicht. Nach einem Neuladen
  wird ein MCP-Tab nie automatisch aktiv. Ein Klick auf den Server in der
  Seitenleiste wechselt zum Nutzer-Tab bzw. öffnet einen neuen, nie zum
  MCP-Tab.
- **Wartende Bestätigung:** Der MCP-Tab zeigt dann ein pulsierendes,
  beschriftetes Abzeichen „Bestätigung nötig" statt nur des Punkts der
  Nutzer-Tabs. Dazu kommt die OS-Benachrichtigung aus Spec 0028,
  Abschnitt 9a. Der Bestätigungs-Timeout aus Spec 0028, Abschnitt 7 gilt
  unverändert.
- **Höchstens eine wartende Bestätigung je MCP-Sitzung:** Solange in einer
  MCP-Sitzung eine Bestätigung offen ist, wird jede weitere vorgeschlagene
  Aktion dieser Sitzung sofort abgewiesen und nicht ausgeführt. Der
  MCP-Client bekommt einen Tool-Fehler, der ihn anweist, die offene Aktion
  zuerst in der App zu bestätigen oder abzulehnen. Es entsteht keine zweite
  Karte. Die offene Bestätigung bleibt unverändert bestehen, auch wenn der
  Tool-Aufruf des Clients schon abgelaufen ist; danach werden neue
  Vorschläge wieder angenommen.

## 4. Inhalt des MCP-Tabs

- Die Aktionskarten des Clients mit Bestätigen und Ablehnen und ihre
  Ergebnisse (Ausgabe, Dateiinhalt, Notiz-Diff), wie im Chat-Panel.
- **Kein interaktives Terminal, kein Dateibrowser, keine Chat-Eingabe.**
  Eine nur lesende Sicht auf das, was lief. An Stelle der Eingabezeile
  steht ein Hinweis, dass die Sitzung einem externen Tool gehört und
  Schließen offene Bestätigungen ablehnt.
- Das Backend setzt das zusätzlich durch: Chat-Nachricht senden, eine
  abgeschnittene Antwort fortsetzen und ein Terminal öffnen werden für
  eine MCP-Sitzung abgelehnt. So kommt kein Nutzer-Kontext in die
  MCP-Sitzung.

## 5. Schließen

- Schließen des MCP-Tabs trägt die Sitzung aus und **lehnt eine wartende
  Bestätigung ab** (fail closed), bevor die Verbindung getrennt wird. Die
  Aktion wird nicht ausgeführt und endet nicht erst am Timeout.
- Der MCP-Client bekommt für diese Aktion die Meldung „Die MCP-Sitzung
  wurde in der App geschlossen; die Aktion wurde nicht ausgeführt. Eine
  neue Anfrage öffnet eine neue MCP-Sitzung." statt „vom Nutzer
  abgelehnt". Ein Ergebnis oder ein Filter-`Deny`, das vorher schon
  feststand, bleibt unverändert.
- Die nächste Anfrage dieses Clients legt eine neue MCP-Sitzung samt Tab
  an.
- Wird der MCP-Tab geschlossen, während der Verbindungsaufbau noch läuft
  (z. B. bei offenem Host-Key-Dialog), wird die danach doch aufgebaute
  Verbindung sofort wieder getrennt. Die Anfrage scheitert mit derselben
  Meldung.
- Das Ablehnen einer wartenden Bestätigung beim Schließen gilt für
  **jeden** Tab, nicht nur für MCP-Tabs, und hängt nicht von der
  Oberfläche ab: Das Backend lehnt auch dann ab, wenn die Oberfläche die
  wartende Aktion nicht mehr kennt (z. B. nach einem Neuladen). Für
  Nutzer-Tabs fragt die Oberfläche vor dem Schließen nach (Spec 0017). War
  die Bestätigung schon entschieden, ändert das Schließen nichts daran.

## 6. Sicherheitszusagen

- Jede MCP-Aktion läuft über denselben Pfad wie bisher: erzwungene
  Bestätigung (Spec 0028, Abschnitt 5), Filter-Engine, Redaction und
  Fencing (Spec 0039), jeweils in der MCP-Sitzung selbst.
- Kein Weg zum erhöhten SFTP-Kanal (Spec 0067).
- MCP-Einträge werden nicht gespeichert (Spec 0040, Abschnitt 4) und
  fließen nicht in eine Zusammenfassung (Spec 0057, Abschnitt 2.1).
- Die Kontexte sind getrennt: MCP-Ausgaben erreichen die Chat-KI des
  Nutzers nicht, und der Nutzer-Chat erreicht die MCP-Sitzung nicht.

## 7. Prüfbare Fälle

- Ein verbundener Nutzer-Tab auf Server X wird für MCP nie verwendet;
  stattdessen wird neu verbunden.
- Zwei Clients auf demselben Server bekommen zwei Sitzungen. Derselbe
  Client nimmt seine verbundene Sitzung wieder, ohne neu zu verbinden.
- Eine MCP-Aktion läuft nur über Verbindung und Verlauf der MCP-Sitzung,
  weiterhin als Bestätigung trotz Allow-Regel. Der Verlauf der
  Nutzer-Sitzung bleibt unverändert, und der Nutzer-Verlauf kommt nicht in
  die MCP-Sitzung.
- Schließen lehnt die wartende Bestätigung ab, nichts wird ausgeführt, die
  nächste Anfrage bekommt eine neue Sitzung; die Sitzung ist ausgetragen,
  bevor die abgelehnte Aktion zurückkehrt.
- Schließen eines Nutzer-Tabs lehnt dessen wartende Bestätigung ohne
  Zutun der Oberfläche ab und zählt wie eine Ablehnung durch den Nutzer;
  eine schon entschiedene oder fehlende Bestätigung bleibt unberührt.
- Ein gescheiterter Aufbau trägt die Sitzung aus; ein während des Aufbaus
  geschlossener Tab ergibt „geschlossen", und die verwaiste Verbindung wird
  getrennt; zwei gleichzeitige Anfragen desselben Schlüssels bauen genau
  eine Verbindung auf, andere Schlüssel warten nicht.
- Nutzer-Eingaben werden nur für MCP-Sitzungen abgelehnt.
- Ein zu langer Client-Name wird gekürzt, Namen mit gleichem Anfang bis
  zur Grenze ergeben denselben Schlüssel, Multi-Byte-Namen führen nicht zum
  Absturz. Bei erreichter Höchstzahl: Fehler, kein Eintrag, kein Tab;
  Schließen gibt einen Platz frei; die eigene verbundene Sitzung wird
  weiter verwendet; eine abgerissene, offene Sitzung zählt mit.
- Oberfläche: kein Fokus-Wechsel beim Erscheinen und nach Neuladen,
  Abzeichen bei wartender Bestätigung, Seitenleisten-Klick nie zum
  MCP-Tab, nur lesende Ansicht ohne Terminal und Eingabe.

## 8. (entfallen)

Früher „Offene Punkte"; es gab keine.
