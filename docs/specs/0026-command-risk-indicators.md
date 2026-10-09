# Spec 0026 — Risiko-Hinweise für KI-Vorschläge (Server-Risiko / Daten-Risiko)

Status: umgesetzt
Zweck: Jeder KI-vorgeschlagenen Aktion wird eine Risiko-Einschätzung auf zwei unabhängigen Achsen beigefügt. Sie ist ein Hinweis neben der Filter-Entscheidung; nur über die Einstellung aus Spec 0092 kann ein rotes Risiko eine Bestätigung verlangen.
Bezüge: Spec 0002 (Zerlegung, Normalisierung und Begrenzungen werden gemeinsam genutzt), Spec 0007 (Kernschleife), Spec 0020 (SFTP-Aktionen), Spec 0029 (Anordnung der Badges), Spec 0074 (Auswertung der Zweitmeinung), Spec 0092 (Rot verlangt Bestätigung), ADR 0024, ADR 0084.

## 1. Ziel

Jede Aktion vom Typ `SuggestCommand`, `ReadRemoteFile` oder
`WriteRemoteFile` bekommt eine Einschätzung auf zwei Achsen:

- **Server-Risiko** (gelb/rot): Könnte die Aktion dem Server schaden
  (destruktiv, irreversibel, dienstunterbrechend)?
- **Daten-Risiko** (gelb/rot): Könnte die Aktion sensible Daten (Passwörter,
  Schlüssel, Secrets) in den Chatverlauf und damit zum KI-Anbieter fließen
  lassen?

Es gibt **kein „grün"-Badge**: Fehlt ein Badge, bedeutet das „nach bekannten
Mustern unauffällig", nicht „sicher".

**Nicht verhandelbar:** Die Einschätzung ersetzt die Filter-Engine (Spec 0002)
nicht, blockiert nichts und führt nichts aus. Sie ändert keine Entscheidung
`Deny`, und sie macht nie aus `Confirm` ein `AutoExec`. Sie ist auch bei
automatisch ausgeführten Kommandos sichtbar. Einzige Ausnahme ist die
Einstellung „Bei rotem Risiko immer nachfragen" (Spec 0092): Ist sie an
(Standard), wird ein automatisch ausführbarer Vorschlag mit Rot auf einer
Achse bestätigungspflichtig. Auch das ist nur Eskalation.

## 2. Regelbasierte Einschätzung

Jede Achse hat die Stufen *keine*, *gelb*, *rot*. Zu jeder Stufe gehört eine
kurze Begründung (welches Muster gegriffen hat). Außerdem ist vermerkt, ob die
KI-Zweitmeinung beteiligt war.

Die Muster sind zwei fest eingebaute Listen, eine je Achse, mit denselben
Mustertypen wie die Filterregeln (Spec 0002, Abschnitt 2). Nutzerregeln
verändern sie nicht.

**Segmentierung:** Verkettungen (`&&`, `;`, `|`, Command-Substitution) werden
exakt wie in der Filter-Engine zerlegt (Spec 0002, Abschnitt 4). Jedes
Teilkommando wird einzeln eingestuft; je Achse gilt das höchste Level aller
Teile.

**Dieselbe Normalisierung und dieselben Begrenzungen wie die Filter-Engine.**
Der Klassifizierer vergleicht gegen das **normalisierte** effektive Kommando
(Spec 0002, Abschnitt 4, Punkt 6: Wrapper, Rechteerhöhung und
Variablen-Präfixe abgeschält), nicht gegen den Rohtext. Sonst würden
`sudo cat /etc/shadow` oder `env shutdown -h now` als „kein Risiko" durchgehen,
weil die Muster am Kommandoanfang ansetzen. Ebenso gelten Längen- und
Verschachtelungsgrenze aus Spec 0002, Abschnitt 4, Punkt 7: Der
Klassifizierer steigt nicht ungebremst in verschachtelte `$(...)` ab. Ein
Kommando über dem Längenlimit gilt bei eingeschalteter Einstellung aus
Spec 0092 als rot, weil seine Einstufung nicht belastbar ist.

**Code in Shell-`-c`-Aufrufen wird mitbewertet.** Steckt ein Kommando im
Code-Argument eines Shell- oder Interpreter-Aufrufs (`bash -c 'shutdown now'`,
`sh -c "cat /etc/shadow"`, auch hinter `sudo` oder Wrappern wie `env bash -c
'…'`), wird dieser Code zusätzlich wie ein direkt eingegebenes Kommando
bewertet: mit derselben Segmentierung, Normalisierung und denselben Mustern
sowie derselben Erkennung von `-c`-Code wie in der Filter-Engine (Spec 0002,
Abschnitt 4.6). Je Achse gilt das höchste Level aus äußerem Aufruf und
innerem Code; `bash -c 'shutdown -h now'` ist also Server-Risiko rot wie
`shutdown -h now`. Verschachtelte Aufrufe (`bash -c "sh -c 'reboot'"`) werden
Ebene für Ebene ausgepackt, höchstens bis zur Verschachtelungsgrenze aus
Spec 0002; darüber hinaus wird nicht weiter ausgepackt (kein Absturz), die
äußeren Ebenen bleiben bewertet. Die Bewertung kann dadurch nur strenger
werden.

**Beispiele Server-Risiko.** Rot: `rm -rf *`, `dd if=* of=/dev/*`, `mkfs*`,
Fork-Bomb, `shutdown*`/`reboot*`/`poweroff*`, `iptables -F*`,
`chmod -R 777 /*`. Gelb: `rm *` (ohne `-rf`), `systemctl stop/restart *`,
`apt/yum remove *`, `git reset --hard*`, `kill *` (ohne PID 1).

**Beispiele Daten-Risiko.** Rot: `cat`/`less`/`head`/`tail` auf `*id_rsa*`,
`*.pem`, `*.key`, `*.env`, `*credentials*`, `*shadow*`, `*.aws/credentials*`;
`env`/`printenv`; `mysqldump`/`pg_dump` ohne Ziel-Redaction; SQL mit
`SELECT * FROM *user*`/`*password*`. Gelb: `find` mit `-name *.key`-artigen
Mustern, `ls` in `~/.ssh` oder `/etc`, `grep` nach `password`/`secret`/`token`
in Dateien.

**Diese Listen sind Startpunkte ohne Anspruch auf Vollständigkeit.** Sie
blockieren nichts; ein fehlendes Muster ist eine unvollständige Warnung, kein
Loch in der Filter-Engine. Mit der Einstellung aus Spec 0092 hängt allerdings
die zusätzliche Rückfrage bei Rot an diesen Listen: Ein inhaltlich rotes
Kommando, das kein Muster trifft, löst sie nicht aus.

**SFTP-Aktionen.** `ReadRemoteFile` und `WriteRemoteFile` werden gegen den
Dateipfad eingestuft, abgebildet auf die Pseudokommandos `sftp-read <pfad>`
und `sftp-write <pfad>` aus Spec 0020, Abschnitt 4.1 (dieselbe Konvention wie
für die Filter-Engine).

## 3. Optionale KI-Zweitmeinung (nur Daten-Risiko)

Die Zweitmeinung gibt es bewusst nur für die Daten-Achse: Ob ein Pfad trotz
unauffälligem Namen sensibel sein könnte, ist eine semantische Frage; Server-
Schaden lässt sich gut musterbasiert erfassen.

1. **Standardmäßig aus** (Opt-in in den Einstellungen). Das Kommando an einen
   weiteren KI-Anbieter zu schicken ist selbst ein zusätzlicher Datenfluss
   und muss ausdrücklich gewählt werden.
2. **Eigener, frei wählbarer Provider** aus den bereits konfigurierten. Der
   Hinweistext empfiehlt ein lokales Modell (z. B. Ollama), damit auch die
   Zweitmeinung nicht zwingend an einen weiteren externen Anbieter geht. Die
   Wahl ist eine app-weite Einstellung und wird beim Verbinden einer Sitzung
   gelesen.
3. **Minimaler Kontext:** Die Anfrage enthält ausschließlich das Kommando bzw.
   den Pfad als Text, keinen Chatverlauf und keine Server-Notizen. Gefragt
   wird sinngemäß, ob die Ausgabe sensible Daten enthalten könnte, die nicht an
   einen KI-Anbieter gehen sollten; die Antwort lautet none/yellow/red mit
   kurzer Begründung.
4. **Nur Eskalation, nie Abschwächung.** Das Endergebnis der Daten-Achse ist
   das Maximum aus regelbasiertem und KI-Ergebnis. Die KI kann `none` zu
   `yellow` oder `yellow` zu `red` anheben, ein regelbasiertes `red` aber nie
   absenken. Eine probabilistische Zweitmeinung darf eine deterministische
   Warnung nicht entkräften, auch nicht per Prompt-Injection über den
   Kommandotext. Enthält die Antwort mehrere Urteilswörter, gilt das
   höchste (Spec 0074).
5. **Zeitpunkt:** Die regelbasierte Einschätzung wird sofort angezeigt. Die
   Zweitmeinung wird danach eingeholt und per Aktualisierung nachgereicht. Ein
   automatisches Ausführen wartet jedoch auf sie (Spec 0092, A3).
6. **Nicht verfügbar ist sichtbar, nie still.** Kann die Zweitmeinung zu
   einer Aktion nicht eingeholt werden – der Provider antwortet mit einem
   Fehler (abgelaufener oder gedrehter Schlüssel, Rate-Limit, Netzwerk,
   nicht laufendes Ollama) oder die Antwort enthält kein erkennbares Urteil –,
   unterscheidet sich das von „geprüft, unauffällig":
   - Die Aktionskarte zeigt den Hinweis „KI-Zweitmeinung nicht verfügbar";
     das Badge behält die regelbasierte Stufe. Ein regelbasiertes `red` wird
     dadurch nie gesenkt.
   - **Fail closed, pro Aktion:** Eine Aktion, die sonst automatisch liefe,
     wird bestätigungspflichtig (Grund: Zweitmeinung nicht verfügbar). Das
     gilt unabhängig von der Einstellung „Bei rotem Risiko immer nachfragen"
     (Spec 0092), weil hier keine rote Einstufung vorliegt, sondern eine
     fehlende Schutzschicht. Eine Aktion, die ohnehin eine Bestätigung
     verlangt oder abgelehnt wird, bleibt davon unberührt.
   - Jeder Provider-Fehler wird mit `warn` und dem Fehlercode protokolliert,
     ohne Kommando und ohne Fehlertext des Providers (Spec 0094). Auch die
     Hinweise enthalten nur feste Texte.
7. **Nicht einrichtbar beim Verbinden:** Ist die Zweitmeinung in den
   Einstellungen aktiv, lässt sich aber beim Verbinden nicht einrichten
   (gewählter Provider gelöscht, ungültige Provider-Kennung, Zugangsdaten
   nicht auflösbar), zeigt die Sitzung einmalig einen Hinweis, dass die
   Zweitmeinung (und der Injection-Check aus Spec 0039 §5.2) in dieser
   Sitzung nicht aktiv ist. Die Sitzung läuft sonst wie bisher; es wird nicht
   jede Aktion bestätigungspflichtig. Ist die Zweitmeinung ausgeschaltet,
   bleibt es still.

## 4. Darstellung

1. Zwei kleine, getrennte Badges („Server", „Daten") an der Aktionskarte und
   im Bestätigungsdialog, nur sichtbar, wenn die Stufe nicht „keine" ist.
   Der Tooltip zeigt die Begründung (das gegriffene Muster bzw. die
   KI-Begründung). Anordnung: Spec 0029.
2. Ist die Zweitmeinung aktiv und noch ausstehend, erscheint ein dezenter
   Lade-Indikator neben dem Daten-Badge (bzw. an dessen Stelle, wenn die
   Regeln „keine" ergaben). Er verschwindet oder wird zu gelb/rot, sobald die
   Antwort da ist.
3. Ein Hinweistext sagt: „Einschätzung basierend auf bekannten Mustern —
   keine Garantie." Es gibt kein „sicher" oder „geprüft" ohne Einschränkung.
4. Die Badges erscheinen nur für KI-vorgeschlagene Aktionen, **nicht** im
   manuellen SFTP-Dateibrowser (Spec 0020, Abschnitt 5) und nicht für direkte
   Terminal-Eingaben: eigene bewusste Aktionen brauchen keine Warnung vor sich
   selbst.

## 5. Grenzen

- Die Muster-Listen lassen sich nicht durch den Nutzer erweitern.
- Die Zweitmeinung betrifft nur das Daten-Risiko, nicht das Server-Risiko.
