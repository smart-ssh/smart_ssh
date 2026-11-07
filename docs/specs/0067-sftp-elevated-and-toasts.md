# Spec 0067 — Dateibrowser mit erhöhten Rechten und Erfolgsmeldungen

Status: umgesetzt
Zweck: Teil A: Der Dateibrowser kann auf Wunsch als root (oder anderer Nutzer) arbeiten, ohne `NOPASSWD`-Regel, wenn ein Sudo-Passwort im Server-Profil gespeichert ist (Spec 0018). Teil B: Jede Dateibrowser-Aktion meldet ihr Ergebnis sichtbar.
Bezüge: Spec 0020 (Dateibrowser), Spec 0054 (Aktionen, „Lokal öffnen“), Spec 0010 (Notiz-Karten unten rechts), Spec 0084 §9 und Spec 0085 (Widerruf), Spec 0102 (Startverzeichnis gilt nicht im erhöhten Modus), Spec 0003 (Server-Profil).

Ein Root-Dateibrowser ist das mächtigste Werkzeug der App; die Regeln in
Teil A sind deshalb Leitplanken, keine Vorschläge.

## Entscheidung

Der erhöhte Modus startet den SFTP-Server des Servers selbst mit
`sudo -n <sftp-server-pfad>` (wie WinSCP). Er braucht eine
`NOPASSWD`-Regel für `sftp-server`. Fehlt sie und ist für den Server ein
Sudo-Passwort gespeichert (Spec 0018), verwendet die App stattdessen dieses
Passwort (Issue #1191; es löst die frühere Entscheidung „nie ein Passwort“
ab, s. ADR 0058). Die Reihenfolge ist fest: (1) `sudo -n -l` erlaubt den
Befehl → `NOPASSWD`-Weg wie bisher; (2) sudo verlangt ein Passwort **und**
eins ist gespeichert → Passwortweg; (3) sonst die Meldung aus A3. Einen
Passwort-Dialog im Dateibrowser gibt es nicht.

---

## Teil A — Rechteerhöhung

### A1. Mechanismus

- Statt des normalen SFTP-Subsystems öffnet die App einen eigenen Kanal mit
  `sudo -n <sftp-server-pfad> -l INFO -f AUTHPRIV` und betreibt darüber
  denselben SFTP-Client. Die Argumente sind fest (nicht einstellbar):
  `sftp-server` schreibt damit jede Anfrage (gelesen, geschrieben, gelöscht)
  in das Auth-Log des Servers (Issue #1192). Erlaubt eine vorhandene
  sudoers-Regel nur den Aufruf ohne Argumente (`""`), öffnet die App den
  Kanal wie zuvor ohne Argumente; dann wird serverseitig nur der Start von
  `sftp-server` protokolliert.
- Im Dateibrowser zeigt das Banner des erhöhten Modus den Zusatz
  „Auf dem Server protokolliert.“, wenn die Logging-Argumente aktiv sind;
  ohne Logging steht kein Zusatz. Die lokale Audit-Zeile jeder Änderung
  nennt `server_logging`.
- `-n` (nicht interaktiv): Verlangt sudo ein Passwort, schlägt der Aufruf
  sofort fehl, statt zu hängen.
- **Passwortweg (Issue #1191):** Die App öffnet den Kanal mit
  `sudo -S -k -p '<marker>' <sftp-server-pfad> -l INFO -f AUTHPRIV`; `<marker>` ist ein pro
  Aktivierung zufälliges Token. Sie liest die Fehlerausgabe, bis der Marker
  erscheint, und schreibt erst dann das gespeicherte Passwort samt
  Zeilenumbruch, genau einmal; danach beginnt der SFTP-Handshake auf
  demselben Kanal. Erscheint der Marker nicht rechtzeitig (z. B. weil eine
  Regel oder ein Zeitstempel die Abfrage überspringt), wird der Kanal
  geschlossen und die Aktivierung scheitert — es ist dann kein
  Passwort-Byte gesendet worden. Erscheint der Marker ein zweites Mal
  (falsches Passwort), wird der Kanal geschlossen und gemeldet, dass das
  gespeicherte Sudo-Passwort abgelehnt wurde; ein erneuter Versuch mit
  demselben Passwort findet nicht statt. Meldet sudo für den Aufruf mit
  Logging-Argumenten „not allowed“, folgt genau ein weiterer Versuch ohne
  Argumente (mit neuem Marker); meldet sudo auch dann „not allowed“, gilt
  der Fall „nicht erlaubt“ aus A3. Das Passwort steht nie im Kommando, in
  Logs, im Audit, in Fehlermeldungen, im Chat oder im KI-Kontext.
- Die erhöhte Sitzung ist ein **zweiter, getrennter** Kanal neben dem
  normalen; der normale Browser bleibt unverändert. Für den lokalen
  Pseudo-Server (Spec 0032) gibt es keinen erhöhten Modus.

### A2. Pfad von `sftp-server`

- Der Pfad unterscheidet sich je Distribution (`/usr/lib/openssh/sftp-server`,
  `/usr/libexec/openssh/sftp-server`, `/usr/lib/ssh/sftp-server`,
  `/usr/libexec/sftp-server`, …). Die App erkennt ihn automatisch mit einem
  kurzen Probe-Kommando auf dem Server (bekannte Pfade auf Ausführbarkeit
  prüfen).
- **Override je Server-Profil** (Experten-Feld, eingeklappt, Standard
  „automatisch“), falls die Erkennung scheitert oder der Pfad nicht
  üblich ist. Ein Override muss ein einfacher absoluter Pfad sein, dessen
  Dateiname `sftp-server` ist; sonst wird er abgelehnt, damit über das Feld
  kein beliebiges Programm mit sudo gestartet und dafür eine Regel
  vorgeschlagen werden kann.

### A3. Voraussetzungsprüfung und verständliche Fehler

Vor dem Umschalten prüft die App, ob passwortloses sudo für genau diesen
Befehl erlaubt ist, in dieser Reihenfolge: (1) `sudo -n -l <pfad> -l INFO -f
AUTHPRIV` erlaubt → Kanal mit Logging-Argumenten; (2) sonst `sudo -n -l
<pfad>` erlaubt (bestehende Regel mit `""`) → Kanal ohne Argumente;
(3) sonst die Fehlerfälle unten bzw. der Passwortweg. Scheitert das, erscheint eine
verständliche Meldung mit konkreter Anleitung statt eines kryptischen Fehlers:

- „Für den erhöhten Modus braucht der Server eine sudo-Regel ohne Passwort
  für sftp-server.“ mit der **konkreten sudoers-Zeile** zum Kopieren, auf den
  ermittelten Pfad und den Login-Nutzer zugeschnitten, z. B.
  `stefan ALL=(root) NOPASSWD: /usr/lib/openssh/sftp-server -l INFO -f AUTHPRIV`.
  Die Regel erlaubt `sftp-server` nur mit genau diesen Argumenten (oder, bei
  einer älteren Regel mit `""`, ohne).
- Ein **ehrlicher Hinweis**: Diese Regel gibt dem SSH-Login
  passwortlosen Root-Dateizugriff. Wer den SSH-Schlüssel hat, kann jede Datei
  lesen und schreiben. Das ist bewusst zu entscheiden.
- Verlangt sudo ein Passwort und ist keins gespeichert, nennt die Meldung
  zusätzlich: Ein im Server-Profil gespeichertes Sudo-Passwort schaltet die
  erhöhten Rechte ebenfalls frei. sudoers-Zeile und Warnung bleiben.
- Weitere Fälle werden erkannt und benannt: `requiretty` in sudoers, ein
  nicht gefundener `sftp-server`, nicht installiertes sudo, ein ungültiger
  Zielnutzer oder Pfad, ein fehlgeschlagener Start.

**Eigentümer und Rechte von `sftp-server`.** Bevor die App eine sudoers-Zeile
vorschlägt (und vor dem sudo-Check), prüft sie lesend auf dem Server, dass
`sftp-server` — nach Auflösen von Symlinks — und **jedes übergeordnete
Verzeichnis bis `/`** root gehören und weder für die Gruppe noch für andere
beschreibbar sind. Das gilt auch für **jeden Symlink, den der konfigurierte
Pfad auf dem Weg durchläuft** (auch Verzeichnis-Links und Ketten): Der Link
selbst muss root gehören — seine Rechtebits zählen nicht, sie sind auf vielen
Systemen immer offen — und das Verzeichnis, das ihn enthält, samt allen
Eltern muss die Regel erfüllen. Gemeldet wird der erste auffällige Eintrag
(Link oder Verzeichnis). Mehr als 20 Links hintereinander oder ein Link-Kreis
gelten als unsicher. Die Prüfung läuft nie mit sudo und nutzt nur
POSIX-Werkzeuge. Ist ein Eintrag unsicher, oder scheitert die Prüfung oder
liefert nichts Verwertbares (im Zweifel unsicher):

- **sudo verlangt ein Passwort und eins ist gespeichert (Passwortweg):**
  wie „sudo erlaubt den Befehl bereits“ — der erhöhte Modus startet, im
  Dateibrowser steht die Warnung.
- **sudo verlangt ein Passwort (keins gespeichert) oder erlaubt den Befehl
  nicht:** Der erhöhte Modus bleibt aus, es wird **keine sudoers-Zeile** gezeigt. Die Meldung nennt
  den auffälligen Pfad und sagt, dass er root gehören und für Gruppe und
  andere nicht beschreibbar sein muss; auf so einem Server könnte jeder, der
  das Binary ersetzen kann, mit der Regel root werden.
- **sudo erlaubt den Befehl bereits:** Der erhöhte Modus startet wie bisher,
  im Dateibrowser steht aber eine Warnung mit demselben Text.

Die Prüfung verschärft nur: Sie kann die Zeile unterdrücken oder eine Warnung
hinzufügen, nie den erhöhten Modus dort freigeben, wo er vorher abgelehnt
wurde. Ist alles sicher, ändert sich nichts.

### A4. Anderer Nutzer

`sudo -n -u <nutzer> <sftp-server>`: Der Umschalter „Erhöhte Rechte“ erhöht
auf `root`. Daneben sitzt ein kleiner Pfeil-Knopf („Weitere Optionen zur
Rechteerhöhung“) mit dem Menüpunkt „Als anderer Nutzer …“. Er öffnet ein
kleines Feld „Ziel-Nutzer“ mit der Erklärung „Führt den Dateibrowser über sudo
als dieser Nutzer aus. Standard: root.“ und einem Bestätigen-Knopf („Erhöhen“).
Das Menü ist per Tastatur bedienbar (Enter/Leertaste öffnet, Escape schließt
und gibt den Fokus an den Pfeil-Knopf zurück). Ein Klick irgendwo außerhalb von Menü
und Feld schließt beide, ohne die Erhöhung zu ändern; ein Klick auf den
Pfeil-Knopf öffnet und schließt das Menü, ein Klick im Menü oder im Feld lässt
es offen. Auch das Umschalten der Erhöhung und das Ausblenden des
Dateibrowsers (Wechsel zum Terminal oder zu einem anderen Sitzungs-Tab)
schließen beide. Der zuletzt bestätigte Nutzer
ist im Feld für den Rest der Sitzung vorbelegt (nicht gespeichert). In der
Werkzeugleiste steht kein unbeschriftetes Textfeld. Gültig sind übliche
POSIX-Nutzernamen (Kleinbuchstaben, Ziffern, `_`, `-`; höchstens 32 Zeichen,
Beginn mit Buchstabe oder `_`). Die sudoers-Zeile aus A3 nennt dann diesen
Zielnutzer.

**Root-Sitzungen.** Beim Verbinden mit einem echten SSH-Server wird einmalig
`id -u` ausgeführt. Ist die Nutzer-ID `0`, blendet der Dateibrowser den
Umschalter samt Pfeil-Knopf und Nutzerauswahl komplett aus. Schlägt die
Prüfung fehl, läuft in die Zeitüberschreitung oder liefert keine Zahl, gilt
der Zustand als unbekannt und die Bedienelemente erscheinen wie bei jedem
anderen Nutzer; das Verbinden scheitert daran nie. Der lokale Pseudo-Server
führt die Prüfung nicht aus. Das Ausblenden betrifft nur die Oberfläche; die
Prüfungen aus A1–A3 bleiben unverändert.

### A5. Sicherheits-Leitplanken (nicht verhandelbar)

- **Expliziter Umschalter** je Dateibrowser-Ansicht („Mit erhöhten
  Rechten“), **nie** standardmäßig an und **nicht gespeichert**: Nach
  Verbindungstrennung oder App-Neustart ist er wieder aus.
- **Unübersehbare Kennzeichnung**, solange aktiv: farbiger Rahmen um den
  Browser und ein Banner „Erhöhte Rechte: root“ (bzw. Zielnutzer). Die
  Kennzeichnung bleibt auch bei verborgener Dateien-Ansicht sichtbar.
- **Bestätigungen bleiben** (Löschen, Überschreiben, rekursives chmod) und
  nennen den Modus („…als root löschen?“).
- **Nur manuelle Nutzung:** KI und MCP-Agenten bekommen über diesen Kanal
  **keinen** Zugriff. Deren Datei- und Kommando-Aktionen laufen unverändert
  durch Filter-Engine und Bestätigung, auch während der erhöhte Browser offen
  ist. Der erhöhte Kanal ist ausschließlich von den Browser-Befehlen
  erreichbar.
- **Start von `sftp-server` per Kommando:** Ein Kommando, das `sftp-server`
  startet, gilt immer als Rot und braucht immer eine Bestätigung — auch
  hinter `sudo`/`doas` mit Optionen und deren Argumenten, selbst wenn ein
  Argument in Anführungszeichen mit Leerzeichen steht (`sudo -p '[sudo]
  password:' -u root …/sftp-server`, `--prompt='x y'`, `-uroot`, `--`). Ein
  Optionsargument, das nur den Namen enthält (`sudo -p 'sftp-server' ls`),
  und bloße Erwähnungen (`ls`, `grep`, `cat` auf den Pfad) lösen nichts aus.
- **Protokolliert** mit Kennzeichnung „erhöht“, Zielnutzer und der Art, wie
  sudo authentifiziert hat (`nopasswd` oder `password`, nie das Passwort;
  Spec 0054, Sicherheitsmodell).
- **Widerruf wirkt sofort:** Wird der Modus ausgeschaltet, die Sitzung
  getrennt oder für einen anderen Nutzer neu aktiviert, wird der Kanal
  verworfen und kein weiterer Befehl läuft mit den alten Rechten
  (Spec 0085).
- **Automatisches Ausschalten bei Inaktivität** (Issue #1193): Findet im
  erhöhten Modus eine Zeit lang keine erhöhte Aktivität statt, schaltet die
  App ihn selbst aus, genau wie der Umschalter (Kanal verworfen, Banner und
  Rahmen weg, Ansicht lädt im normalen Modus neu). Als Aktivität zählt jeder
  Zugriff über den erhöhten Kanal (auch Auflisten) sowie das Laufen und das
  Ende einer erhöhten Operation; solange eine Operation läuft, wird nie
  ausgeschaltet, und die Frist beginnt danach neu. Die Frist stellt man in
  den Einstellungen (Sicherheit) ein: 5, 15, 30 oder 60 Minuten oder
  „Nie“; Standard 15 Minuten. Ein fehlender oder ungültiger gespeicherter
  Wert gilt als 15 Minuten, nie als „Nie“. Nach dem automatischen Ausschalten
  erscheint ein Hinweis, der stehen bleibt, bis man ihn schließt („Erhöhte
  Rechte nach N Minuten ohne Aktivität ausgeschaltet“). „Lokal öffnen“ und
  Uploads verhalten sich danach wie nach dem manuellen Ausschalten. Das
  Protokoll nennt als Grund „inactivity“ (manuell: „manual“).
- **Pro Server abschaltbar** (Issue #1194): Im Server-Formular (Bereich
  „Erweitert“) steht der Schalter „Erhöhte Rechte im Dateibrowser erlauben“;
  Standard an, auch für bestehende Profile und ältere Sicherungen. Ist er aus,
  zeigt der Dateibrowser dieses Servers den Umschalter, den Pfeil-Knopf und
  die Nutzerauswahl nicht, das Feld für den Pfad zu `sftp-server` ist im
  Formular gesperrt, und jeder Versuch, den erhöhten Modus zu starten, wird
  abgelehnt (Fehler „Für diesen Server nicht erlaubt“), ohne dass der Server
  befragt wird und ohne sudo. Lässt sich der Schalter nicht lesen, gilt er
  als aus. Wird er ausgeschaltet, während eine Sitzung erhöht ist, wird der
  Kanal sofort verworfen, die Ansicht lädt im normalen Modus neu, und ein
  Hinweis nennt den Grund. Der Schalter schränkt nur ein: Er erlaubt nichts,
  was A1–A3 verbieten.
- **„Lokal öffnen“** (Spec 0054): Wird eine Datei im erhöhten Modus
  geöffnet, läuft der spätere Upload **über denselben erhöhten Kanal**; die
  Bearbeitung merkt sich den Modus. Ist der Kanal inzwischen geschlossen,
  erscheint eine verständliche Meldung, nie ein stiller Upload als normaler
  Nutzer.
  Weicht die Identität der Bearbeitung von der aktuellen ab (anderer
  Ziel-Nutzer, erhöhter Modus inzwischen an oder aus), sagt die
  Bearbeitungsleiste das („Als www-data geöffnet, der erhöhte Modus läuft
  jetzt als root.“) und bietet statt „Hochladen“ nur „Als root hochladen“
  (bzw. „Als Login-Nutzer hochladen“, wenn der erhöhte Modus aus ist) an.
  Erst dieser bewusste Klick lädt einmal mit der aktuellen Identität hoch;
  danach gehört die Bearbeitung dieser Identität. Ein Upload mit der alten
  Identität wird weiterhin abgelehnt, nie still umgestellt. Die
  Bearbeitungskopie ist nur für den Nutzer lesbar (0600), weil
  Root-Dateien sensibel sein können.
- Im Passwortweg ist das gespeicherte Sudo-Passwort ein `SecretString`, wird
  nach dem Schreiben verworfen und ist der Redaktion bekannt. Die
  Fehlerausgabe von sudo gelangt trotzdem nie ungefiltert in Chat oder
  KI-Kontext.

---

## Teil B — Erfolgsmeldungen

### B1. Welche Aktionen

Jede Aktion aus Spec 0054 meldet ihr Ergebnis:

- Herunterladen (Datei: „`datei.conf` heruntergeladen nach `~/Downloads`“;
  Ordner: „Ordner `x` heruntergeladen — 42 Dateien“).
- Hochladen, Löschen, Umbenennen, Verschieben, Ordner anlegen, chmod (rekursiv
  mit Anzahl).
- Pfad kopieren, Inhalt kopieren („In die Zwischenablage kopiert“).
- Lokal öffnen und „Änderung hochgeladen“.

### B2. Form

- Die Meldung erscheint als Toast oben rechts im App-Fenster, unter dem
  Header, getrennt von den Notiz-Karten unten rechts (Spec 0010) und in
  derselben Optik. Er verdeckt auch bei kleinem Fenster (800×560) nicht das
  Eingabefeld des Chats.
- **Erfolg:** kurz, verschwindet nach einigen Sekunden.
- **Fehler:** bleibt stehen, bis der Nutzer ihn schließt, mit verständlichem
  Grund (Rechte, Ziel existiert, Verbindung weg).
- **Download:** Der Toast bietet „Im Finder/Explorer zeigen“ (Beschriftung je
  Betriebssystem).
- **Im erhöhten Modus** trägt die Meldung den Zusatz „(als root)“ (bzw.
  Zielnutzer).
- **Sammeloperationen** (Ordner, rekursiv, mehrere Dateien) melden **eine**
  Zusammenfassung statt eines Toasts je Datei.
- **Lang laufende Aktionen** zeigen weiter ihren Fortschritt; die
  Erfolgsmeldung kommt am Ende.
- Alle Texte gibt es auf Deutsch und Englisch.

### B3. Inhalt

Dateinamen und Zielpfade sind in Meldungen in Ordnung; **Dateiinhalt**
erscheint nie.

---

## Invarianten

- Der erhöhte Modus ist nie Standard, nie gespeichert und immer sichtbar
  gekennzeichnet.
- KI und MCP erreichen den erhöhten Kanal nie.
- Ein Sudo-Passwort wird nur verwendet, wenn es im Server-Profil gespeichert
  ist, sudo es ausdrücklich verlangt, und nur nach dem Prompt-Marker, genau
  einmal. Sonst gilt `sudo -n`; fehlende Berechtigung scheitert sofort und
  verständlich, nie durch Hängen.
- Das Audit des Einschaltens nennt, wie sudo authentifiziert hat
  (`nopasswd` oder `password`), nie das Passwort.
- „Lokal öffnen“ lädt eine im erhöhten Modus geöffnete Datei nur erhöht
  hoch, nie still als normaler Nutzer.
- Bestätigungen bleiben und nennen den Modus.

## Grenzen

- Der erhöhte Modus braucht auf dem Server eine passende sudoers-Regel; die
  App legt sie nie selbst an.
- Das konfigurierte Startverzeichnis (Spec 0102) gilt für den Login-Nutzer,
  nicht für den erhöhten Modus.
