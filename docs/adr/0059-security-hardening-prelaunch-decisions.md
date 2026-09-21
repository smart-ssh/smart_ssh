# ADR 0059: Entscheidungen bei der Umsetzung von Spec 0068 (Sicherheitshärtung vor dem Launch)

Status: Angenommen
Bezug: docs/specs/0068-security-hardening-prelaunch.md, Commits `37c55a1`,
`8817a6f`, `46efd7f`, `372c338`, `1ec4955`, `12dc876`, Review-Nacharbeit
`ab1fd08`, `322af3f`, `df7bcc4` und zweite Review-Runde (s. Abschnitt 7)

Grundsatz der Spec: jede Änderung verschärft, keine lockert. Wo zwischen
„sicherer“ und „bequemer“ zu wählen war, ist hier „sicherer“ gewählt und
begründet.

## 1. Redaction (Teil 1)

- **Formate aus Quellen, nicht aus dem Gedächtnis:** gitleaks
  `config/gitleaks.toml` (Anthropic, OpenAI, GitLab PAT/Runner/Deploy,
  Hugging Face), gitleaks-Issue #2158 (Anthropic-OAuth) und #2180
  (Groq/xAI), OpenRouter-, Groq- und xAI-Doku. Präfix exakt, Länge als
  Mindestlänge unterhalb des beobachteten Werts — ein leicht abweichend
  langer echter Key rutscht so nicht durch.
- **Reihenfolge:** die neuen Präfix-Muster laufen direkt nach den
  `$`-Hash-Mustern und vor AWS/Google/Slack, damit diese keinen langen Key
  zerteilen. Muster, die einen Header-/Schlüsselnamen mit verbrauchen
  (`x-api-key`/`api-key`, `Authorization: Basic|Token`, `.netrc`,
  `.pgpass`, Docker `auth`, `client-key-data`), stehen **am Ende** der
  Liste: weiter vorn nahmen sie älteren Mustern den Anker und ließen
  Klartext stehen (zweite Review-Runde). Die generische URL-Regel gibt es
  zweimal: streng (ohne `?`/`#`) direkt nach dem DB-Muster — sonst griff
  sie über einen Port in den Query-String und zerstörte den Anker von
  `password=` — und in der breiten ersten Fassung am Listenende, wo der
  Query-String-Fall schon redigiert ist (unkodiertes `#`/`?` in
  Passwörtern ist in `.env`-Dateien real).
- **Über den Spec-Wortlaut hinaus** (Review-Fund): die Formate genau der
  Dateien, deren Lesen Teil 2 bestätigungspflichtig macht (`.netrc`,
  `.pgpass`, Docker-`auth`, kubeconfig `client-key-data`) — nach einem
  bestätigten Lesen sollen sie trotzdem nicht im Klartext an die KI gehen.
  `.netrc` nur in seiner Struktur (`machine … password …`, `password x`
  als eigene Zeile), damit Fließtext wie „password is required“
  unberührt bleibt.
- Kein generisches Hoch-Entropie-Muster (Spec: verworfen).

## 2. Secret-Pfade (Teil 2)

- **Eigene Prüfung neben dem Risiko-Badge.** Die Spec sagt „bestehenden
  Mechanismus erweitern“. Das Badge (`data_risk`) ist Anzeige, die neue
  Prüfung entscheidet über `Confirm`; sie ist eine eigene Funktion
  (`risk::secret_path_read_reason`), die an keiner Stelle enger sein darf
  als das Badge (`shadow`/`credentials` daher als Wort, wie dort). Eine
  Kopplung „Badge Rot ⇒ Confirm“ wurde verworfen: Rot umfasst auch `env`,
  `printenv`, `mysqldump`, `pg_dump` — das wäre eine eigene
  Produktentscheidung.
- **Nie schwächer als die erste Fassung:** `secret_path_read_reason` ist
  „erste Fassung (wörtlich) ODER erweiterte Prüfung“. Die erste
  Nachbesserung hatte die Prüfung umgebaut und dabei Fälle verloren, die
  die erste Fassung erkannte (zweite Review-Runde); die ODER-Struktur
  schließt das strukturell aus.
- **Quote-bewusste Zerlegung:** Platzhalter zählen nur ungequotet (die
  Shell expandiert `'a.*'` nicht), `$` nur außerhalb einfacher Quotes.
  So bleiben `sed 's/a.*/b/'` und `awk '{print $NF}'` unbehelligt, ohne
  Muster-Argumente überspringen zu müssen.
- **Ort der Eskalation:** in `handle_action_proposed` direkt nach
  `evaluate_action`, **vor** Post-Ingest- und Injection-Prüfung. Die
  Injection-Prüfung verbraucht ihr Flag nur bei `AutoExec`; läge die
  Secret-Prüfung dahinter, verbrauchte eine Secret-Lese-Aktion das Flag
  und die eigentliche Folgeaktion liefe wieder automatisch (zweite
  Review-Runde). Bei gesetztem Flag zeigt der Dialog den Injection-Grund,
  das Flag wird dabei nur gelesen.
- **Bewusst strenger (Komfortkosten, gemeldet):**
  - Rekursives Lesen (`grep -r`, `rg`, `ag`, `ack`) und Lesen per
    `-exec`/`xargs` verlangt **immer** eine Bestätigung — jedes Verzeichnis
    kann eine `.env` oder einen Schlüssel enthalten, der Inhalt ist vorab
    nicht prüfbar. Auch `grep -r error /var/log` fragt also nach.
  - Ein bloßes `*` als letzter Pfadteil (`cat /var/log/*`) fragt nach, weil
    es `.env`/`id_*`/`x.pem` treffen kann; `*.log` nicht.
  - Ein Lesebefehl zählt an **jeder** Stelle eines Teilkommandos (nicht nur
    am Anfang). Damit fragt auch `cp ~/.ssh/id_rsa /backup/ && cat log`
    nach, weil das Gesamtkommando geprüft wird.
  - `.env.example` und `*.pem` (auch Zertifikate) eskalieren.
  - Jede Umleitung auf einen Secret-Pfad eskaliert — auch
    `cat .env > /srv/app/.env`; eine sichere Unterscheidung von
    `> /dev/stdout`, `>&2`, `| tee` wäre selbst eine Umgehungsfläche.
  - `curl`/`wget`/`scp`/`rsync` mit Secret-Pfad eskalieren: sie bringen den
    Inhalt nicht in den Chat, aber vom Server weg.
- **Ausnahmen, bewusst:** `cp`, `install`, `mv`, `stat`, `test -f`, `ls`
  (die Wege, die die Prompt-Regel aus Spec 0066 empfiehlt) — außer mit
  `/dev/stdout`, `/dev/fd/*`, `/dev/pts/*`, `/proc/*/fd/*`, `/dev/tty` als Ziel.
  Öffentliche Schlüssel (`*.pub`), `known_hosts`, `authorized_keys` und
  `~/.ssh/config`.
- **Grenzen (dokumentiert, nicht behebbar lexikalisch):** Symlinks,
  Programme außerhalb der Liste (Skriptsprachen mit Dateinamen, `tar`),
  Kodierungen, Umweg über eine zuvor unverdächtig kopierte Datei.
  Inline-Code (`bash -c`, `python -c`, `$(…)`) setzt die Filter-Engine
  ohnehin auf `Confirm`.

## 3. Sudo-Fallback beim Schreiben (Teil 3)

Ist-Befund: `WriteRemoteFile` war immer `Confirm`, der Dialog erwähnte Sudo
aber nie; nach der Bestätigung schrieb die App bei `SftpPermissionDenied`
mit hinterlegtem Passwort still per Sudo. Die Spec ließ die Wahl zwischen
„vorab ankündigen“ und „erneut bestätigen“. **Entscheidung:** vorab im
Dialog ankündigen (`usesStoredSudoPassword` auch für Schreibaktionen, wenn
ein Passwort hinterlegt ist). Der Fallback läuft nur mit genau dem im
Dialog gesendeten Wert (durchgereicht, keine Neuberechnung —
Review-Fund). Ein zweiter Dialog nach einem Rechte-Fehler wäre ebenso
sicher, aber ein unerwarteter Zusatzschritt mitten in einer schon
bestätigten Aktion.

## 4. Mehrere Tool-Calls in einer Antwort (Teil 4)

Die Tests fanden kein Fehlverhalten. Der Review zeigte aber eine heikle
Stelle: lehnte der Nutzer Aktion 1 ab, lief eine per Allow-Regel
freigegebene Aktion 2 derselben Antwort sofort automatisch — obwohl sie
auf der abgelehnten aufbauen kann. **Entscheidung (Verschärfung):** nach
einer Ablehnung (Nutzer, Regel-`Deny`, abgebrochener Dialog, Timeout,
bearbeitetes und dann blockiertes Kommando)
verlangt jede weitere Aktion derselben Antwort eine Bestätigung
(`FILTER_EARLIER_ACTION_REJECTED_REQUIRES_CONFIRM`). Das Flag gilt pro
KI-Antwort; MCP-Aufrufe (je genau eine Aktion) bekommen ein frisches.

## 5. Timeouts (Teil 5)

- `discovery.rs`: dieselbe Frist wie der Chat-Pfad
  (`SSE_INACTIVITY_TIMEOUT`, 90 s) für Verbindung und Erfolgs-Body,
  `read_error_body_with_timeout` für Fehler-Bodys; keine neue Konstante.
- Host-Key: dieselbe Frist wie das Confirm-Timeout aus Spec 0046 Fund 4
  (`PENDING_ACTION_CONFIRM_TIMEOUT`, 1 h). Timeout = Ablehnung; vertraut
  wird ausschließlich im Zweig einer echten `Trust`-Antwort.
- `ConfirmationRegistry` vergibt pro Registrierung eine Generation;
  `cancel_if_current` entfernt nur den eigenen Eintrag. Heute sind alle
  `SessionId`s frische UUIDs und die Neu-Registrierung beim `connect()`-
  Retry passiert erst nach dem Ende des alten Wartens — die Generation ist
  die strukturelle Absicherung dafür, dass das so bleibt.

## 6. Bewusst NICHT behoben / offen

- **Host-Key-Dialog nach Timeout bei MCP-Verbindungen:** das Frontend
  schließt den Dialog bei einem Fehler des eigenen `connect()`-Aufrufs;
  bei einer MCP-ausgelösten Verbindung bleibt er nach dem Timeout stehen.
  Ein späteres „Vertrauen“ scheitert sichtbar (`resolve` findet nichts),
  vertraut also nichts — sicher, aber verwirrend. Ein
  `host-key-verification-cancelled`-Event ist als Folgeschritt vorgemerkt.
- **Tests für die Body-Timeouts in `discovery.rs`:** wiremock verzögert nur
  die ganze Antwort; getestet ist der Verbindungs-Timeout. Die Body-Pfade
  nutzen denselben Mechanismus.
- **`reqwest::Client::new()` statt `build_http_client()` in Discovery**
  (TCP-Keepalive): kein Sicherheitsthema, die Frist greift ohnehin.
- **Timeout-Meldung ohne Fehlercode** (nur deutsch): wie der bestehende
  Reject-Zweig; i18n der Verbindungsfehler ist ein eigenes Thema.
- **Sudo-Passwort nach Verbindungsaufbau gelöscht:** die offene Session
  behält ihre Kopie bis zur Trennung (vorbestehend, Spec 0018). Der Dialog
  kündigt den Fallback dann weiter an — nicht still, aber vielleicht nicht
  erwartet. Vorgemerkt.
- **Sudo-Hinweis auch auf `Deny`-Karten** (Frontend, kosmetisch).
- **Weitere Redaction-Formate** außerhalb von Spec und Review (z. B.
  DeepSeek `sk-<32 hex>` ohne eindeutiges Präfix) — bewusst nicht
  geraten.
- **Grenzen der Secret-Prüfung über Aktionen hinweg:** ein
  Arbeitsverzeichnis aus einer früheren Aktion ist unbekannt (`cd ~/.kube`
  in Aktion 1, `cat config` in Aktion 2). Jede Aktion läuft ohnehin in
  einer frischen Shell im Home-Verzeichnis; ein `cd` wirkt nur innerhalb
  derselben Aktion, und dort wird es ausgewertet.
- **`EditThenApprove` mit selbst ergänztem `sudo`:** der Nutzer tippt
  `sudo` selbst; das hinterlegte Passwort wird dann eingespeist, obwohl die
  ursprüngliche Karte keinen Sudo-Hinweis trug (vorbestehend, Spec 0018).
  Der Nutzer hat `sudo` bewusst geschrieben — kein stiller Einsatz.

## 7. Zweite Review-Runde

Der Review der Nachbesserungen fand, dass diese selbst lockerten
(Grundsatz verletzt): der umgebaute Klassifizierer ließ Fälle durch, die
die erste Fassung erkannte (`grep '' ~/.ssh/id*`,
`head …/credentials</../dev/null`, `cat /etc/shadow_old`,
`cat${IFS}/root/.ssh/id_rsa`); neue Redaction-Muster ließen Klartext
stehen, den ältere Muster vorher redigierten
(`Authorization: Token token="…"`, `api-key: Bearer …`,
`password = …`, `smtp://u:S3cr#t@h`); und die verschobene Secret-Prüfung
verbrauchte das Injection-Flag. Behoben strukturell (ODER mit der ersten
Fassung, Anker-Muster ans Listenende, Reihenfolge zurück), jeweils mit
Regressionstests, die gegen den Stand der Nachbesserung rot sind.

## 8. Dritte Review-Runde

Die dritte Runde bestätigte: Orchestrierung und Klassifizierer lockern
gegenüber der ersten Fassung nichts. Gefunden und behoben wurde Folgendes:

- **Redactor:** Ein Teiltreffer mitten in einem längeren Wert (zufälliges
  `AKIA…` in Base64, Provider-Key in einem Bearer-Token) ließ einen Rest
  stehen, weil spätere Muster kein `[` kennen. Behoben durch einen letzten
  Durchgang: Jeder Platzhalter schluckt die direkt angrenzenden
  Token-Zeichen. Er erzeugt nie eine Redaction ohne vorhandenen
  Platzhalter. Die Header-Muster der ersten Fassung laufen zusätzlich
  wieder an ihrer ursprünglichen Stelle.
- **Klassifizierer:** Gegenüber der ersten Nachbesserung eskalierten
  gequotete Platzhalter in einer weiteren Shell nicht mehr (`watch '…'`,
  `ssh h cat '…'`, `su -c`, `eval`, `kubectl exec … sh -c`). Behoben:
  - Kommt ein Programm vor, das an eine weitere Shell weitergibt, zählen
    auch gequotete Platzhalter.
  - Mehrwortige Code-Strings werden rekursiv geprüft. Die Rekursion ist in
    Tiefe und Anzahl begrenzt; beim Überschreiten wird eskaliert.
- **Weitere Lücken geschlossen:**
  - kombinierte `-hd recurse`
  - mehrstufige `cd`-Ketten
  - `tar`, `getent`, Skriptsprachen mit Dateinamen, `parallel`, `fd -x`
  - `/proc/*/environ`, Shell-Historien, `/etc/kubernetes/*.conf`,
    `wp-config.php`

**Bewusst offen:**

- `perl -ne`/`-pe` erkennt die Filter-Engine nicht als Inline-Code, weil
  `SCRIPT_INTERPRETER_CODE_FLAGS` nur das exakte `-e` kennt. Das liegt
  außerhalb dieser Spec und ist vorgemerkt. Mit einem Secret-Pfad
  eskaliert die neue Prüfung trotzdem, weil `perl` jetzt als Leser zählt.
- Ist das Injection-Flag gesetzt, trägt eine Secret-Lese-Aktion den
  Injection-Code. Das Frontend übersetzt nach Code, der Secret-Grund steht
  dann nur im Text. Gewählt wurde bewusst der alarmierendere Hinweis.
- Kleine Redaction-Falsch-Positive, zugunsten der Sicherheit in Kauf
  genommen:
  - `api-key: see docs` wird zu `api-key: [REDACTED] docs`.
  - Eine Zeile, die nur `password <wort>` enthält, wird redigiert.
