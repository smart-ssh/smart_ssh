# Spec 0011 — Regel-Schnellvorschlag im Bestätigungsdialog

Status: umgesetzt
Zweck: Im Bestätigungsdialog legt der Nutzer je Teilkommando passende Allow-Regeln fest und speichert sie gesammelt, damit ähnliche Kommandos künftig nicht erneut bestätigt werden müssen.
Bezüge: Spec 0002 (Filter-Engine), Spec 0009 (Regelverwaltung), Spec 0007 (Bestätigungsdialog), Spec 0077 (ungültige Muster), ADR 0002 (Abgleich mit und ohne `sudo`).

## 1. Ziel

Im Bestätigungsdialog für `Confirm`-Kommandos (weder Hard-Blacklist noch
Allow-Regel hat gegriffen) kann der Nutzer je Teilkommando, das noch eine Regel braucht, einen Muster-Vorschlag
auswählen (Abschnitt 4) und die gewählten Regeln mit **„Regeln speichern (N)"**
anlegen. Eine Schaltfläche „Akzeptieren + Regel" für das ganze Kommando gibt es
nicht mehr.

## 2. Muster-Vorschläge

Ein Vorschlag besteht aus einer **Menge** von Mustern, die zusammen angelegt
werden. Es gibt höchstens sechs Vorschläge, vom Spezifischen zum Allgemeinen
geordnet; Vorschläge mit gleicher Muster-Menge erscheinen nur einmal.

**Teilkommandos.** Eine Verkettung oder Pipeline (`|`, `&&`, `||`, `;`)
wird so zerlegt wie von der Filter-Engine (Spec 0002, Abschnitt 4); die
Vorschläge entstehen je Teilkommando, weil die Engine jedes Teilkommando
einzeln prüft. Gleiche Teilkommandos zählen einmal. Lässt sich das Kommando
nicht sicher zerlegen, gibt es nur den Exakt-Vorschlag.

- **Exakt:** das Kommando selbst; bei mehreren Teilkommandos „exakt je
  Teilkommando" (ein Muster je Teilkommando, nie die ganze Zeile).
- **Pfad-begrenzt:** Enthält ein Teilkommando genau ein Pfadargument
  (absolut, oder relativ mit `/`), bleiben Programm, Unterkommando und
  Optionen wörtlich; der Pfad wird durch sein Elternverzeichnis plus `*`
  ersetzt, Zahlen direkt hinter einer Option durch `*`. `tail -n 100
  /var/log/syslog` → `tail -n * /var/log/*` (trifft `tail -n 50
  /var/log/auth.log`, nicht `/var/log/nginx/access.log` und nicht
  `/etc/shadow`; `*` überschreitet kein `/`, Spec 0060). Bei Programmen, die
  Verzeichnisse auflisten (`ls`, `tree`), kommt das Verzeichnis selbst
  hinzu: `ls -la /etc/nginx` → `ls -la /etc/*` und `ls -la /etc/nginx/*`.
  Kein Vorschlag, wenn das Elternverzeichnis `/` ist, der Pfad `.`/`..`/
  leere Segmente, einen Endschrägstrich, `://` oder Shell-Sonderzeichen
  enthält, es mehrere Pfadargumente gibt oder eine Option einen Pfad trägt.
  Es gibt keine Abfrage auf dem Server; ob ein Pfad ein Verzeichnis ist,
  folgt allein aus dem Programm.
- **Letztes Argument offen:** Ist das letzte Wort ein Argument ohne `-` und
  kein Pfad, wird nur dieses durch `*` ersetzt: `systemctl restart nginx` →
  `systemctl restart *`; `docker logs -f web` → `docker logs -f *`. Kein
  Vorschlag bei Pfaden (der pfad-begrenzte deckt sie ab) und wenn zwischen
  Programm und letztem Wort nichts steht (das wäre der Programm-Vorschlag).

Beide entstehen je Teilkommando (Teilkommandos ohne eigenes Muster bleiben
exakt) und stehen zwischen „Exakt" und „Unterkommando".
- **Unterkommando:** das Unterkommando mit beliebigen Argumenten und ohne
  Argumente, z. B. `systemctl status nginx` → `systemctl status` und
  `systemctl status *` (zwei Muster, ein Vorschlag). Das Unterkommando ist
  das erste Wort nach dem Programm, das nicht mit `-` beginnt. Bei bekannten
  Programmen werden Optionen davor übersprungen, bleiben aber **wörtlich** im
  Muster, damit die Regel kein anderes Unterkommando trifft: `git -C
  /srv/app pull` → `git -C /srv/app pull` und `git -C /srv/app pull *`
  (nicht `git -C /srv/app push`). Bekannt: `git` (`-C`, `-c`, `--git-dir`,
  `--work-tree`), `docker` (`--context`, `-H`/`--host`), `kubectl` (`-n`,
  `--namespace`, `--context`), `systemctl` (`--user`, `--system`).
  Teilkommandos ohne Unterkommando fallen auf die Programm-Variante zurück.
- **Programm:** alle Aufrufe des Programms (`<programm> *`), nur wenn das
  Teilkommando mehr als ein Wort hat. Bei `sudo`/`doas` ohne Optionen gilt das
  Programm dahinter (`apt *` statt `sudo *` oder `sudo apt *`); dank des
  Abgleichs mit und ohne `sudo` (ADR 0002) trifft `apt *` auch `sudo apt …`.
  Kein Vorschlag nennt ein reines `sudo`-Muster.

Wörter mit Anführungszeichen, Glob- oder Shell-Sonderzeichen fließen nicht in
ein Glob-Muster ein. Programme hinter anderen Wrappern (`env`, Variablen-
zuweisung) und `sudo` mit Optionen erhalten nur den Exakt-Vorschlag.

Jeder Vorschlag trifft das bestätigte Kommando: Seine Muster decken jedes
Teilkommando so ab, wie die Engine eine Allow-Regel abgleicht. Ein Vorschlag,
bei dem das nicht zutrifft, oder dessen Muster sich nicht übersetzen lässt
(Spec 0077), wird weggelassen. Regelunabhängige Prüfungen (Hard-Blacklist,
Umleitung, Substitution, undurchsichtige Eingabe, Risiko Rot) gelten weiter
und werden durch eine Regel nie aufgehoben.

Die Beschriftung ist übersetzt: Das Backend liefert nur die Art des Vorschlags
und Parameter (Programme bzw. Unterkommandos); die Oberfläche baut den Text
in der gewählten Sprache und zeigt die Muster dahinter.

## 3. Regeln anlegen

Das Speichern vorgemerkter Regeln (Abschnitt 4, Abschnitt 8) legt für **jedes**
Muster jeder vorgemerkten Regel eine Regel mit der Aktion **Allow** an, je
vorgemerkter Regel mit deren Geltungsbereich, **atomar**: Ist ein Muster
ungültig oder scheitert das Speichern, wird keine der Regeln gespeichert. Die
Aktion ist fest, weil eine Confirm-Regel gegenüber dem Standardfall nichts
bringt. Die Priorität ist 0. Die Anlage läuft über dieselbe Prüfung wie das
Regel-Formular (Spec 0009, Spec 0077).

Das Speichern löst die wartende Bestätigung **nicht** auf: Der Nutzer entscheidet
danach mit „Ausführen" oder „Ablehnen". Die neuen Regeln wirken nicht
rückwirkend auf das gerade wartende Kommando; es läuft nur aufgrund der
ausdrücklichen Bestätigung. Erst künftige, ähnliche Kommandos profitieren.

## 4. Oberfläche

Die Teilkommando-Liste (Abschnitt 6) steht immer im Bestätigungsdialog, auch bei
einem einzelnen Kommando. Oben fasst sie die Teilkommandos, die beim Öffnen schon
erlaubt sind, kompakt zusammen: eine kurze Zeile je Teilkommando mit grünem
Haken, die abdeckende Regel als sichtbarer Text in der Zeile (zusätzlich als Tooltip). Darunter steht jedes Teilkommando, das
noch entschieden werden muss, als eigene Zeile.

- Eine Zeile „braucht eine Regel" hat einen Umschalter mit Chevron-Symbol (nach rechts = zugeklappt, nach unten = aufgeklappt; `aria-expanded`,
  per Tastatur bedienbar). Aufgeklappt (direkt in der Zeile, kein Popup, auch
  auf schmalen Bildschirmen) zeigt sie den Geltungsbereich und die Vorschläge
  für dieses Teilkommando mit ihren Urteilen (Abschnitt 8). Der Geltungsbereich
  gilt je Teilkommando: **„Dieser Server"** (Vorgabe, die sicherere Wahl),
  „Global" oder „Tag" (mit Tag-Name), wie im Regel-Formular (Spec 0009).
- Ein Klick auf einen Vorschlag **merkt** die Regel vor: Die Zeile klappt zu und
  zeigt Haken, Muster und Geltungsbereich, als „noch nicht gespeichert"
  erkennbar. Die Zeile bleibt an ihrer Stelle, der Umschalter bleibt; die
  Vormerkung lässt sich ändern oder über „Entfernen" zurücknehmen. Die Urteile
  der Vorschläge anderer Zeilen berücksichtigen die vorgemerkten Regeln.
- Zeilen „fragt unabhängig von Regeln nach" und bereits erlaubte Teilkommandos
  haben keinen Umschalter.
- **„Regeln speichern (N)"** steht in der Fußzeile neben Ablehnen/Ausführen und ist ab N ≥ 1 bedienbar und speichert die
  vorgemerkten Regeln (Abschnitt 3); es führt das Kommando nicht aus und lehnt
  es nicht ab. Bei Erfolg wird aus der Vormerkung ein grüner Haken „abgedeckt
  durch Regel …"; die Zeilen bleiben in diesem Dialog an ihrer Stelle, der
  Umschalter bleibt (eine andere Regel lässt sich weiter vormerken). Erst beim
  nächsten Kommando mit einem solchen Teilkommando steht es in der Zusammenfassung
  oben. Bei einem Fehler erscheint eine Meldung, die Vormerkungen bleiben.
- „Ausführen" und „Ablehnen" bleiben unverändert. Bei nicht gespeicherten
  Vormerkungen steht daneben „Nicht gespeicherte Regeln: N". „Ausführen" speichert
  sie nicht; „Ablehnen" und „Ausführen" verwerfen die Vormerkungen.

## 5. Grenzen

- Es wird nur die Aktion Allow angeboten.
- Die Vorschläge sind keine sicherheitsrelevante Auswertung. Ob ein später
  passendes Kommando automatisch läuft, entscheidet allein die Filter-Engine
  (Spec 0002); Vorschläge ändern nichts daran, was sie erlaubt oder ablehnt.
- Teilkommandos, die bereits eine Allow-Regel abdeckt, bekommen kein Muster
  (Abschnitt 6). Die Vorschläge selbst legen nur Allow-Regeln an; ob das
  Kommando danach automatisch läuft, zeigt das Urteil je Vorschlag.

## 6. Abdeckung je Teilkommando und Urteil je Vorschlag

**Abdeckung.** Die Bestätigungskarte listet jedes Teilkommando (auch ein einzelnes) in
der Reihenfolge des Kommandos mit seinem Zustand:

- **abgedeckt** – eine bestehende Allow-Regel lässt das Teilkommando
  automatisch laufen; die Karte nennt das Muster dieser Regel;
- **braucht eine Regel** – keine Regel greift;
- **fragt unabhängig von Regeln nach** – mit Grund: Umleitung, Substitution,
  nicht analysierbare Eingabe, Blacklist, eine Regel verlangt Bestätigung,
  eine Regel verbietet es.

Zu den regelunabhängigen Gründen zählt auch eine Eskalation nach der
Engine (z. B. Lesen einer Datei mit Zugangsdaten, `sftp-server`, rotes
Risiko, eingelesener Serverinhalt, nicht prüfbares Kommando): Sie wird dem
Teilkommando zugeordnet, auf das sie zutrifft (in `cd /etc && cat shadow`
dem lesenden Teilkommando); lässt sie sich keinem zuordnen, trägt jedes sonst
abgedeckte Teilkommando den Grund. Die Karte zeigt ein Teilkommando nie als
abgedeckt, wenn der Ausführungsweg nachfragen würde. Lässt sich die Liste
nicht laden, steht an ihrer Stelle die gedämpfte Zeile „Regelabdeckung konnte
nicht geladen werden"; Ausführen und Ablehnen bleiben bedienbar. Die Zustände stammen aus der Auswertung der
Filter-Engine (Spec 0002, §4.8), nicht aus einem eigenen Abgleich.

**Nur unabgedeckte Teilkommandos.** Die Vorschläge (Abschnitt 2) enthalten
Muster nur für Teilkommandos, die nicht abgedeckt sind. Kleine Helfer wie
`wc -l`, `head -20` oder `sort` werden wie jedes andere Teilkommando
vorgeschlagen. Sind alle Teilkommandos abgedeckt, gibt es keinen Vorschlag.

**Urteil je Vorschlag.** Jeder Vorschlag sagt vorab, ob seine Annahme dazu
führt, dass genau dieses Kommando künftig automatisch läuft:

- **„läuft beim nächsten Mal automatisch"** – die Engine liefert mit den
  Mustern des Vorschlags als zusätzlichen Allow-Regeln im gewählten
  Geltungsbereich und nach allen vom Nutzer unabhängigen Eskalationen
  (z. B. rotes Risiko bei aktivierter Einstellung, Secret-Pfad, hinterlegtes
  Sudo-Passwort, Hinweis auf eingeschleuste Anweisungen) „automatisch";
- **„fragt weiterhin nach: …"** – sonst, mit den Gründen (z. B. Umleitung,
  Teilkommando ohne Regel, Risiko rot). Der Nutzer kann die Regel trotzdem
  anlegen; die Beschriftung macht die Grenze klar.

Ein Kommando mit Substitution, `bash -c`, `eval` oder nicht analysierbarer
Eingabe bekommt nie „läuft automatisch", solange die Engine es nicht selbst
automatisch zulässt. Ein Geltungsbereich, der auf diesem Server nicht gilt
(Server-Regel eines anderen Servers), zählt nicht. Ein Wechsel des
Geltungsbereichs oder des bearbeiteten Kommandotexts wertet die Urteile neu
aus. Die Auswertung ist rein lesend: Es wird nichts gespeichert, bevor der
Nutzer einen Vorschlag annimmt.

## 7. Hinweis zur Regel bei automatischer Ausführung

Läuft ein Kommando automatisch (Entscheidung „automatisch ausführen“, ohne
Bestätigung), zeigt seine Karte im Chat in der Kopfzeile (statt des Labels „Vorgeschlagenes
Kommando“ und eines Entscheidungs-Badges) ein Blitz-Symbol und: „Automatisch
ausgeführt · Regel `<Muster>` · <Geltungsbereich>“. Das Muster steht als Chip
mit Rahmen. Liegt das Ergebnis vor, steht der Exit-Code rechtsbündig in
derselben Kopfzeile (nicht mehr unter der Ausgabe). Bei wiederhergestellten
Chats ohne Regeldaten zeigt die Kopfzeile nur „Automatisch ausgeführt“. Der
Geltungsbereich ist „global“, „Server `<Name>`“ oder „Tag `<Tag>`“.

- Besteht das Kommando aus mehreren Teilkommandos, nennt die Zeile die Regel
  jedes Teilkommandos; deckt eine Regel mehrere Teilkommandos ab, erscheint sie
  nur einmal.
- Ein Klick auf das Muster öffnet den Reiter der Filterregeln mit dieser
  Regel zum Bearbeiten. Ist die Regel inzwischen gelöscht, steht das Muster
  als einfacher Text ohne Verweis da.
- Die Zeile erscheint nur live; beim Wiederöffnen eines gespeicherten Chats
  ist sie nicht gefordert. Kommandos, die erst nach einer Bestätigung liefen,
  und abgelehnte Kommandos zeigen keine Zeile. Das gilt gleich für Aktionen,
  die von einem externen Werkzeug (MCP) angefragt wurden.
- Die Zeile ist reine Anzeige: Sie stammt aus derselben Auswertung, die die
  Entscheidung getroffen hat, und ändert diese nicht.

## 7a. Ausgabe ausgeführter Kommandos in der Karte

Gilt für jede Karte eines ausgeführten Kommandos, ob automatisch oder nach
Bestätigung ausgeführt.

Die Ausgabe eines beendeten Kommandos (stdout und stderr getrennt) zeigt in der
Karte nur die ersten 8 Zeilen, wenn sie länger ist, dazu „… N weitere Zeilen“ und
eine Schaltfläche „Alle anzeigen“ / „Weniger anzeigen“ (`aria-expanded`, per
Tastatur bedienbar). Der Zustand gilt je Karte und wird nicht gespeichert; „In Notiz
übernehmen“ übernimmt immer den vollen Text. Ausgabe mit höchstens 8 Zeilen hat
keine Schaltfläche. Laufende Ausgabe bleibt unverändert.
Die Obergrenze von 2000 Zeichen und der Hinweis „abgeschnitten“ bleiben auch in der
eingeklappten Ansicht sichtbar.

## 8. Vorschläge je Teilkommando und Sammel-Speichern

Zwei zusätzliche Befehle der Schnittstelle (bestehende Befehle bleiben
unverändert). Beide ändern die Entscheidung der Engine nicht; jedes Urteil
stammt aus der Kandidaten-Auswertung der Engine (Abschnitt 6).

- **Vorschläge für ein Teilkommando.** Gegeben Sitzung, Kommando, Index des
  Teilkommandos (derselbe Index wie in der Abdeckung, Abschnitt 6), Geltungsbereich
  (dieser Server, global, Tag) und die bisher vorgemerkten Regeln (Muster mit je
  eigenem Geltungsbereich), liefert er höchstens sechs Vorschläge für genau dieses
  Teilkommando, vom Spezifischen zum Allgemeinen, in den Arten aus Abschnitt 2
  (exakt, pfad-begrenzt, letztes Argument offen, Unterkommando, Programm). Kein
  Muster bezieht sich auf ein anderes Teilkommando. Nur lesend; nichts wird
  gespeichert.
- **Zwei Urteile je Vorschlag.** *Teilkommando:* abgedeckt (läuft mit den
  vorgemerkten Regeln plus diesem Vorschlag automatisch) oder „fragt weiter“
  mit Gründen (Regel verlangt Bestätigung, nachgelagerte Eskalation, keine Regel,
  oder der Geltungsbereich trifft die Sitzung nicht — z. B. ein Tag, den der
  Server nicht hat). *Kommando:* ob das ganze Kommando automatisch läuft; „ja“
  nur, wenn die Engine für das ganze Kommando AutoExec liefert und keine
  Eskalation greift.
- **Teilkommando, das immer fragt** (Zustand „blockiert“, z. B. harte
  Blacklist, Umleitung, Eskalation, undurchsichtige Syntax): keine Vorschläge,
  stattdessen die Gründe.
- **Sammel-Speichern.** Speichert eine Liste vorgemerkter Regeln, jede mit eigenen
  Mustern und eigenem Geltungsbereich, in einem Schritt: alles oder nichts. Ist
  ein Muster ungültig (leer oder nicht übersetzbar, Spec 0077) oder eine
  vorgemerkte Regel ohne jedes Muster, wird nichts
  gespeichert und ein Fehler geliefert. Eine wartende Bestätigung bleibt offen;
  das Speichern löst sie nicht auf. Rückgabe: die neuen Regel-Kennungen.
