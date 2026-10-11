# Spec 0068 — Sicherheitshärtung: Redaction, Secret-Pfade, Sudo-Ankündigung, mehrere Tool-Calls, Timeouts

Status: umgesetzt
Zweck: Fünf Schutzmaßnahmen, die ein sicherheitsbewusster Nutzer beim ersten Ausprobieren prüfen würde: nackte API-Keys werden geschwärzt, das Lesen von Secret-Dateien verlangt immer eine Bestätigung, ein Sudo-Fallback beim Schreiben wird vorab angekündigt, mehrere Tool-Calls in einer Antwort werden einzeln entschieden, und Wartezeiten auf Provider und Host-Key-Bestätigung sind begrenzt.
Bezüge: Spec 0002 (Filter-Engine), Spec 0006 und Spec 0095 (Redaction), Spec 0018 (Sudo-Passwort), Spec 0020 (Datei schreiben), Spec 0021 (Fortsetzung), Spec 0026 (Risiko-Einstufung), Spec 0039 (Eskalation nach untrusted Inhalt), Spec 0060 (Pfad-Auswertung), Spec 0065 (abgeschnittene Antworten), Spec 0066 (Stopp, Prompt-Regel „sensible Dateien nicht lesen"), Spec 0069 (Fehlermeldungen, Zeitgrenzen), Spec 0078 (Passwort in URLs), Spec 0092 (rotes Risiko), ADR 0058 (Abschnitt 8, `sftp-server`), ADR 0069, ADR 0108 (Host-Key-Abfrage beenden).
Review-Priorität: ERHÖHT

**Grundsatz für alle fünf Teile:** Jede Änderung verschärft, keine lockert. Kein
bestehender Schutz, keine bestehende Regel und kein bestehendes Muster wird
schwächer.

## Teil 1 — Redaction: nackte API-Keys und Auth-Header

Die Redaction erkennt Schlüssel auch dann, wenn kein Stichwort wie
`password=` davorsteht — so liegen sie in `.env`-Dateien, Konfigurationen und
Logs. Erkannt und durch `[REDACTED]` ersetzt werden:

- Anthropic-Keys (`sk-ant-…`, auch Admin- und OAuth-Formen),
- OpenAI-Keys (`sk-proj-…`, `sk-svcacct-…`, `sk-admin-…`, das ältere Format
  mit festem Wasserzeichen),
- OpenRouter (`sk-or-v…`), GitLab (`glpat-…` samt routbarem Suffix, ebenso
  Runner- und Deploy-Token), Hugging Face (`hf_…`, `api_org_…`), Groq
  (`gsk_…`), xAI (`xai-…`),
- HTTP-Header `x-api-key`, `api-key`, `x-goog-api-key` und
  `Authorization: Basic …` bzw. `Token …` (auch `Proxy-Authorization`),
- Zugangsdaten in beliebigen URLs (`schema://user:passwort@host`), nicht nur
  in Datenbank-Schemata; ersetzt wird nur das Passwort,
- die Formate der Dateien aus Teil 2 (Docker-`auth`, kubeconfig-
  `client-key-data`, `.netrc`), damit auch ein bestätigtes Lesen sie nicht im
  Klartext an die KI gibt.

Regeln dafür:

- **Reihenfolge.** Neue Muster stehen so, dass kein anderes sie zerteilt und
  sie kein bestehendes zerstören; Muster, die einen Header- oder
  Schlüsselnamen mitverbrauchen, stehen am Ende. Die Zeichenklassen der
  URL-Regel sind nicht gieriger als die der Datenbank-Regel (frühere
  Regressionen).
- **Mindest- statt exakter Länge** hinter einem eindeutigen Präfix: Ein leicht
  abweichend langer echter Key rutscht nicht durch.
- **Keine Falsch-Positive** auf normalen Text: ein bloßes `sk-`, Git-SHAs,
  UUIDs, Base64-Bilddaten, Bezeichner wie `hf_hub_download`, Fließtext wie
  „Basic authentication" bleiben unverändert.
- **Kein generisches Hoch-Entropie-Muster** (bewusst verworfen: zu viele
  Falsch-Positive).

## Teil 2 — Lesebefehle auf Secret-Pfade verlangen Bestätigung

Die Prompt-Regel „sensible Dateien nicht lesen" (Spec 0066) ist weich. Als
harte Ergänzung stuft die Risiko-Einstufung das Lesen von Secret-Pfaden so ein,
dass es **immer** eine Bestätigung verlangt.

- **Pfade** (Auswahl): private SSH-Schlüssel `~/.ssh/id_*` (nicht `*.pub`) und
  Host-Schlüssel, `*.pem`, `*.key`, Schlüsselspeicher (`.p12`, `.pfx`, `.jks`),
  `.env`-Dateien (auch `.env.example`: Beispieldateien enthalten oft echte
  Werte), `/etc/shadow`, `/etc/gshadow`, `~/.aws/credentials`,
  `~/.docker/config.json`, `~/.kube/config`, `~/.netrc`, `~/.pgpass`,
  `~/.git-credentials`, `.npmrc`, `.pypirc`, `.my.cnf`, Vault-Token, GnuPG-
  Schlüsselbund, private TLS-Schlüssel, Shell-Historien, Prozess-Umgebungen.
- **Befehle**, die Inhalt in die Ausgabe oder vom Server weg bringen: unter
  anderem `cat`, `less`, `head`, `tail`, `grep`, `sed`, `awk`, `xxd`, `od`,
  `strings`, `base64`, `openssl`, Editoren, Interpreter sowie `curl`, `wget`,
  `scp`, `rsync` — **und die Datei-Lese-Aktion** (als `sftp-read`), in Chat
  und MCP. Nicht erfasst werden `cp`, `install`, `mv` und Umleitungen ohne
  Ausgabe: Das sind die Wege, die die Prompt-Regel empfiehlt, weil der Inhalt
  dabei nie in einer Ausgabe landet.
- **Wirkung.** Ein Treffer macht aus automatischer Ausführung immer eine
  Bestätigung — auch wenn eine Allow-Regel greift. Das ist reine Eskalation
  wie in Spec 0039: `Deny` bleibt `Deny`, nichts wird gelockert. Der Dialog
  nennt den Grund „Liest eine Datei mit Zugangsdaten (z. B. SSH-Schlüssel,
  .env, /etc/shadow) – erfordert immer Bestätigung". Hat die App den Verdacht
  auf eingeschleuste Anweisungen, hat dieser Grund Vorrang.
- **Im Zweifel streng.** Anführungszeichen und Backslashes im Pfad ändern das
  Ergebnis nicht. Platzhalter auf mögliche Secret-Dateien, `find -exec`/`xargs`
  mit Lesebefehl und Variablen im Pfad eskalieren. Verkettungen (`echo x; cat
  ~/.ssh/id_rsa`), verschachtelte Shells und `cd`-Präfixe werden über die
  Teilkommandos ausgewertet.
- **Nicht prüfbare Kommandos.** Ist ein Kommando zu lang für die Prüfung oder
  enthält es tiefer verschachtelte Shell-Code-Strings, als die Prüfung zerlegt
  (dasselbe gilt für die Prüfung auf `sftp-server`-Aufrufe), wird es ebenso
  eskaliert (nur automatische Ausführung → Bestätigung, `Deny` bleibt `Deny`).
  Der Dialog nennt dann den eigenen Grund „Kommando ist zu lang oder zu tief
  verschachtelt für eine Prüfung – erfordert immer Bestätigung", nicht
  „Liest eine Datei mit Zugangsdaten" bzw. „Startet sftp-server". Ein Verdacht
  auf eingeschleuste Anweisungen hat weiterhin Vorrang.
- **Ehrlich begrenzt.** Die Prüfung ist lexikalisch und über Variablen oder
  Symlinks umgehbar. Redaction (Teil 1) und Bestätigung bleiben die weiteren
  Schichten.

## Teil 3 — Sudo-Fallback beim Schreiben wird angekündigt

Beim Schreiben einer Datei (Spec 0020) nutzt die App das hinterlegte
Sudo-Passwort, wenn das normale Schreiben an fehlenden Rechten scheitert.

- Hat der Server ein Sudo-Passwort, zeigt der Bestätigungsdialog einer
  Schreib-Aktion **vor** der Bestätigung: „Fehlen die Rechte zum Schreiben,
  schreibt die App mit dem hinterlegten Sudo-Passwort (sudo)." (Für Kommandos
  mit `sudo` bleibt die bestehende Warnung „Wird mit hinterlegtem
  Sudo-Passwort ausgeführt.")
- Die Aktion verlangt dann immer eine Bestätigung, auch gegen eine Allow-Regel.
- **Der Fallback ist an die Ankündigung gebunden.** Er läuft nur, wenn der
  Dialog ihn angekündigt hat; sonst endet das Schreiben mit der Meldung, dass
  erhöhte Rechte nötig sind und kein Sudo-Passwort hinterlegt ist. Die App
  schiebt Sudo nie still nach.
- Gilt für Chat und MCP.

## Teil 4 — Mehrere Tool-Calls in einer Antwort

Liefert ein Provider mehrere vollständige Tool-Calls in einer Antwort (Anthropic
`tool_use`, OpenAI parallele `tool_calls`), werden sie nacheinander verarbeitet:

- Jede Aktion läuft einzeln durch die Filter-Engine und bekommt ihre eigene
  Entscheidung (Bestätigung oder automatische Ausführung).
- **Die Eskalation wirkt über Aktionen hinweg.** Hat Aktion 1 untrusted Inhalt
  gelesen, wird Aktion 2 derselben Antwort eskaliert (Spec 0039).
- **Nach einer Ablehnung wird nicht mehr automatisch ausgeführt.** Wird eine
  Aktion abgelehnt oder blockiert (auch ein vollständig geschwärzter
  Notiz-Vorschlag oder ein vom Nutzer bearbeitetes, dann blockiertes
  Kommando), verlangen die übrigen Aktionen derselben Antwort immer eine
  Bestätigung, weil sie auf der abgelehnten aufbauen können. Sie werden weder
  still mit abgelehnt noch still ausgeführt, sondern einzeln entschieden.
- Ein **Stopp** (Spec 0066) zwischen zwei Aktionen verhindert die Ausführung
  der folgenden.
- Ist die Antwort am Längenlimit abgeschnitten, wird der letzte, unvollständige
  Tool-Call nie ausgeführt (Spec 0065).

## Teil 5 — Zeitgrenzen

**5a — Modell-Erkennung und Zugangsdaten-Prüfung.** Verbindungsaufbau und
Lesen der Antwort sind begrenzt, mit derselben Zeitgrenze wie im Chat-Pfad
(90 Sekunden ohne Aktivität); es gibt keine zweite Konstante. Ein Server, der
nie antwortet, führt zum Fehler „Zeitüberschreitung" (Spec 0069), nicht zu
einem hängenden Dialog.

**5b — Host-Key-Bestätigung.**

- Die Abfrage wartet höchstens so lange wie eine Aktions-Bestätigung (eine
  Stunde). Geht das Frontend verloren, hängt der Verbindungsaufbau nicht ewig.
- **Zeitablauf ist Ablehnung.** Ein Host-Key wird durch Zeitablauf nie
  akzeptiert und nie als bekannt gespeichert. Die Verbindung endet mit der
  Meldung, dass die Host-Key-Bestätigung nicht rechtzeitig beantwortet wurde
  (nicht vertraut).
- **Nur der eigene Eintrag.** Jede Abfrage trägt eine eigene Kennung
  (Generation). Wiederholt der Nutzer den Verbindungsaufbau, während die alte
  Abfrage noch wartet, entfernt ein späteres Aufgeben der alten Abfrage nie die
  neue.
- Jeder Ausgang des Wartens (Entscheidung, Zeitablauf, Abbruch) meldet dem
  Frontend, dass genau diese Abfrage beendet ist; das Frontend schließt damit
  die passende Abfrage. Das Ereignis entscheidet nichts (ADR 0108).

## Sicherheitszusagen

- Kein Teil lockert einen bestehenden Schutz; alle Eskalationen gehen nur in
  Richtung Bestätigung.
- Das Lesen von Secret-Pfaden verlangt immer eine Bestätigung, auch gegen
  Allow-Regeln, in Chat und MCP; `Deny` bleibt `Deny`.
- Ein nicht prüfbares Kommando wird nie stillschweigend durchgelassen.
- Kein Sudo ohne Wissen des Nutzers.
- Nach einer Ablehnung läuft in derselben Antwort nichts mehr ohne Rückfrage.
- Ein Host-Key wird nie durch Zeitablauf akzeptiert.
- Von der Redaction erkannte Geheimnisse gehen nicht im Klartext an die KI.

## Grenzen

- Die Secret-Pfad-Prüfung ist lexikalisch; Variablen, Symlinks und Pfade, die
  erst zur Laufzeit entstehen, erkennt sie nicht.
- Die Redaction ist musterbasiert; ein unbekanntes Schlüsselformat wird nicht
  erkannt.
- Ein Befehl wie `cp ~/.ssh/id_rsa /tmp/x` löst keine Bestätigung aus Teil 2
  aus (er bringt keinen Inhalt in eine Ausgabe).
