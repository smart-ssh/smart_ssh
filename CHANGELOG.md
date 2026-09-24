# Changelog

Alle nennenswerten Änderungen an Smart SSH werden hier dokumentiert. Das
Format folgt [Keep a Changelog](https://keepachangelog.com/de/1.1.0/),
dieses Projekt hält sich an [Semantic Versioning](https://semver.org/lang/de/).

Funktionen, die nur in einer kostenpflichtigen Edition verfügbar sind,
sind mit **(Pro)** markiert.

## [Unreleased]

## [0.5.1] — 2026-09-25

### Added
- Dateibrowser mit erhöhten Rechten: Ein Umschalter „Erhöhte Rechte“ öffnet
  den Dateibrowser als root (oder einen anderen Nutzer), indem der
  SFTP-Server per `sudo -n` gestartet wird — wie bei WinSCP. Voraussetzung
  ist eine sudo-Regel ohne Passwort für `sftp-server` auf dem Server; die
  App fragt nie nach einem sudo-Passwort. Fehlt die Regel, zeigt die App
  die passende sudoers-Zeile zum Kopieren samt Hinweis, was sie bedeutet.
  Der Modus ist nie standardmäßig an, wird nicht gespeichert, ist durch
  Rahmen und Banner unübersehbar, und Bestätigungen nennen ihn („…als root
  löschen?“). KI und MCP-Clients haben auf diesen Kanal keinen Zugriff.
  Den Pfad zu `sftp-server` erkennt die App automatisch; im Server-Profil
  lässt er sich unter „Erweitert“ überschreiben.
- Jede Dateibrowser-Aktion meldet jetzt ihr Ergebnis: Erfolg kurz
  eingeblendet („„datei.conf“ heruntergeladen nach ~/Downloads“, mit „Im
  Finder zeigen“), Fehler bleiben mit Grund stehen. Ordner-, rekursive und
  Mehrfach-Aktionen fassen das Ergebnis in einer Meldung mit Anzahl
  zusammen.
- Der Über-Dialog zeigt jetzt auch, ob ein Dev- oder Release-Build läuft
  (z. B. „0.5.0 (d887019) · Community · Dev-Build“); die Titelzeile
  markiert Dev-Builds mit „· Dev“. Beide nutzen getrennte
  Datenverzeichnisse — so ist sofort klar, welche Daten man gerade sieht.
  „Kopieren“ übernimmt die ganze Zeile für Fehlerberichte.
- Der Chat ist jetzt jederzeit bedienbar, auch während die KI arbeitet:
  Eine währenddessen geschriebene Nachricht unterbricht nichts, sondern
  wird eingereiht und mit der nächsten Anfrage an die KI mitgeschickt
  (bzw. direkt danach, falls keine weitere Runde folgt) — gut zum
  Korrigieren, ohne auf das Ende der Antwort warten zu müssen. Der
  „Stopp"-Knopf erscheint bei jeder laufenden Antwort und bricht die
  laufende Anfrage an den KI-Anbieter sofort ab, statt nur weitere
  automatische Runden zu verhindern. Ein gerade laufendes Kommando auf dem
  Server läuft dabei zu Ende; ein offener Bestätigungsdialog bleibt stehen.
- Die KI wird jetzt angewiesen, Passwörter, private Schlüssel, Tokens und
  ähnliche Geheimnisse möglichst nicht zu lesen: Existenz wird über
  Metadaten (z. B. Dateigröße) geprüft, und solche Dateien werden direkt
  auf dem Server kopiert (`cp`, Pipe) statt gelesen und neu geschrieben —
  so landen Geheimnisse gar nicht erst im Chat-Verlauf.
- App nutzt jetzt Anthropics Prompt-Caching für System-Prompt,
  Werkzeug-Definitionen und Server-Notiz — diese ändern sich innerhalb
  einer Sitzung kaum, werden bislang aber bei **jedem** Request neu
  gesendet und voll gegen das Input-Token-Rate-Limit gezählt. Gecachte
  Tokens zählen bei den meisten Claude-Modellen **nicht** gegen das
  Rate-Limit, senkt also Kosten und **Rate-Limit-Last** spürbar (Anthropic
  nennt als Beispiel 80% Cache-Trefferquote = effektiv 5× mehr
  Input-Durchsatz pro Minute). Der bisher direkt im System-Prompt
  eingebettete `uname`-Systembanner des verbundenen Servers wandert dabei
  in eine eigene, wie Kommando-Ausgaben gekennzeichnete Nachricht (hätte
  sonst bei jeder Sitzung den Cache ungültig machen können).
- Neuer "Diagnosepaket erzeugen"-Knopf in den Einstellungen (Diagnose)
  neben "Logpfad öffnen": erzeugt ein einzelnes, redigiertes Text-Paket
  (Version + Build-Hash, Betriebssystem/Architektur, Datenpfade,
  konfigurierte KI-Provider-**Typen**, Server-**Anzahl**, die letzten bis
  zu 500 Log-Zeilen) zur Weitergabe an den Support oder zum Anhängen an
  ein GitHub-Issue. Enthält **keine** Zugangsdaten, Passwörter, Host-Keys,
  Server-Adressen oder Notiz-/Chat-Inhalte; eingebettete Log-Zeilen laufen
  zusätzlich durch dieselbe Redaction wie sonst in der App. Wird vor dem
  Speichern erst als Vorschau angezeigt — nichts wird automatisch
  gespeichert oder versendet.
- App liest jetzt die Rate-Limit-Header des KI-Providers (Anthropic:
  `anthropic-ratelimit-{requests,input-tokens,output-tokens,tokens}-
  {remaining,reset}`) und drosselt proaktiv, statt blind ins Limit zu
  laufen: sinkt das bekannte Restbudget unter ~15% (oder würde eine
  geschätzte Anfrage das verbleibende Token-Budget überschreiten), wartet
  die App bis zum bekannten Reset-Zeitpunkt, bevor sie die Anfrage
  abschickt — der Chat zeigt dabei "Warte auf KI-Budget — nächster
  Versuch in Xs…" an, der Versand erfolgt danach automatisch. Alle
  KI-Aufrufe (Haupt-Chat, optionale Zweitmeinung/Einschleusungs-Check,
  Zusammenfassung, Auto-Titel, Notiz-Vorschlag/-Kürzung), die denselben
  API-Key/Endpunkt/Modell nutzen, teilen sich dabei ein gemeinsames
  Budget. Provider ohne diese Header (z. B. ein lokaler Ollama-Endpunkt)
  bleiben unverändert — kein proaktives Warten, nur das bestehende
  reaktive Wiederholen nach einem 429.
- App zeigt jetzt bei Startfehlern eine verständliche Meldung statt
  stillem Absturz: kann die Datenbank nicht geöffnet werden (z. B. von
  einer neueren Programmversion angelegt, beschädigt, oder das
  Datenverzeichnis nicht beschreibbar) oder der Host-Key-Speicher nicht
  geladen werden, erscheint jetzt ein Dialog mit der Fehlerursache statt
  eines unerklärten Absturzes vor dem ersten Fenster. Ist der
  Systemschlüsselbund beim Start gesperrt oder nicht verfügbar, startet
  die App wie bisher trotzdem (Chat-Verlauf und Notiz-Zusammenfassungen
  sind dann für diesen Programmlauf deaktiviert), zeigt das aber jetzt
  ebenfalls sichtbar an statt es nur stillschweigend zu protokollieren.
- Ist die gespeicherte Notiz eines Servers sehr groß, bietet die App beim
  Trennen der Verbindung jetzt an, sie per KI zusammenzufassen (oder
  selbst zu kürzen) — wie bei jedem KI-Notiz-Vorschlag wird die
  Zusammenfassung erst nach Bestätigung im Diff-Vergleich (alt/neu)
  tatsächlich gespeichert, nie automatisch. Dieser Vorschlag erscheint
  jetzt auch für den lokalen Pseudo-Server (Localhost).
- Der Notiz-Editor zeigt jetzt beim Bearbeiten einer sehr großen Notiz
  einen dezenten Hinweis mit direktem Link, sie sofort per KI
  zusammenfassen zu lassen (derselbe Bestätigungs-Ablauf wie beim
  Trennen-Vorschlag).

- Die maximale Antwortlänge der KI ist jetzt modellabhängig statt fest
  auf ~4000 Tokens begrenzt (bei aktuellen Claude-Modellen bis zu 128K)
  — ein zu knappes Limit führte bislang gelegentlich dazu, dass die
  Antwort mitten im Satz/Kommando abbrach. Bricht eine Antwort trotzdem
  am Längenlimit ab, bleibt der bisherige Text sichtbar und ein
  "Weiter"-Knopf setzt die Antwort exakt dort fort, wo sie endete.
  Zusätzlich im KI-Provider-Formular unter "Erweitert" ein optionales
  Feld "Max. Antwortlänge (Tokens)" für Provider mit unbekanntem
  Output-Maximum (z. B. ein selbstgehostetes Modell) — Standard weiterhin
  "Automatisch".
- Öffnest du die KI-Provider-Einstellungen ohne konfigurierten
  Ollama-Provider, sucht die App einmalig ein lokal laufendes Ollama
  (`127.0.0.1:11434`) und bietet an, es zu übernehmen — inklusive
  Modellauswahl. Ohne gefundenes Ollama zeigt eine kompakte Anleitung, wie
  man es installiert. Kein Netzwerkaufruf ohne diese Aktion oder einen
  Klick auf „Erneut suchen".
- Ist noch kein eigener Server angelegt, führt die Serverliste jetzt aktiv
  mit „Ersten Server anlegen" in den Anlage-Dialog, statt nur einen
  Entwicklertext zu zeigen.
- Beim Anlegen eines Servers mit Schlüssel-Anmeldung empfiehlt ein Hinweis
  Ed25519-Schlüssel (`ssh-keygen -t ed25519`) — RSA bleibt unterstützt.
- Bei Ollama ist der API-Key im Formular nicht mehr erforderlich.
- Die Diagnose-Ansicht zeigt eine Zeile „Systemschlüsselbund: verfügbar /
  nicht verfügbar (Grund)" samt nächstem Schritt — nachschlagbar auch dann,
  wenn der Startdialog bereits weggeklickt wurde.
- Der Button „Modelle laden" ist jetzt auch für Anthropic-Provider
  verfügbar — bisher fehlte er dort ganz.
- Beim Anlegen und Bearbeiten eines Servers steht neben Passwort, Private
  Key, SSH-Agent und Zertifikat jetzt eine fünfte Anmeldeart zur Wahl:
  „Schlüsseldatei“ — ein Pfad statt eines im Schlüsselbund gespeicherten
  Inhalts, wie `IdentityFile` in `ssh_config`. Vor dem Speichern zeigt die
  Oberfläche, ob die Datei existiert, ob die Rechte passen, ob sie wie ein
  OpenSSH-Schlüssel aussieht und ob sie verschlüsselt ist. Server-Liste und
  -Details zeigen den Pfad, an dem die Anmeldung hängt.
- Ein Knopf „In den Schlüsselbund übernehmen“ legt den Inhalt einer solchen
  Schlüsseldatei einmalig in den Systemschlüsselbund und stellt die
  Anmeldeart auf „Private Key“ um — die Ursprungsdatei bleibt dabei
  unverändert liegen. Vorher zeigt ein Dialog, welche Datei gelesen wird
  und was sich ändert.

### Changed
- Smart SSH ist jetzt Open Source unter der **Apache License 2.0**
  (bisher Functional Source License 1.1, FSL-1.1-MIT). Das gilt
  rückwirkend: Auch alle bereits veröffentlichten Versionen dürfen unter
  der Apache License 2.0 genutzt werden.
- Der System-Prompt weist die KI jetzt ausdrücklich an, ein angekündigtes
  Kommando auch tatsächlich über `suggest_command` vorzuschlagen, statt es
  nur im Fließtext anzukündigen und dort aufzuhören — beobachtetes Muster,
  bei dem die automatische Fortsetzung dadurch ohne erkennbaren Grund
  stehen blieb. Kurze Erklärungen vor einem Kommando bleiben ausdrücklich
  erwünscht.
- Fehlermeldungen auf dem Weg „Provider einrichten → Server anlegen →
  Verbindung testen → verbinden" nennen jetzt durchgängig Ursache **und**
  nächsten Schritt, auf Deutsch und Englisch — u. a. für falschen API-Key,
  unbekanntes Modell, nicht erreichbaren lokalen KI-Dienst, abgelehnte
  SSH-Verbindung, unbekannten Host, nicht erreichbaren Host und
  abgelehnten/nicht rechtzeitig bestätigten Host-Key.
- Der SSH-Verbindungsaufbau bricht jetzt nach 10 Sekunden ohne Antwort mit
  einer klaren Meldung ab, statt minutenlang auf das Betriebssystem zu
  warten.
- Die Startdialoge (Datenbank-, Host-Key- und Schlüsselbund-Fehler) sprechen
  jetzt Deutsch oder Englisch, abhängig von `LC_ALL`/`LC_MESSAGES`/`LANG`.
  Vorgabe bleibt Deutsch, wenn keine brauchbare Spracheinstellung gesetzt
  ist.
- Ein vertippter Modellname zeigt bei Anthropic jetzt korrekt „Modell nicht
  gefunden" statt „Provider nicht erreichbar".
- Der Platzhaltertext in der Verwalten-Ansicht ohne ausgewählte
  Gruppe/Server ist jetzt auch auf Englisch übersetzt (erschien zuvor auch
  in der englischen Oberfläche auf Deutsch).
- API-Key, Server-Passwort, Passphrase und Sudo-Passwort werden an den
  Rändern nach derselben Regel bereinigt. Nur die Ränder: Was innerhalb
  eines Wertes steht, bleibt unangetastet — auch ein ungewöhnliches
  Zeichen, das dort hingehören könnte.
- Beim selbst gesetzten Pfad zum `sftp-server` (erhöhter Dateibrowser)
  gilt dieselbe Regel: Ein unsichtbares Zeichen am Rand wird entfernt,
  statt den Pfad als ungültig abzulehnen. Ein solches Zeichen
  **innerhalb** des Pfades führt unverändert zur Ablehnung.
- Als Folge davon kann die Prüfung auf eingeschleuste Anweisungen jetzt
  häufiger anschlagen — etwa wenn das Modell in seiner Begründung eine
  gewöhnliche Konfigurationszeile wie `PermitRootLogin yes` zitiert. Die
  nächste Aktion verlangt dann eine Bestätigung, statt automatisch zu
  laufen. Das ist die sichere Richtung: Eine Nachfrage zu viel ist sichtbar
  und mit einem Klick erledigt, eine verschluckte Warnung nicht.
- Eine bereits gespeicherte Regel mit einem solchen Muster — etwa aus einer
  älteren Programmfassung — wird in der Regelliste sichtbar markiert, mit
  dem Hinweis, dass das Muster ungültig ist, und der Fundstelle darin.
  Bearbeiten und Löschen bleiben möglich; Speichern verlangt dann ein
  gültiges Muster. Zusätzlich wird eine solche Regel bei jeder Auswertung
  im Protokoll vermerkt (mit Regel-Kennung, nie mit dem Kommando).
- Meldet das Regel-Formular ein ungültiges Muster, steht dort jetzt ein
  verständlicher Satz in der eingestellten Sprache und darunter die genaue
  Fundstelle im Muster — bisher nur der englische Text der zugrunde
  liegenden Bibliothek.
- An einer so markierten Regel lässt sich die Priorität nicht mehr über die
  Pfeiltasten verschieben; die Pfeile sind deaktiviert und nennen den Grund.
- Der Vorschlag, eine sehr große Notiz per KI zusammenzufassen, hat jetzt
  einen ✕-Knopf zum Schließen sowie einen Knopf „Später“, nach dem er für
  diesen Server bis zum nächsten Start der App nicht wieder erscheint.
- Dieser Vorschlag sowie der Hinweis beim Bearbeiten einer großen Notiz im
  Notiz-Editor erscheinen jetzt erst ab 10 000 Zeichen statt bisher 8 000
  Byte.
- KI-Antworten ohne Text verschwinden nicht mehr still. Bricht die
  Antwort eines Reasoning-Modells wegen des Ausgabe-Limits leer ab, wird
  sie einmal automatisch mit mehr Budget wiederholt; bleibt eine Antwort
  auf eine Nachricht trotzdem ganz ohne Text und ohne Vorschlag, zeigt der
  Chat jetzt einen Hinweis mit dem Tipp, das Ausgabe-Limit in den
  Provider-Einstellungen zu erhöhen.

### Fixed
- "Mache ich selbst" im Kürzungs-Vorschlag scrollt jetzt direkt zum
  Notizfeld und fokussiert es, statt nur das Server-Formular zu öffnen.
- Ein KI-Provider (oder ein zwischengeschalteter Proxy), der einen
  Fehlerstatus zwar sofort beantwortete, den Antworttext danach aber
  hängen ließ, konnte einen Chat-Turn unbegrenzt und ohne jede
  Fehlermeldung blockieren (Chat antwortet nicht mehr, kein Log-Eintrag).
  Der Aufruf bricht jetzt spätestens nach 90 Sekunden mit einer
  sichtbaren Fehlermeldung ab.
- Eine sehr lange Chat-Sitzung oder eine sehr große Server-Notiz konnte
  eine KI-Anfrage so groß werden lassen, dass sie das Kontextfenster des
  Modells sprengte — der Chat blieb dann ohne Antwort/Fehlermeldung
  hängen. Die App schätzt jetzt vor jeder Anfrage die tatsächliche Größe
  und kürzt bei Bedarf automatisch: ältere Gesprächsrunden werden durch
  eine von der KI erstellte, laufend aktualisierte Zusammenfassung
  ersetzt (schlägt dieser Zusammenfassungs-Aufruf fehl, schneidet die App
  stattdessen wie gehabt ab — der Chat bleibt in jedem Fall funktionsfähig),
  danach ggf. überlange einzelne Kommando-Ausgaben und zuletzt die Notiz
  gekürzt — die gespeicherte Notiz und der vollständige Sitzungsverlauf
  bleiben dabei unangetastet.
- Ein KI-Provider-Aufruf konnte nach einem kurzen Netzwerk-Aussetzer ohne
  jede Fehlermeldung unbegrenzt hängen bleiben (der Chat antwortete dann
  einfach nicht mehr). Die Verbindung zum KI-Provider nutzt jetzt aktives
  TCP-Keepalive, damit eine durch den Aussetzer "leise gestorbene"
  Verbindung schneller erkannt wird.
- Linux ohne Systemschlüsselbund: Statt des englischen Bibliothekstexts
  „No default store has been set, so cannot search or create entries" nennt
  Smart SSH jetzt den tatsächlichen Zustand, die dadurch blockierten
  Funktionen und das Paket, das ihn behebt — je nachdem, ob kein
  Secret-Service-Anbieter läuft (`sudo apt install gnome-keyring`, auch
  unter KDE), kein D-Bus-Session-Bus erreichbar ist
  (`sudo apt install dbus-user-session`) oder der Schlüsselbund nur
  gesperrt ist. Ein gesperrter Schlüsselbund führt dabei nie zu einem
  Installationsvorschlag. Wer KWallet oder KeePassXC ohnehin nutzt, wird
  darauf hingewiesen, dass deren Secret-Service-Integration laufen bzw.
  eingeschaltet sein muss — Nachinstallieren allein genügt dort nicht.
- Der Startdialog behauptete bisher, ohne Schlüsselbund funktioniere alles
  außer dem Chat-Verlauf normal. Das stimmte nicht: Ohne Schlüsselbund
  lassen sich weder KI-Provider noch Server-Passwörter, Passphrasen oder
  Sudo-Passwörter speichern oder lesen. Der Dialog zählt jetzt auf, was
  wirklich blockiert ist — und was weiterhin geht (SSH-Agent, Schlüssel
  ohne Passphrase).
- Das Server-Formular meldete „kein Sudo-Passwort hinterlegt", wenn der
  Schlüsselbund gar nicht antworten konnte. Es zeigt jetzt einen neutralen
  Zustand, statt etwas zu behaupten, das es nicht wissen kann.
- „Hinterlegtes Sudo-Passwort entfernen" meldete Erfolg, auch wenn der
  Schlüsselbund den Eintrag gar nicht löschen konnte — das Passwort wäre
  beim nächsten `sudo` weiter eingespeist worden. Der Vorgang schlägt jetzt
  sichtbar fehl.
- Einen Server zu löschen funktioniert weiterhin auch dann, wenn der
  Schlüsselbund klemmt. Neu ist: Smart SSH sagt danach ausdrücklich, welche
  Einträge im Schlüsselbund zurückgeblieben sind, dass sie dort jetzt
  verwaist sind und wie sie sich von Hand entfernen lassen.
- „Verbindung testen" bewertet leere und eingefügte Zugangsdaten jetzt
  genau wie „Speichern". Legt man einen neuen Server an und lässt das
  Passwort-, Schlüssel- oder Zertifikatsfeld leer, meldet der Test die
  Fehlermeldung, statt sich mit einem leeren Wert anzumelden — auf einem
  Server, der leere Passwörter erlaubt, konnte er dafür bisher Erfolg
  melden, obwohl sich derselbe Server anschließend nicht speichern ließ.
  Beim Bearbeiten bedeutet ein leeres Feld weiterhin „das hinterlegte
  Zugangsdatum verwenden".
  Die Meldung ist dieselbe (und in der eingestellten Sprache) wie beim
  Speichern, etwa „Passwort ist erforderlich".
- „Verbindung testen" behandelt die Passphrase einer Schlüsseldatei oder
  eines privaten Schlüssels jetzt genauso wie „Speichern". Bisher konnte
  derselbe eingefügte Wert den Verbindungstest scheitern lassen und
  danach trotzdem richtig gespeichert werden — der Test sagte damit
  etwas anderes aus als der Server, der daraus entstand.
- Ein eingefügter API-Key oder ein eingefügtes Passwort funktioniert jetzt
  auch dann, wenn beim Kopieren ein unsichtbares Zeichen an den Rand
  geraten ist — etwa ein BOM aus einer Textdatei oder ein Zero-Width-Space
  aus einer Webseite. Bisher wurde ein solcher Wert unverändert
  gespeichert, die Anmeldung schlug fehl, und der Key sah in der
  Oberfläche trotzdem richtig aus.
- Eine Filterregel, deren Muster sich nicht übersetzen lässt, wird beim
  Anlegen und beim Ändern jetzt abgewiesen, statt gespeichert zu werden und
  anschließend wirkungslos in der Liste zu stehen. Das galt bisher für jede
  Aktion — auch für eine Deny- oder Bestätigen-Regel, die dadurch stillschweigend
  nichts tat. Das gilt für das Regel-Formular und für die Schnellregel aus
  dem Bestätigungsdialog.
- Der Bestätigungsdialog schlägt keine Schnellregel mehr vor, deren Muster
  sich nicht übersetzen lässt. Bisher entstand ein solcher Vorschlag aus
  einem Kommando mit einer Klammer im Argument und ließ sich anwählen,
  führte aber zu keiner wirksamen Regel.
- Zugangsdaten in einer Verbindungs-URL werden jetzt auch dann vollständig
  geschwärzt, wenn Passwort oder Benutzername ein unkodiertes `@` enthalten
  (etwa `postgres://app:Xy9@kLm2@db/prod` oder die bei mehreren gehosteten
  Datenbanken vorgeschriebene Schreibweise `benutzer@mandant`). Das gilt für
  alles, was an das KI-Modell geht, und für alles, was in der
  Gesprächshistorie gespeichert wird.
- Ein Passwort-Parameter im Query-String einer Verbindungs-URL ohne Pfad
  (`redis://cache:6379?password=…`) wird ebenfalls vollständig geschwärzt.
- „Zugangsdaten testen“ für einen Anthropic-Provider schlug fälschlich mit
  „Provider nicht erreichbar“ fehl, obwohl der Schlüssel gültig war — die
  Probe schickte keinen System-Prompt, und Anthropic lehnt einen leeren
  System-Textblock mit Caching-Markierung ab.

### Security
- Glob-`*` in pfadförmigen Allow-/Deny-Regeln (Muster mit einem
  Dateipfad-artigen Argument, z. B. `Allow: cat /var/log/*`) überquert
  keine Verzeichnisgrenzen mehr, auch nicht über Shell-Quoting/-Escaping
  (`\..`, `".."`, `'..'`, Brace-/Bracket-Tricks) versteckt. Bisher konnte
  eine harmlos erteilte Allow-Regel wie `cat /var/log/*` auch
  `cat /var/log/../../../etc/shadow` automatisch erlauben, weil der
  Glob-`*` über `/`-Grenzen hinwegging — die Filter-Engine "erlaubte"
  damit etwas, das nie freigegeben werden sollte. **Verhaltensänderung**:
  eine bestehende pfadförmige Regel mit einem einzelnen `*` deckt jetzt
  nur noch eine Verzeichnisebene ab (z. B. matcht `/etc/*` weiterhin
  `/etc/passwd`, aber nicht mehr `/etc/nginx/nginx.conf`). Für
  **Deny/Confirm**-Regeln mit mehrstufigem Schutzbedarf `**` verwenden
  (`/etc/**`) — bestehende Deny-Regeln verlieren dabei ohnehin nie
  Schutzumfang (altes und neues Matching gelten zusätzlich). Für
  **Allow**-Regeln wird `**` NICHT empfohlen: `**` kann in einer
  Allow-Regel auch über das eigentlich gemeinte Argument hinweg
  zusätzliche, unbeabsichtigte Kommando-Teile mit erlauben — für
  mehrstufigen Allow-Bedarf stattdessen mehrere spezifische,
  einstufige Allow-Regeln anlegen. Kommando-Argument-Globs ohne
  Pfad-Charakter (z. B. über eine URL) sind unverändert.
- Ein von der KI vorgeschlagenes Kommando, dessen Antwort mitten im
  Vorschlag durch das Längenlimit abgeschnitten wurde, konnte in einem
  seltenen Fall trotzdem — mit zufällig vollständig aussehendem, aber
  in Wahrheit gekürztem Inhalt — bis zum Bestätigungsdialog durchkommen.
  Ein solcher abgeschnittener Vorschlag wird jetzt nie mehr angezeigt
  oder ausgeführt; die App versucht stattdessen einmalig automatisch
  erneut mit mehr Platz, bevor sie im Ausnahmefall einen sichtbaren
  Fehler statt eines unvollständigen Kommandos zeigt.
- API-Schlüssel von KI-Anbietern und Code-Hostern werden jetzt auch ohne
  erkennbares Stichwort davor geschwärzt, bevor Text an die KI geht oder
  gespeichert wird: Anthropic (`sk-ant-…`), OpenAI (`sk-proj-…` u. a.),
  OpenRouter (`sk-or-v1-…`), GitLab (`glpat-…`, `glrt-…`, `gldt-…`),
  Hugging Face (`hf_…`), Groq (`gsk_…`) und xAI (`xai-…`). Ebenso Header
  wie `x-api-key`, `api-key`, `x-goog-api-key`, `Authorization: Basic …`
  und `Authorization: Token …`, Passwörter in beliebigen Adressen der Form
  `schema://nutzer:passwort@host` (der Nutzername bleibt lesbar) sowie die
  Zugangsdaten aus `.netrc`, `.pgpass`, Docker- und kubeconfig-Dateien.
- Liest ein KI-Vorschlag eine typische Geheimnis-Datei (private SSH- und
  Host-Schlüssel, `*.pem`, `*.key`, `*.p12`/`*.pfx`/`*.jks`, `.env`/`.envrc`,
  `/etc/shadow`, `~/.aws/credentials`, `~/.docker/config.json`,
  `~/.kube/config`, `.netrc`, `.pgpass`, `.git-credentials`, `~/.gnupg`,
  `/etc/ssl/private`, `~/.npmrc`, `~/.pypirc`, `~/.my.cnf`, Shell-Historien,
  Prozess-Umgebungen, `wp-config.php`, Kubernetes-Admin-Konfigurationen
  u. a.), fragt die
  App jetzt immer nach — auch wenn eine Allow-Regel das Kommando sonst
  automatisch freigeben würde. Das gilt auch für Anfragen externer
  MCP-Clients und für das direkte Lesen einer Datei, und es lässt sich
  nicht über Pfad-Tricks, Verzeichniswechsel, Platzhalter oder
  vorangestellte Befehle umgehen. **Verhaltensänderung:** rekursives
  Durchsuchen (`grep -r`, `rg`) und Lesen per `find -exec`/`xargs` fragt
  ebenfalls immer nach, weil der Inhalt vorab nicht prüfbar ist.
- Schlägt die KI einen Aufruf von `sftp-server` vor (etwa per `sudo -n`),
  zeigt die Risikoeinschätzung jetzt Rot und die App fragt immer nach —
  auch wenn eine Allow-Regel das Kommando sonst freigeben würde: Mit der
  sudo-Regel für den erhöhten Dateibrowser bedeutet das Dateizugriff mit
  Root-Rechten.
- Lehnt man eine von mehreren Aktionen einer KI-Antwort ab (oder blockiert
  eine Regel sie), laufen die übrigen Aktionen derselben Antwort nicht mehr
  automatisch, sondern fragen ebenfalls nach — sie könnten auf der
  abgelehnten aufbauen.
- Der Bestätigungsdialog für das Schreiben einer Datei sagt jetzt vorab,
  wenn die App bei fehlenden Rechten mit dem hinterlegten Sudo-Passwort
  schreiben würde. Bisher war das erst im Ergebnis zu sehen.
- Ein Host-Key-Dialog, der nie beantwortet wird (z. B. weil das Fenster
  neu geladen wurde), lässt den Verbindungsaufbau nicht mehr ewig warten.
  Nach einer Stunde bricht die App ab und gilt die Abfrage als abgelehnt;
  ein Host-Key wird durch Zeitablauf nie vertraut. Auch „Modelle laden“
  und die Attestierungsabfrage hängen bei einem nicht antwortenden
  Anbieter nicht mehr, sondern melden nach 90 Sekunden einen Fehler.
- Die KI-Zweitmeinung zum Datenrisiko und die Prüfung auf eingeschleuste
  Anweisungen lesen ihr Urteil jetzt zuverlässig aus der Antwort: Enthält
  eine Antwort mehrere Urteilswörter, zählt das warnende. Bisher zählte das
  zuerst genannte — eine Antwort, die den geprüften Text zitierte und darin
  ein „nein"/„none" enthielt, konnte so eine danach ausgesprochene Warnung
  verschlucken. Bei der Prüfung auf eingeschleuste Anweisungen ließ sich das
  gezielt ausnutzen, weil der geprüfte Text dort aus einer nicht
  vertrauenswürdigen Quelle stammt. Eine Warnung kann jetzt nicht mehr durch
  die Wortstellung verlorengehen.

## [0.5.0] — 2026-09-11

### Added
- Chat-Eingabefeld erlaubt jetzt mehrzeilige Eingaben: Enter sendet,
  Shift+Enter fügt einen Zeilenumbruch ein, das Feld wächst mit dem Inhalt
  bis zu einer Maximalhöhe und scrollt danach intern.
- Chat: "In Notiz übernehmen"/"Als Markdown exportieren" sind bei jeder
  Antwort weiterhin vorhanden (auch per Tastatur erreichbar), blenden aber
  erst bei Hover/Tastaturfokus ein statt dauerhaft sichtbar zu sein — ein
  laufender/fehlgeschlagener/gerade erfolgreicher Export bzw. eine
  Notiz-Übernahme bleibt dabei immer sichtbar, unabhängig von Hover/Fokus.
- Einstellungen: der KI-Provider-Bereich ist überarbeitet — konfigurierte
  Provider, die optionale Risiko-Zweitmeinung und das Formular für einen
  neuen Provider stehen jetzt in klar umrandeten, gruppierten Karten statt
  einer kontrastarmen, ungegliederten Liste; alle Formularfelder haben
  einen sichtbaren Fokus-Rahmen, der Format-Hinweis für den API-Key und
  das Testen-Ergebnis erscheinen als eigene, klar erkennbare Hinweisboxen.

### Fixed
- Chat-Eingabefeld: Enter während einer laufenden IME-Komposition (z. B.
  Kandidatenauswahl bei ostasiatischer Eingabe, Akzent-Eingabe über
  Tottasten) sendet nicht mehr vorzeitig eine noch unfertige Eingabe.
- Einstellungen: einige Kategorien ("Anzeige & Sprache", "Sitzungen &
  Daten", "MCP-Server") zeigten ihren Titel doppelt (einmal von der
  Einstellungen-Navigation, einmal als eigene Überschrift im Inhalt).
- Einstellungen: die Navigationseinträge "Sitzungen & Daten" und
  "MCP-Server" standen unabhängig von der UI-Sprache immer auf Deutsch da
  — erscheinen jetzt in Englisch übersetzt, wenn die UI-Sprache Englisch
  ist.

### Security
- Kommando-Ausgaben mit Unix-Passwort-Hashes (z. B. `/etc/shadow`,
  Apache-`.htpasswd`) gingen bislang unredigiert an den KI-Anbieter bzw.
  einen verbundenen MCP-Client weiter, wenn der Nutzer das zugrunde
  liegende Kommando (nach dem üblichen Bestätigungsdialog) ausführen
  ließ — die Redaction kannte deren Hash-Format schlicht nicht. Erkennt
  jetzt gängige Crypt-Hash-Formate (MD5-, SHA-256-, SHA-512-, yescrypt-,
  scrypt- und bcrypt-Hashes) und ersetzt nur den Hash-Anteil, Nutzername
  und übrige Felder bleiben lesbar.
- Aus demselben Grund gingen bislang auch DB-Connection-Strings
  (Postgres/MySQL/MariaDB/MongoDB/Redis/AMQP, Passwort zwischen `:` und
  `@`) sowie Provider-Tokens mit eindeutigem Präfix (Slack, Stripe,
  Google-API, npm) unredigiert weiter. Erkennt jetzt auch diese — bei
  Connection-Strings bleibt nur das Passwort-Segment ersetzt, Schema/
  Nutzername/Host/Datenbank bleiben lesbar.

## [0.4.6] — 2026-09-10

### Added
- Dateimanager: Spaltenbreiten (Name, Größe, Rechte, Geändert) lassen sich
  jetzt per Ziehen anpassen (Mindestbreiten, Name bleibt der flexible
  Rest).
- Die Aufteilung zwischen KI-Bereich und SSH-/SFTP-Bereich (Terminal/
  Dateimanager) lässt sich per Ziehen am Trenner anpassen (Mindestgrößen
  für beide Bereiche, Fallback auf die Standardaufteilung bei sehr kleinen
  Fenstern). Beide Einstellungen überleben einen Neustart.
- Dateimanager: Rechtsklick öffnet jetzt dasselbe Kontextmenü wie das
  Drei-Punkte-Symbol, mit neuen Aktionen — Herunterladen direkt ins
  Standard-Downloadverzeichnis oder per Dialog an einen gewählten Ort
  (auch für Ordner, rekursiv), Dateiinhalt kopieren, Pfad kopieren,
  Eigenschaften (Größe, Rechte numerisch + symbolisch, Besitzer/Gruppe,
  Änderungsdatum) und Aktualisieren.
- Dateimanager: server-verändernde Aktionen — Rechte bearbeiten (chmod,
  Checkbox-Matrix + numerisch, optional rekursiv für Ordner), Löschen jetzt
  auch für Ordner (rekursiv, mit Datei-/Ordner-Anzahl in der Bestätigung),
  Umbenennen mit Kollisionswarnung, Verschieben per Ausschneiden/Einfügen
  (ebenfalls mit Kollisionswarnung) und Hochladen mit Diff-Vorschau beim
  Überschreiben einer bestehenden Datei. Keine Bestätigung außer bei
  irreversiblen/überschreibenden Aktionen — manuelle Dateibrowser-Aktionen
  laufen (wie das Terminal) nicht durch die Filter-Engine.
- Einstellungen: neue Kategorie "Dateien" — pro Dateiendung ein lokales
  Standardprogramm festlegen (Fallback: Betriebssystem-Standard). Rein
  lokal, ohne Server-Bezug.
- Dateimanager: "Lokal öffnen…" lädt eine Datei in ein kontrolliertes
  Temp-Verzeichnis herunter, öffnet sie mit dem festgelegten (oder dem
  Betriebssystem-Standard-)Programm, und bietet nach einer erkannten
  lokalen Änderung automatisch den Upload an — inklusive Diff-Vorschau und
  einer Warnung, falls sich die Datei auf dem Server seit dem Download
  ebenfalls geändert hat. Die Bearbeitungskopie wird beim Beenden der
  Bearbeitung bzw. spätestens beim Trennen der Verbindung aufgeräumt.
- Datei-Diff-Vorschauen (Notiz-/Dateischreibvorgänge, Datei-Überschreiben
  beim Hochladen) zeigen jetzt die Zeilennummer vor jeder hinzugefügten
  oder entfernten Zeile.

### Fixed
- Dateimanager: das Drei-Punkte-Menü an einem Eintrag ließ sich oft nicht
  öffnen bzw. schloss ein gerade erst geöffnetes Menü eines anderen
  Eintrags sofort wieder — verursacht durch einen mit jedem weiteren Klick
  kollidierenden internen "Klick-außerhalb-schließt"-Mechanismus.

## [0.4.5] — 2026-09-09

### Fixed
- **Kritische Regression aus 0.4.4 behoben**: 0.4.4 startete auf Windows
  überhaupt nicht mehr (`Migrate(VersionMismatch(1))`-Absturz bei jedem
  Start, per Tester-Log bestätigt). Ursache war ein Seiteneffekt des
  CI-Fixes aus 0.4.4 selbst (`.gitattributes`) — die Windows-CI checkte
  dadurch die SQL-Migrationsdateien mit anderen Zeilenenden aus als bei
  0.4.1–0.4.3, was die zur Compile-Zeit eingebettete Prüfsumme der ersten
  Migration änderte und sie gegen bestehende Datenbanken bestehender
  Windows-Installationen ungültig machte. Eingegrenzt auf das, was
  tatsächlich betroffen war (Rust-Quelldateien) — SQL-Migrationen
  behalten ihr ursprüngliches Checkout-Verhalten.

## [0.4.4] — 2026-09-09

### Fixed
- Titelleisten-Buttons auf Windows/Linux waren die ganze Zeit sichtbar,
  aber mit sehr schlechtem Kontrast (kaum sichtbare graue Icons, nur
  "Schließen" reagierte erkennbar auf Hover): `tauri-plugin-decoration`
  färbt seine Controls standardmäßig für eine **helle** Titelleiste ein
  und wechselt nur bei einem im Betriebssystem eingestellten dunklen
  Modus auf helle Icons — unsere App ist aber immer dunkel, unabhängig
  vom Windows-Theme. Erzwingt das dunkle Farbschema jetzt bedingungslos
  — auf Windows 11 bestätigt behoben.

## [0.4.3] — 2026-09-09

### Fixed
- Zwei von `tauri-plugin-decoration` laut eigener Doku verlangte
  CSS-Variablen (`--tauri-plugin-decoration-titlebar-height`/`-z-index`)
  waren nie gesetzt — jetzt gesetzt. War letztlich nicht die Ursache der
  gemeldeten Sichtbarkeitsprobleme (s. 0.4.4), aber eine für sich
  genommen korrekte Ergänzung laut Plugin-Dokumentation.

## [0.4.2] — 2026-09-09

### Added
- Version + Commit-Hash (`0.4.2 (a5b3e01)`) sind jetzt auf einen Blick
  sichtbar: in der ersten Log-Zeile, kopierbar in einer neuen "Über"-
  Kategorie der Einstellungen, und (nur während der 0.x-Testphase, leicht
  abschaltbar) zusätzlich in der Titelzeile — identifiziert einen Bug-
  Report/ein Log jetzt eindeutig, auch wenn mehrere Builds dieselbe
  Version tragen.

## [0.4.1] — 2026-09-09

### Added
- Einstellungen neu strukturiert: Navigation links, Inhalt rechts (statt
  einer langen, ungegliederten Liste) — Kategorien für KI-Provider, Anzeige
  & Sprache, Diagnose, Sitzungen & Daten und MCP-Server.
- KI-Provider-Formular: sofortiger, rein lokaler Hinweis, falls ein
  eingegebener API-Key nicht zum erwarteten Format des gewählten Providers
  passt (nur ein Hinweis, blockiert nie das Speichern).
- KI-Provider-Formular: "Zugangsdaten testen"-Button prüft die gerade
  eingegebenen, noch nicht gespeicherten Zugangsdaten mit einem echten
  Mini-Request und zeigt, ob sie gültig sind, die Authentifizierung
  fehlschlägt, oder der Provider nicht erreichbar ist.

### Fixed
- Ein eingefügter API-Key, ein Server-Passwort/Sudo-Passwort oder eine
  Key-Passphrase mit einem angehängten Zeilenumbruch/Leerzeichen (z. B. von
  einem Copy-Paste unter Windows) wird jetzt beim Speichern getrimmt, statt
  die Authentifizierung mit "Credentials ungültig" scheitern zu lassen.
- Windows: die Titelleiste fällt bei einem Aktivierungsfehler jetzt sauber
  auf die volle native Titelleiste zurück (inkl. funktionierender
  Minimieren-/Maximieren-/Schließen-Controls und Fenster-Ziehen), statt in
  einem kaputten Zwischenzustand hängen zu bleiben.
- Ein vom KI-Provider zurückgemeldetes Rate-Limit (HTTP 429) — bislang der
  häufigste Grund, warum die KI mitten in einer Sitzung ohne jede Meldung
  aufhörte zu antworten — wird jetzt automatisch mit Backoff und
  `Retry-After`-Berücksichtigung wiederholt; die mehreren KI-Anfragen pro
  Nachricht (Hauptantwort, Risiko-Zweitmeinung, Einschleusungs-Check) werden
  zeitlich entzerrt statt als Burst abgeschickt. Scheitert es trotzdem, zeigt
  der Chat jetzt eine eigene, handlungsanleitende Meldung ("bitte kurz warten
  und erneut senden") statt der generischen "Provider-Konfiguration prüfen"-
  Meldung.

### Security
- Bei einem Fehler eines KI-Providers (falscher API-Key, Rate-Limit,
  Netzwerkfehler, Modell nicht gefunden) landet die Fehlerantwort des
  Providers jetzt redigiert im Log, ergänzend zur bestehenden
  UI-Meldung — erleichtert die Diagnose, ohne je ein Secret preiszugeben.

## [0.4.0] — 2026-09-07

Erste öffentliche Testversion.

### Added
- Word-Export als erstes Pro-Modul **(Pro)**.
- Lizenz-Eingabe-UI mit Live-Aktivierung **(Pro)**.

### Security
- Ressourcen-Caps gegen feindliche/fehlerhafte Server: ein Output-Cap, der
  vorher erst nach vollständigem Puffern griff, begrenzt jetzt bereits
  während des Streamings; ein expliziter Rekursions-Cap gegen
  verschachtelte Command-Substitution.
- Integrität der Fencing-Markierungen für nicht vertrauenswürdigen Inhalt
  im KI-Kontext abgesichert.

### Fixed
- Härtungsrunde für die Testphase: verwaiste Keychain-Einträge nach einem
  fehlgeschlagenen Server-Anlegen werden jetzt zuverlässig zurückgerollt,
  Fehlermeldungen (falscher API-Key, Host nicht erreichbar, Provider nicht
  gestartet, …) nennen jetzt den nächsten Schritt statt roher Technik, und
  ein Absturz beim Start landet jetzt garantiert in der Logdatei statt
  spurlos zu verschwinden.

## [0.3.0] — Initial Early Access

Erste zusammenhängende Version: SSH-Client mit KI-Copilot und
Filter-/Policy-Engine mit Bestätigungs-Workflow für jedes vorgeschlagene
Kommando, Server-/Gruppen-/Regel-/Notizverwaltung, MCP-Server-Anbindung,
persistente Chat-Sessions, Risiko-Indikatoren, Multi-Provider-KI-Anbindung
(Anthropic, OpenAI, Ollama, generisch OpenAI-kompatibel) mit Redaction
sensibler Inhalte, macOS-Build signiert und notarisiert.
