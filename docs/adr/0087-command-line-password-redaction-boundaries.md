# ADR 0087 — Grenzen der Redaction von Kommandozeilen-Passwörtern

Status: angenommen · Spec: 0095 · Backlog: BL-0118

## Kontext

Spec 0095 bringt dem Redactor Passwörter bei, die als **Argument** eines
Kommandozeilenprogramms übergeben werden (`mysql -pX`, `sshpass -p X`,
`curl -u u:X`, `htpasswd -b`, `redis-cli -a`, `smbclient -U u%X`,
`openssl -pass pass:X`, `-k X`) sowie weitere Token- und Hash-Formen.

Der Redactor arbeitet mit regulären Ausdrücken auf **freiem Text**. Seine
Eingaben sind nicht Kommandozeilen, sondern alles, was auf dem Weg zu einem
KI-Anbieter, in eine Logzeile, in den Chat oder zu einem MCP-Client liegt:
Kommandoausgaben, Fehlermeldungen (`Kommando '…' konnte nicht ausgeführt
werden`), Chatverlauf, Notiztexte, Diagnose-Exporte. In all diesen Texten
**stecken** Kommandos, sie bilden sie aber nicht ab.

Daraus folgen Entscheidungen, die die Spec offen gelassen hat. Sie stehen
hier, weil sie die Grenze des Schutzes festlegen.

## Entscheidungen

### 1. Regeln, die einen Programmnamen verbrauchen, stehen am Ende der Musterliste

Jede der neuen A1-Regeln verbraucht einen Programmnamen, einen Schalter und
einen Wert, der am nächsten Leerraum endet. Genau diese Bauart hat im
Redactor bereits dreimal einem späteren Muster den mehrteiligen Anker
zerschnitten (`api-key: -----BEGIN …`, zweimal die Query-String-Regel).

Hinter allen bestehenden Mustern angewendet sehen diese den Originaltext
zuerst: **keine bestehende Regel kann durch die neuen etwas verlieren.**
Die umgekehrte Richtung ist möglich und hingenommen — steht
`password=mysql -pGeheim` im Text, verbraucht das Schlüsselwort-Muster
`password=mysql`, und die mysql-Regel findet ihren Anker nicht mehr. Der
Wert bleibt dann im Klartext, aber genauso wie vor dieser Spec: eine Grenze
der Abdeckung, keine Lockerung.

Die Hash- und Token-Muster (Argon2, phpass, Twilio, SendGrid, JWT) stehen
umgekehrt **vorn**, bei den übrigen Hash- und Provider-Key-Mustern: ein
Zufallstreffer eines kürzeren Musters (`AKIA`/`AIza` + Folgezeichen) kann in
einer langen Base64-Nutzlast liegen und sie zerschneiden. Sie können
ihrerseits keinen mehrteiligen Anker zerteilen, weil sie ausschließlich
zusammenhängende Token ohne Leerraum matchen und jeder mehrteilige Anker im
Redactor durch Leerraum getrennt ist.

### 2. Über-Redaktion wird hingenommen, Unter-Redaktion nicht

Ein Programmname wird nicht in „Kommandoposition" verlangt. Folge: Steht er
irgendwo in einer Zeile — auch als Pfadbestandteil — und folgt auf derselben
Kommandostufe ein `-p<wort>`, greift die Regel.
`find /var/lib/mysql -type f -print` verliert dadurch sein `-print`.

Das ist eine Entscheidung, nicht ein Versehen. Die naheliegende Verengung
(„der Programmname muss am Anfang eines Kommandos stehen") würde genau die
Eingaben verfehlen, für die dieser Redactor existiert: `Kommando 'mysql -pX'
konnte nicht ausgeführt werden` und `bitte mysql -pX ausführen` sind reale
Log- und Kontextzeilen dieses Projekts, und dort steht ein echtes Passwort.
Ein regulärer Ausdruck kann `/usr/bin/mysql` (Programm, von Spec 0095 T1
ausdrücklich verlangt) nicht von `/var/lib/mysql` (Argument) unterscheiden.

Von beiden Fehlern ist Über-Redaktion der harmlose: sie kostet Lesbarkeit im
Chat und im KI-Kontext, kein Geheimnis. Die Filter-Engine sieht ohnehin das
Original (Spec 0095 §6), die Entscheidung über ein Kommando bleibt also
unberührt.

Festgehalten in den Tests
`test_redactor_known_over_redaction_when_a_program_name_appears_in_a_path`
und `test_redactor_known_over_redaction_of_a_fourth_htpasswd_argument`,
damit das Verhalten nicht unbemerkt kippt und nicht für einen Fehler
gehalten wird. Der Preis an Lesbarkeit im KI-Kontext ist ein
Qualitätsthema und gehört ins Backlog, nicht in diesen Schritt.

### 3. Ein irreführender Platzhalter ist schlimmer als ein sichtbares Geheimnis

Zwei Review-Runden haben dieselbe Fehlerklasse in fünf Ausprägungen
gefunden: Der Wert wurde nur teilweise erfasst, der Rest blieb direkt neben
einem `[REDACTED]` stehen. Gegenüber dem Stand vor dieser Spec war jede
davon **keine** Verschlechterung — vorher stand der ganze Wert im Klartext.
Trotzdem sind sie alle behoben worden, weil die Zeile danach etwas
behauptet, was nicht stimmt: Wer sie liest, hält das Geheimnis für
geschwärzt.

Behoben wurden: Shell-Escape im Wert (`-pa\;b`), Zeilenfortsetzung zwischen
Schalter und Wert, Zeilenfortsetzung innerhalb des Werts, Unicode-Leerraum
(U+00A0) im Wert, und ein halb erfasster Schalter (`redis-cli --password X`
traf den `--pass`-Präfix und verbrauchte `word` als Wert).

Als Leitlinie für künftige Muster: Eine Regel, die einen Wert nur bis zu
einem Zeichen erfassen kann, das für die Shell **kein** Wortende ist, ist
schlechter als keine Regel.

### 4. Offen gelassen, mit Grund

- **`mysql -p <wert>` / `--password <wert>` mit Leerzeichen** und
  `ssh -p <port>` — dort ist im Text gar kein Passwort erkennbar: ohne
  angehängten Wert fragt der MySQL-Client interaktiv, und der folgende Wert
  ist der Datenbankname. Ein Muster dafür würde `mysql -u root -p mydb`
  schwärzen. Spec 0095 §3, festgehalten in T12.
- **Quotierter Wert über einen Zeilenumbruch** (`-p"Geheim` ⏎ `0095"`) —
  für die Shell ein Wort, für eine Zeichenklasse nicht erreichbar. Falsch
  negativ **ohne** irreführenden Platzhalter, also nicht in der Klasse aus
  Entscheidung 3.
- **`$S$` (Drupal 7 und neuer), `$sha1$`/`$md5$` (Solaris)**,
  Twilio-Auth-Token ohne `SK`-Präfix, `openssl -K <hexkey>`,
  Programmnamen-Varianten (`mysql5`, `mysqlsh`, `mysqlpump`,
  `mariadb-dump`) — von Spec 0095 nicht verlangt, hier offengelegt statt
  stillschweigend fallengelassen.
- **Kein generisches Hoch-Entropie-Fallback.** Jedes neue Muster ist über
  einen eindeutigen Präfix, Schlüsselnamen oder Programmnamen verankert.

### 5. Die Spec-0094-Log-Tests wechseln die Geheimnis-Form, nicht ihre Aussage

Neun Tests in vier Crates belegen, dass ein Kommandotext ab `info` nicht
mehr im Log steht. Sie trugen das Geheimnis bewusst in einer Form, die der
Redactor nicht kannte — sonst wäre ihr Grün auch durch die Redaction
erklärbar. Spec 0095 bringt genau diese Formen bei, also wechseln die Tests
auf `mysql -u root -p <wert>` (Entscheidung 4, erster Punkt). Ihre Aussage
bleibt: gemessen scheitern alle neun weiter gegen einen Stand, der den
Kommandotext wieder auf `info` schreibt.

Die committete Spec 0094 beschreibt in §1.3 und §7 weiter die alten Formen
als „vom Redactor nicht erkannt". Das ist jetzt falsch. Ob eine committete
Spec nachgezogen oder als Stand ihrer Zeit belassen wird, ist keine
Coder-Entscheidung — hier nur vermerkt.

## Folgen

- Der Schutz greift in Log, KI-Kontext, Chat und MCP-Antwort an allen
  Stellen, an denen der Redactor schon vorher lief; neue Aufrufstellen
  entstehen nicht.
- Kommandoausgaben können an Stellen ein `[REDACTED]` tragen, an denen kein
  Geheimnis stand (Entscheidung 2). Filter-Engine und Risiko-Einstufung
  sehen weiter das Original.
- Laufzeit von `redact_text` auf 1 MB ohne einen einzigen Treffer, gemessen
  mit einer Eingabe, die alle neuen Ankerwörter trägt: 7,6 ms → 28,1 ms
  (Release), 269 ms → 584 ms (Debug). Linear, kein Backtracking; die
  `regex`-Crate sucht alle Muster ohne Rückverfolgung.
