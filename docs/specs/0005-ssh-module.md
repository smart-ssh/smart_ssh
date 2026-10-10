# Spec 0005 — SSH-Verbindungsmodul

Status: umgesetzt
Zweck: Wie Smart SSH eine Verbindung zu einem Server aufbaut (auch über Jump-Hosts), dessen Host-Key prüft und über dieselbe Verbindung einzelne Kommandos, ein interaktives Terminal und Dateizugriffe bedient.
Bezüge: Spec 0003 (Server-Profile, Anmeldearten, Jump-Host-Feld), Spec 0002 (Filter-Engine), Spec 0007 (Sitzung, Host-Key-Abfrage im Ablauf), Spec 0020 (SFTP), Spec 0027 (Abbruch), Spec 0043 (Ausgabegrenzen), Spec 0069 (Fehlerarten, Zeitgrenzen), Spec 0076 (Schlüsseldatei), Spec 0100 (Host-Key-Dialog per Tastatur), ADR 0007, ADR 0012, ADR 0110.

## 1. Ziel und Betriebsarten

Eine Verbindung wird einmal aufgebaut und dann auf zwei Arten genutzt:

- **Einzelkommando.** Ein Kommando läuft in einem eigenen Kanal, Standardausgabe,
  Fehlerausgabe und Exit-Code werden eingesammelt. Das ist der Weg für alles,
  was die KI vorschlägt: jedes Kommando wird einzeln geprüft (Spec 0002) und
  bestätigt (Spec 0007), bevor es hier ankommt.
- **Interaktives Terminal.** Eine Shell mit Pseudo-Terminal für das
  Terminal-Panel. Hier tippt der Nutzer selbst; die Filter-Engine prüft diese
  Eingaben nicht, weil kein Vorschlag einer KI dazwischen liegt.

Beide Arten (und der Dateizugriff, Spec 0020) laufen als getrennte Kanäle über
**dieselbe** offene Verbindung; ein Kommando löst keinen neuen
Verbindungsaufbau aus. Je geöffnetem Server-Tab gibt es genau eine Verbindung
(kein Teilen zwischen Tabs).

## 2. Austauschbarkeit

Alles oberhalb des Transports (Filter-Engine, Risiko-Einstufung, Bestätigung,
KI-Anbindung) kennt nur die abstrakte Schnittstelle „Kommando ausführen,
Shell öffnen, Dateien lesen/schreiben, trennen". Der lokale Pseudo-Server
(Spec 0032) hängt an derselben Schnittstelle und durchläuft deshalb dieselben
Prüfungen. Die Host-Key-Ablage ist ebenfalls austauschbar; die Verbindungslogik
hängt nicht an ihrem Speicherort.

## 3. Keine externe SSH-Installation nötig

Smart SSH bringt seinen SSH-Client mit. Ein installiertes OpenSSH oder
`libssh` ist weder nötig noch wird es benutzt; das Verhalten ist auf Windows,
macOS und Linux gleich. Ein SSH-Agent des Systems wird nur für die Anmeldeart
„Agent" befragt (Spec 0003).

## 4. Betriebsarten im Einzelnen

**4.1 Einzelkommando.** Ergebnis ist Standardausgabe, Fehlerausgabe und der
Exit-Code. Beendet sich der Kanal ohne Exit-Code (etwa nach einem Abbruch,
Spec 0027), bleibt der Exit-Code leer; das ist kein Fehler.

**4.2 Eingabe über Standardeingabe.** Ein Kommando kann Eingabedaten
mitbekommen, die sofort nach dem Start geschrieben werden und danach den Kanal
für die Eingabe schließen. Das nutzt die Sudo-Passwort-Funktion (Spec 0018),
weil ein Einzelkommando kein Terminal hat.

**4.3 Ausgabegrenze.** Die eingesammelte Ausgabe je Kommando ist auf 2 MiB
begrenzt. Wird die Grenze erreicht, wird der Kanal geschlossen, die Ausgabe
endet mit dem Hinweis `[Output truncated: exceeded limit]`, und das Ergebnis
ist als abgeschnitten gekennzeichnet (Spec 0043).

**4.4 Interaktives Terminal.** Die Shell wird mit Terminal-Typ `xterm-256color`
geöffnet, die Größe wird beim Öffnen übergeben und kann später geändert werden.
Gelesen wird ein freier Datenstrom; Ende des Kanals meldet sich als leere
Antwort, danach gilt die Sitzung als getrennt (Spec 0017).

**4.5 Trennen.** Trennen schließt die Verbindung samt aller Kanäle. Ein Fehler
dabei lässt die Sitzung trotzdem als beendet gelten.

## 5. Jump-Hosts

Hat ein Server im Profil einen Jump-Host (Spec 0003), wird die Kette vom
äußersten Jump-Host bis zum Ziel aufgelöst und in dieser Reihenfolge
verbunden:

- Zum ersten Hop gibt es eine TCP-Verbindung. Jeder weitere Hop wird durch
  einen Tunnel-Kanal der vorherigen Verbindung erreicht, darüber läuft ein
  eigener SSH-Handshake. Es entsteht kein weiterer TCP-Socket vom eigenen
  Rechner aus.
- Jeder Hop meldet sich mit der **eigenen** Anmeldeart und den eigenen
  Zugangsdaten an; nichts von einem Hop wird an einen anderen weitergereicht.
- Jeder Hop hat eine **eigene** Host-Key-Prüfung (Abschnitt 6). Ein
  unbekannter Schlüssel auf der Bastion wird ebenso einzeln abgefragt wie
  der des Ziels.
- Eine Kette, die auf sich selbst zurückführt, endet vor jedem Netzwerkzugriff
  mit dem Fehler „Jump-Host-Zyklus". Ein Jump-Host, der nicht mehr gefunden
  wird, endet mit einem Verbindungsfehler, der ihn nennt.
- Der lokale Pseudo-Server kann kein Jump-Host sein (Spec 0032).

Befehle, Dateizugriffe und das Terminal laufen auf dem **letzten** Hop; die
Zwischenverbindungen bleiben so lange offen wie die Sitzung.

## 6. Host-Key-Prüfung (Trust on First Use)

Smart SSH akzeptiert **nie** einen unbekannten oder geänderten Host-Key
von selbst.

**6.1 Ablauf.** Der Schlüssel wird während des Handshakes geprüft, **bevor** die
Anmeldung beginnt: Zugangsdaten gehen nie an einen Server, dessen Schlüssel
nicht bestätigt ist. Das Ergebnis ist eines von drei:

- **Bekannt.** Der gespeicherte Schlüssel für diesen Host, diesen Port und diesen
  Schlüsselalgorithmus stimmt überein; der Aufbau läuft ohne Rückfrage weiter.
- **Unbekannt.** Für Host und Port ist (für diesen Algorithmus) noch kein
  Schlüssel gespeichert. Der Aufbau hält an, der Nutzer sieht Host:Port und den
  Fingerprint und entscheidet. Erst mit „Vertrauen" wird der Schlüssel
  gespeichert und der Aufbau von vorn wiederholt; bei einer Kette folgt für
  jeden weiteren unbekannten Hop eine eigene Abfrage.
- **Geändert.** Für Host, Port und Algorithmus ist ein anderer Schlüssel
  gespeichert. Das ist ein harter Stopp mit eigener, deutlich strengerer
  Warnung (Abschnitt 6.3).

**6.2 Fingerprint und Schlüsseltyp.** Der Fingerprint wird als `SHA256:` mit
Base64 angezeigt, wie ihn `ssh-keygen -l` und OpenSSH-Clients ausgeben, sodass
er sich mit einer Angabe des Server-Betreibers vergleichen lässt. Beide
Abfragen zeigen zusätzlich den Schlüsseltyp (z. B. `ssh-ed25519`), damit der
Nutzer Host:Port, Fingerprint und Typ gegen eine Quelle außerhalb der
Verbindung halten kann. Lässt sich der Typ nicht lesen, entfällt die Zeile.
Der Typ ist rein anzeigend und fließt nie in die Entscheidung ein. Präsentiert
der Server ein Host-Zertifikat, zählt der darin enthaltene Schlüssel.

**6.3 Dialog.**

- *Unbekannt:* Überschrift „Unbekannter Host", Host:Port, Fingerprint,
  Schlüsseltyp, Schaltflächen „Ablehnen" und „Vertrauen".
- *Geändert:* rot gestalteter Warndialog „Host-Schlüssel geändert" mit dem
  Hinweis auf einen möglichen Man-in-the-Middle-Angriff und der
  Gegenüberstellung „Bekannt" und „Jetzt angeboten"; Schaltflächen
  „Verbindung abbrechen" und „Trotzdem vertrauen". Der Anfangsfokus liegt auf
  der ablehnenden Schaltfläche; Escape lehnt ab (Spec 0100).

**6.4 Entscheidung.**

- „Ablehnen"/„Verbindung abbrechen", Escape und eine ungelöste Abfrage nach
  einer Stunde gelten als Ablehnung. Die Verbindung wird
  nicht aufgebaut, der Schlüssel nicht gespeichert; der Fehler trägt den Code
  `SSH_HOST_KEY_NOT_TRUSTED` bzw. beim Ablauf die Meldung „Die Host-Key-Abfrage
  für host:port ist abgelaufen. Die Verbindung wurde nicht aufgebaut, dem
  Schlüssel wird nicht vertraut."
- „Vertrauen" speichert den Schlüssel. Bei „geändert" ersetzt der neue Schlüssel
  den alten desselben Algorithmus, sodass der alte, womöglich kompromittierte
  Schlüssel nicht weiter gilt. Schlüssel anderer Algorithmen desselben Hosts
  bleiben nebeneinander bestehen.

**6.5 Wo die Abfrage erscheint.** Dieselbe Abfrage erscheint bei jedem
Verbindungsaufbau: aus der Serverliste, bei „Verbindung testen" im Server-Formular
(nach „Vertrauen" wird erneut getestet) und für eine von einem externen
MCP-Client ausgelöste Verbindung (Spec 0028). Sie steht über der Oberfläche,
auch wenn gerade ein anderer Tab aktiv ist. Ein Neuladen der Oberfläche
während der Abfrage lässt die wartende Verbindung nicht verschwinden: sie
bleibt als „wartet auf Host-Key" in der Sitzungsliste (Spec 0017).

**6.6 Ablage.** Bekannte Schlüssel liegen dauerhaft in einer Datei im
App-Datenverzeichnis, je Eintrag Host, Port und Schlüssel, getrennt nach
Algorithmus. Host und Port gelten so, wie sie im Profil stehen; es gibt keine
Zuordnung zwischen verschiedenen Schreibweisen oder Aliasen desselben Servers.
Unter Unix ist die Datei nur für den Besitzer lesbar. Sie wird atomar
geschrieben; ein Schreibfehler bei „Vertrauen" bricht die Verbindung ab, statt
den Schlüssel nur für diese Sitzung zu merken. Eine nicht lesbare oder defekte
Datei verhindert den App-Start mit einer Fehlermeldung; sie wird nie
stillschweigend durch eine leere ersetzt.

Das Server-Formular zeigt die für Host und Port gespeicherten Schlüssel
(Algorithmus und Fingerprint) schreibgeschützt an.

## 7. Fehler und Zeitgrenzen

Ein Verbindungsfehler trägt eine Kategorie, damit die Oberfläche eine
verständliche Meldung zeigen kann (Texte und Codes: Spec 0069, Spec 0024):

| Kategorie | Bedeutung |
|---|---|
| Verbindung fehlgeschlagen | allgemeiner Fehler beim Aufbau |
| Verbindung abgelehnt | Port zu oder kein SSH-Dienst dort |
| Host nicht gefunden | Name nicht auflösbar |
| Host nicht erreichbar | keine Route |
| Verbindung beendet | Gegenseite hat den Aufbau abgebrochen |
| Anmeldung fehlgeschlagen | Server hat die Zugangsdaten abgelehnt |
| Host-Key abgelehnt | siehe Abschnitt 6 |
| Zeitüberschreitung | eine Zeitgrenze (unten) ist abgelaufen |
| Jump-Host-Zyklus | siehe Abschnitt 5 |
| Zugangsdaten nicht auflösbar | Eintrag fehlt oder ist ungültig |
| Secret-Speicher nicht erreichbar | der Speicher der Zugangsdaten hat nicht geantwortet (Spec 0098, Spec 0101) |
| Kanalfehler | Fehler bei einem Kommando, einer Shell oder einem Dateizugriff |
| Sitzung verloren | eine bereits aufgebaute Verbindung ist abgerissen |
| fehlende Rechte | Dateizugriff ohne die nötigen Rechte (Spec 0020) |

Je Hop gilt eine Zeitgrenze von 10 Sekunden für Verbindung und Handshake und
60 Sekunden für die Anmeldung. Um den gesamten Versuch liegt ein
Sicherheitsnetz, das mit der Zahl der Hops wächst. Das Warten auf eine
Host-Key-Entscheidung zählt nicht dazu und kann nie als „vertraut"
enden. Scheitert das Lesen von Zugangsdaten in einer Kette, nennt die Meldung den
betroffenen Hop als `Benutzer@Host:Port`.

Jeder Schritt des Aufbaus (Namensauflösung, TCP-Verbindung, Tunnel, Handshake,
Host-Key-Prüfung, Anmeldung, Sitzung bereit) wird mit Dauer und Ergebnis in ein
Schritt-Protokoll geschrieben, das im Fehlerfall und beim Verbindungstest
zugeklappt unter der Meldung steht (ADR 0110). Es enthält keine
Zugangsdaten, nur die Art der Anmeldung, und geht nie in die Log-Datei oder den
Diagnose-Export.

## 8. Prüfbarkeit

Die Zusagen dieser Spec sind ohne echtes Netzwerk prüfbar: Auflösung und
Zirkelerkennung der Jump-Host-Kette, die Entscheidungslogik „bekannt /
unbekannt / geändert" und die Abbildung der Fehlerkategorien laufen gegen
Ersatzimplementierungen des Transports und der Host-Key-Ablage. Echte
Protokolltests (Einzelkommando, Terminal, Dateizugriff, Kette mit mehreren
Hops, Host-Key-Abfrage je Hop, Abbruch eines nie endenden Kommandos) laufen in
einer eigenen Testsuite gegen einen im Testprozess gestarteten SSH-Server; ein
Docker-Container oder ein externer Server ist dafür nicht nötig.

## 9. Sicherheitszusagen

- Kein unbekannter und kein geänderter Host-Key wird ohne ausdrückliche
  Entscheidung des Nutzers vertraut; Zeitablauf, Abbruch, Escape und
  Tab-Schließen sind Ablehnung.
- Zugangsdaten werden erst gesendet, nachdem der Schlüssel des jeweiligen Hops
  bekannt oder bestätigt ist.
- Ein geänderter Schlüssel wird nie wie ein unbekannter behandelt: eigener
  Dialog, kein „Vertrauen"-Standard, Fokus auf Ablehnen.
- Jeder Hop einer Kette wird einzeln geprüft; ein bestätigter Hop macht den
  nächsten nicht vertrauenswürdig.
- Ein Fehler im Fehlerpfad (Schreibfehler der Ablage, unlesbarer Schlüssel
  des Servers) endet als Fehler der Verbindung, nie als stilles „vertraut".
- Das Schritt-Protokoll beobachtet nur; keine Vertrauens- oder
  Anmeldeentscheidung hängt an ihm.

## 10. Grenzen

- **Kein automatischer Wiederaufbau.** Reißt die Verbindung ab, schlagen
  Kommandos und Dateizugriffe mit „Sitzung verloren" fehl. Der Tab zeigt
  „getrennt", sobald das Terminal endet; ohne geöffnetes Terminal bleibt der
  Statuspunkt auf „verbunden", bis der Tab geschlossen wird (Spec 0017). Der
  Nutzer schließt den Tab und verbindet neu.
- **Keine geteilten Verbindungen.** Ein Server-Tab hat seine eigene Verbindung;
  mehrere Tabs zum selben Server oder ein Pool werden nicht angeboten.
- **Neuer Algorithmus gilt als „unbekannt".** Hat ein Host bisher nur einen
  Schlüssel eines Algorithmus gespeichert und bietet er jetzt einen anderen an,
  erscheint die normale „Unbekannt"-Abfrage, nicht die Warnung „geändert";
  verglichen wird nur je Algorithmus. Der Fingerprint des anderen gespeicherten
  Schlüssels wird in der Abfrage nicht gezeigt.
- **Abbruch ohne Garantie.** Das Schließen eines Kommando-Kanals beendet nur
  das lokale Warten, nicht den Prozess auf dem Server (Spec 0027).
- **Host-Key-Datei ohne Austauschformat.** Die Ablage ist keine
  `known_hosts`-Datei; es gibt keinen Import oder Export der Schlüssel.
