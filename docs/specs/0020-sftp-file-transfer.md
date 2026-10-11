# Spec 0020 — Dateitransfer (SFTP): Dateibrowser und KI-Dateizugriff

Status: umgesetzt
Zweck: Wie Smart SSH Dateien auf einem Server zeigt, überträgt und ändert. Es gibt zwei Wege mit unterschiedlicher Kontrolle: den Dateibrowser, den der Nutzer selbst bedient, und Dateiaktionen, die die KI vorschlägt.
Bezüge: Spec 0002 (Filter-Engine), Spec 0005 (Verbindung, Kanäle), Spec 0006 (Redaktion), Spec 0017 (Sitzungs-Tabs), Spec 0018 (Sudo-Passwort), Spec 0019 (Notiz-Diff), Spec 0028 (MCP), Spec 0054 (Aktionen im Dateibrowser), Spec 0067 (erhöhter Modus), Spec 0068 Teil 3 (Sudo-Ankündigung), Spec 0086 (Größengrenzen im Browser).

## 1. Ziel und Abgrenzung

Zwei Anwendungsfälle teilen sich dieselbe Verbindung, sind aber unterschiedlich
reguliert:

1. **Manueller Dateibrowser.** Der Nutzer navigiert selbst durch das
   Dateisystem des Servers, lädt Dateien hoch und herunter. Das ist eine
   direkte Nutzeraktion, vergleichbar mit dem interaktiven Terminal
   (Spec 0005): **keine Prüfung durch die Filter-Engine**, der Nutzer tut es
   selbst und sieht, was er tut.
2. **KI-Dateizugriff.** Die KI schlägt vor, eine Datei zu lesen oder zu
   schreiben. Das ist ein Vorschlag wie jeder andere und unterliegt derselben
   Kontrolle wie ein Shell-Kommando (Abschnitt 4).

## 2. Warum die KI Dateien über SFTP bekommt

Die KI kann Dateien auch über Shell-Kommandos schreiben (`echo`,
`cat <<EOF`), die durch die Filter-Engine laufen. Der Dateizugriff ist
nicht notwendig, aber besser kontrollierbar:

- **Echter Diff statt Kommandotext.** Bei einer Änderung an einer
  bestehenden Datei liest die App den alten Inhalt und zeigt einen
  Zeilen-Diff. Der Nutzer sieht, *was sich ändert*, nicht *was geschrieben
  wird*.
- **Keine Quoting-Fallstricke.** Sonderzeichen, Anführungszeichen,
  Backslashes und `$foo`-artige Inhalte brauchen bei Heredocs sorgfältiges
  Escaping; Fehler erzeugen still falsche Dateien. Bei der Dateiübertragung
  entfällt das Risiko.
- **Keine Längengrenzen** der Kommandozeile.

Der Dateizugriff schwächt die bestehende Kontrolle nicht: Ohne die Regeln aus
Abschnitt 4 wäre ein Dateischreibvorgang eine stille Umgehung der
Filter-Engine.

## 3. Transport

Dateizugriffe laufen als weiterer Kanal über **dieselbe** Verbindung wie
Kommandos und Terminal (Spec 0005): kein zweiter Verbindungsaufbau, keine
erneute Anmeldung, keine erneute Host-Key-Prüfung, auch nicht über eine
Jump-Host-Kette. Es wird ausschließlich das SFTP-Protokoll verwendet, kein
SCP; die Oberfläche verhält sich wie ein klassischer Transfer-Client.

Angeboten werden: Verzeichnis auflisten, Datei lesen, Datei schreiben,
Eintrag abfragen, löschen, umbenennen/verschieben, Ordner anlegen und
Rechte setzen. Je Eintrag sind Name, Pfad, Typ, Größe, Rechte und
Änderungszeit bekannt.

Der Dateikanal einer Sitzung wird beim ersten Dateizugriff geöffnet und
bleibt für die Dauer der Sitzung offen. Er gehört der Sitzung; kein Code
außerhalb der Anwendungslogik kann ihn ersetzen (Spec 0085 A3).

## 4. KI-Zugriff: Aktionen und ihre Kontrolle

Zusätzlich zu `SuggestCommand` kennt die KI zwei Dateiaktionen:
`ReadRemoteFile` (Datei lesen) und `WriteRemoteFile` (Datei schreiben).

### 4.1 Lesen

Lesen wird wie ein lesendes Kommando behandelt und läuft durch die
**Filter-Engine**. Dafür wird die Aktion auf ein gleichwertiges Kommando
abgebildet: `sftp-read <pfad>`. Nutzer schreiben damit gewöhnliche Regeln
(z. B. Allow für `sftp-read /etc/nginx/*`, Deny für `sftp-read /etc/shadow`)
mit derselben Syntax und Präzedenz wie für Shell-Kommandos. Pfade werden vor
der Auswertung normalisiert (`.`, `..`, doppelte `/`), damit sich ein Muster
nicht über Pfadumwege umgehen lässt (Spec 0060).

Der gelesene Inhalt läuft vor der Rückgabe an die KI durch die Redaktion
(Spec 0006), wie jede Kommandoausgabe, und wird im KI-Kontext als nicht
vertrauenswürdig gekennzeichnet (Spec 0039).

Dateien über **256 KB** werden mit klarer Meldung an Nutzer und KI abgelehnt,
statt vollständig in den KI-Kontext geladen zu werden. Die Grenze ist fest,
nicht einstellbar. Sie wird anhand der vorab abgefragten Größe geprüft;
liefert diese Abfrage keinen Wert, wird trotzdem gelesen.

### 4.2 Schreiben

Die sicherheitskritischste Aktion dieser Spec. Ablauf:

1. **Filter-Engine.** Die Aktion wird auf `sftp-write <pfad>` abgebildet und
   wie in 4.1 ausgewertet. Eine Deny-Regel blockiert wie gewohnt.
2. **Nie ohne Anzeige.** Auch bei einer Allow-Regel wird nie ohne
   Bestätigung geschrieben; es gibt hier kein automatisches Ausführen. Ein
   Dateischreibvorgang ist schwerer rückgängig zu machen und zu überblicken
   als ein Kommando, und typische Allow-Regeln mit Platzhaltern
   (`sftp-write /etc/nginx/*`) würden sonst weitreichende, unsichtbare
   Änderungen erlauben. Aktionen, die über MCP kommen, brauchen ebenfalls
   immer eine Bestätigung (Spec 0028).
3. **Änderungs-Vorschau.** Vor der Bestätigung liest die App die Zieldatei
   (sofern vorhanden) und zeigt im Dialog den Diff zwischen altem und neuem
   Inhalt, mit derselben zeilenbasierten Darstellung wie bei
   Notiz-Vorschlägen (Spec 0019). Existiert die Datei nicht, zeigt der
   Dialog den vollen Inhalt als neue Datei ohne Hervorhebung. Ist die
   bestehende Datei nicht als Text lesbar (Binärdatei), zeigt der Dialog
   stattdessen einen Hinweis mit alter und neuer Größe.
4. **Automatisches Backup.** Vor dem Überschreiben einer bestehenden Datei
   legt die App auf dem Server eine Sicherungskopie
   `<pfad>.smartssh-backup-<zeitstempel>` an. Der Backup-Pfad steht im
   Chat-Ergebnis.
5. **Ergebnis.** Nach der Bestätigung wird geschrieben; Erfolg oder Fehler
   samt Backup-Pfad gehen als Ergebnis in den KI-Kontext zurück.

### 4.3 Privilegierte Schreibzugriffe

Die Dateisitzung läuft mit den Rechten des SSH-Login-Nutzers. Ein Schreiben
auf eine root-eigene Datei (`/etc/nginx/nginx.conf`) scheitert deshalb mit
„Zugriff verweigert“, sofern man nicht als root verbunden ist. Ist für den
Server ein Sudo-Passwort hinterlegt (Spec 0018), gilt:

1. Der reguläre Schreibversuch kommt zuerst. Gelingt er, ist nichts weiter
   zu tun.
2. **Keine stille Eskalation.** Dass das hinterlegte Sudo-Passwort für das
   Schreiben verwendet werden kann, steht **schon im Bestätigungsdialog**,
   bevor der Nutzer bestätigt (Spec 0068 Teil 3, Spec 0018 Abschnitt 7).
   Nur dann, wenn der Dialog es angekündigt hat, darf nach einem Rechte-Fehler
   mit Sudo geschrieben werden; eine zweite Rückfrage gibt es nicht.
3. Der Inhalt wird in eine temporäre Datei im Home-Verzeichnis des
   Login-Nutzers geschrieben und dann mit `sudo -S install -m <modus> <temp>
   <ziel>` an den Zielort gebracht; die temporäre Datei wird danach entfernt.
   `install` setzt Rechte und Eigentümer des Ziels in einem Schritt, statt sie
   von der temporären Datei zu erben. Eine bestehende Datei behält ihren
   Modus, eine neue bekommt 0644.
4. Das Backup aus 4.2, Punkt 4 entsteht in diesem Fall ebenfalls mit Sudo
   (`sudo -S cp -p`), weil die Zieldatei sonst nicht kopierbar wäre.
5. Ist **kein** Sudo-Passwort hinterlegt (oder wurde es nicht angekündigt),
   wird der Fehler unverändert gemeldet, mit dem Hinweis, dass für den Pfad
   erhöhte Rechte nötig sind. Es gibt keinen stillen Ersatzweg.

Auch Lesen kann an Rechten scheitern. Dort wird der Fehler gemeldet, ohne
Sudo-Eskalation; die KI kann stattdessen ein regulär geprüftes
`sudo cat <pfad>` vorschlagen.

### 4.4 Was die KI nicht darf

Löschen, Umbenennen und Ordner anlegen werden der KI **nicht** als
Dateiaktionen angeboten. Will sie etwas löschen oder verschieben, schlägt
sie ein gewöhnliches Shell-Kommando vor, das durch die Filter-Engine
inklusive der harten Sperrliste läuft (Spec 0002, Abschnitt 3.1). Für diese
Operationen bietet SFTP keinen der Vorteile aus Abschnitt 2 (kein Diff, kein
Quoting-Problem), aber ein zusätzliches Umgehungsrisiko; einen zweiten Weg
gibt es deshalb nicht.

## 5. Manueller Dateibrowser

Der Nutzer kann Verzeichnisse auflisten und wechseln, Dateien herunterladen
und hochladen, Einträge löschen, umbenennen, verschieben, Ordner anlegen und
Rechte ändern; die einzelnen Aktionen und ihre Bestätigungen beschreibt
Spec 0054.

- **Keine Filter-Engine.** Direkte Nutzeraktionen, wie im interaktiven
  Terminal (Spec 0005, Abschnitt 1).
- **Nativer Dateidialog.** Herunterladen in einen gewählten Pfad und
  Hochladen laufen über den Dialog des Betriebssystems. Es wird nie ohne
  explizite Auswahl auf die lokale Festplatte geschrieben oder von ihr
  gelesen; welche lokalen Dateien gelesen werden dürfen, regelt 5.2.
- **Bestätigung.** Löschen verlangt immer eine Rückfrage, auch als
  Nutzeraktion, damit ein Fehlklick nichts zerstört.
- **Übertragungen** laufen asynchron, blockieren die Sitzung nicht und zeigen
  ihren Fortschritt.

### 5.1 Oberfläche

Der Dateibrowser ist eine umschaltbare Ansicht im rechten Bereich neben dem
Terminal, mit dem Umschalter „Terminal | Dateien“. Der rechte Bereich ist der
„direkte Zugriff auf den Server“, links liegt der KI-Chat. Der Browser zeigt
eine Pfadleiste mit Navigation und eine Dateiliste (Name, Größe, Rechte,
Änderungsdatum). Aktionen stehen im Kontextmenü und im Drei-Punkte-Menü
(Spec 0054), Upload geht per Button oder Drag-and-Drop aus dem
Betriebssystem. Spaltenbreiten und Bereichsaufteilung sind verstellbar
(Spec 0053).

### 5.2 Welche lokalen Dateien gelesen werden dürfen

Die Oberfläche ist keine Vertrauensgrenze: Sie zeigt auch KI-erzeugte
Inhalte. Ein lokaler Pfad gilt deshalb nicht schon, weil die Oberfläche ihn
nennt. Für Upload, die Überschreib-Vorschau beim Upload und die
Änderungserkennung von „Lokal öffnen“ liest die App nur Dateien, die der
Nutzer in dieser Sitzung freigegeben hat:

- **Ausgewählt:** im nativen Öffnen-Dialog des Upload-Buttons oder im
  Ordner-Dialog von „Ordner hochladen“. Die App öffnet den Dialog selbst; die
  Oberfläche kann keinen Pfad vorgeben.
- **Abgelegt:** per Drag-and-Drop aus dem Betriebssystem auf das Fenster.
  Maßgeblich sind die Pfade, die das Betriebssystem beim Ablegen meldet, nicht
  ein von der Oberfläche genannter Pfad. Ein Ablegen gilt für die Sitzung, deren
  Dateibrowser es entgegennimmt, und verfällt nach kurzer Zeit, wenn es
  niemand entgegennimmt.
- **Eigene Bearbeitungskopie:** Dateien in der Bearbeitungskopie dieser
  Sitzung („Lokal öffnen“). Die Bearbeitungskopie einer anderen Sitzung zählt
  nicht.

Ist ein Ordner freigegeben, gelten auch die Dateien darunter als freigegeben;
darauf beruht der Ordner-Upload (Spec 0054, Teil 3), der jede Datei einzeln
gegen die Freigabe prüft, kurz bevor er sie liest. Ob ein Pfad unter einer
Freigabe liegt, wird am tatsächlichen Ziel entschieden: Ein Pfad mit `..`
oder ein symbolischer Link, der aus der Freigabe hinausführt, ist nicht
freigegeben.

Jeder andere lokale Pfad wird abgelehnt, bevor etwas gelesen wird: Upload und
Überschreib-Vorschau scheitern mit einer Meldung, die Änderungserkennung
meldet keinen Zeitstempel. Freigaben bestehen nur im Speicher und enden, wenn
die Sitzung getrennt wird.

### 5.3 Ergebnisse und Fehler

Jede Aktion meldet ihr Ergebnis (Spec 0067, Teil B). Ein Fehler beim
Dateizugriff erscheint mit verständlichem Grund, etwa fehlende Rechte,
bestehendes Ziel oder getrennte Verbindung; die Verbindung selbst bleibt
dabei bestehen.

### 5.4 Nur der aktive Tab nimmt Drops an

Jede geöffnete Sitzung bleibt beim Tab-Wechsel bestehen (Spec 0017). Ein
Ablegen aus dem Betriebssystem wird nur vom Dateibrowser des **aktiven Tabs**
entgegengenommen, und nur, solange dessen Ansicht „Dateien“ gewählt ist;
sonst würden mehrere Hintergrund-Tabs gleichzeitig zum Ziel.

## 6. Zugesichertes Verhalten (durch automatische Tests belegt)

- Die Abbildung auf `sftp-read`/`sftp-write`, die Pfad-Normalisierung, die
  Größengrenze, die Redaktion gelesener Inhalte, der Backup-Pfad, die
  Diff-Vorschau und die Sudo-Ankündigung sind mit Test-Doubles geprüft.
- Upload, Download, Verzeichnisliste, Umbenennen und Löschen sind gegen einen
  echten SFTP-Server im Test geprüft, auch über den erhöhten Kanal
  (Spec 0067).

## 7. Grenzen

- **Symbolische Links** werden wie normale Einträge aufgelistet. Ob ein
  Schreibvorgang das Ziel oder den Link ersetzt, ist nicht festgelegt und
  hängt vom Server ab; das betrifft etwa Pfade unter `sites-enabled`.
- **Backups** (`.smartssh-backup-*`) sammeln sich auf dem Server an; die App
  räumt sie nicht auf und listet sie nicht gesondert.
- **Große Verzeichnisse** (mehrere tausend Einträge) werden vollständig
  geladen, ohne Seitenaufteilung.
- Die 256-KB-Grenze für die KI gilt nur anhand der vorab abgefragten Größe
  (4.1); eine zwischen Abfrage und Lesen gewachsene Datei wird dort nicht
  erneut geprüft. Für den Dateibrowser gilt die engere Regel aus Spec 0086.
