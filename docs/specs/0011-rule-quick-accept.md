# Spec 0011 — Regel-Schnellvorschlag im Bestätigungsdialog

Status: umgesetzt
Zweck: Im Bestätigungsdialog führt ein Klick das Kommando aus und legt zugleich eine passende Allow-Regel an, damit ähnliche Kommandos künftig nicht erneut bestätigt werden müssen.
Bezüge: Spec 0002 (Filter-Engine), Spec 0009 (Regelverwaltung), Spec 0007 (Bestätigungsdialog), Spec 0077 (ungültige Muster), ADR 0002 (Abgleich mit und ohne `sudo`).

## 1. Ziel

Im Bestätigungsdialog für `Confirm`-Kommandos (weder Hard-Blacklist noch
Allow-Regel hat gegriffen) gibt es neben „Ausführen" und „Ablehnen" die
Schaltfläche **„Akzeptieren + Regel"** mit einem Aufklappmenü sinnvoller
Muster-Vorschläge.

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

## 3. Akzeptieren und Regel anlegen

Der Klick auf einen Vorschlag tut zwei Dinge:

1. Er legt für **jedes** Muster des Vorschlags eine Regel mit der Aktion
   **Allow** an, alle mit demselben Geltungsbereich, **atomar**: Ist ein
   Muster ungültig oder scheitert das Speichern, wird keine der Regeln
   gespeichert. Die Aktion ist fest, weil eine Confirm-Regel gegenüber dem
   Standardfall nichts bringt. Die Priorität ist 0, sofern nicht anders
   angegeben. Die Anlage läuft über dieselbe Prüfung wie das Regel-Formular
   (Spec 0009, Spec 0077).
2. Er löst die wartende Bestätigung auf, exakt wie ein normales „Ausführen".

Die Bestätigung wird auch dann aufgelöst, wenn die Regeln nicht angelegt
werden konnten (z. B. ungültiges Muster); der Fehler kommt getrennt zurück
und wird übersetzt angezeigt.

Die neuen Regeln wirken nicht rückwirkend auf das gerade laufende Kommando. Es
läuft aufgrund der ausdrücklichen Bestätigung in diesem Moment, nicht wegen
der neuen Regeln. Erst künftige, ähnliche Vorschläge profitieren.

## 4. Oberfläche

Das Aufklappmenü zeigt die Vorschläge mit übersetzter Beschriftung und den
Mustern des Vorschlags (bei mehreren, durch Komma getrennt). Daneben wählt der
Nutzer den Geltungsbereich: **„Dieser Server"** (Vorgabe, die sicherere Wahl),
„Global" oder „Tag" (mit Tag-Name). Die Auswahl entspricht der im
Regel-Formular (Spec 0009). Ein Klick auf einen Vorschlag schließt den Dialog.

## 5. Grenzen

- Es wird nur die Aktion Allow angeboten.
- Die Vorschläge sind keine sicherheitsrelevante Auswertung. Ob ein später
  passendes Kommando automatisch läuft, entscheidet allein die Filter-Engine
  (Spec 0002); Vorschläge ändern nichts daran, was sie erlaubt oder ablehnt.
- Teilkommandos, die bereits eine Allow-Regel abdeckt, bekommen kein Muster
  (Abschnitt 6). Die Vorschläge selbst legen nur Allow-Regeln an; ob das
  Kommando danach automatisch läuft, zeigt das Urteil je Vorschlag.

## 6. Abdeckung je Teilkommando und Urteil je Vorschlag

**Abdeckung.** Besteht das Kommando aus mehr als einem Teilkommando
(Verkettung, Pipeline), listet die Bestätigungskarte jedes Teilkommando in
der Reihenfolge des Kommandos mit seinem Zustand:

- **abgedeckt** – eine bestehende Allow-Regel lässt das Teilkommando
  automatisch laufen; die Karte nennt das Muster dieser Regel;
- **braucht eine Regel** – keine Regel greift;
- **fragt unabhängig von Regeln nach** – mit Grund: Umleitung, Substitution,
  nicht analysierbare Eingabe, Blacklist, eine Regel verlangt Bestätigung,
  eine Regel verbietet es.

Bei einem einzelnen Teilkommando erscheint diese Zeile nur, wenn ein
regelunabhängiger Grund greift. Die Zustände stammen aus der Auswertung der
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

**Hinweis nach dem Annehmen.** Genügen die angelegten Regeln nicht, damit
das Kommando automatisch läuft, erscheint im Chat ein kurzer Hinweis mit den
Gründen. Bei „läuft automatisch" erscheint keiner.
