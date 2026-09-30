# Spec 0095 — Redactor: Passwort-Argumente von Kommandozeilenprogrammen und weitere Token-Formen

Status: freigegeben · Backlog: BL-0118 · Gate: —
Zweck: Der Redactor erkennt Passwörter, die als Argument gängiger Kommandozeilenprogramme übergeben werden, sowie die offenen Token- und Hash-Formen aus BL-0118 — ohne dass eine heute redigierte Eingabe weniger redigiert wird.
Review-Priorität: ERHÖHT (Redactor)

## 1. Ist-Stand (origin/main aa316fd)

1. **Aufbau** (gelesen): `DefaultOutputRedactor` wendet die Regeln aus
   `built_in_patterns()` nacheinander an; die Reihenfolge ist tragend
   (Kommentare zu Spec 0068/0078 und Q-BL-0248 in `built_in_patterns`, ADR 0069, BL-0257
   „Anker-Diebstahl": eine frühe Regel kann einer späteren den Anker
   zerschneiden). Am Listenanfang stehen Kopien der Schlüsselmuster
   (Spec 0078 §9).
2. **Gemessen** mit `DefaultOutputRedactor::new().redact_text` (Wegwerf-Crate,
   Pfadabhängigkeit auf `crates/core`, `cargo run`; Protokoll mit allen 39
   Eingaben in der Beilage):
   - **Schon erkannt**, obwohl BL-0118 sie als offen führt: `mssql://`,
     `sqlserver://`, `oracle://`, `jdbc:`-URLs (Zugangsdaten und
     `password=`), ADO-Strings `Password=…;`, `glpat-`, nackte `sk-ant-`/
     `sk-proj-`, `x-api-key:`, `Authorization: Basic`, Zugangsdaten in
     `https://`/`ftp://`-URLs, `wget --password=`.
   - **Nicht erkannt:** Passwort-Argumente von `mysql`/`mariadb`/
     `mysqldump` (`-p<pw>`, `-p'<pw>'`), `sshpass -p`, `curl -u`/`--user
     user:pw`, `htpasswd -b`, `redis-cli -a`, `smbclient -U user%pw`,
     `openssl … pass:<pw>`; Tokens Twilio (`SK` + 32 Hex), SendGrid
     (`SG.…`), Azure (`AccountKey=`); Hashes Argon2 (`$argon2id$…`),
     phpass (`$P$`, `$H$`); JWT ohne `Bearer`; `user:pw@host` ohne Schema.
3. **Wächter aus Spec 0094:** `test_redactor_does_not_yet_know_command_line_password_arguments`
   (`core/src/ai/tests.rs`) hält fest, dass `mysql -p'…'`, `mysql -p…`,
   `sshpass -p …` und `curl -u …` heute **nicht** erkannt werden, weil die
   Log-Tests aus Spec 0094 (T1–T3, T6 u. a.) genau diese Formen als
   „vom Redactor unerkanntes Geheimnis" benutzen. Er wird mit dieser Spec rot.
   Betroffen sind (je mit Doc-Kommentar „Form, die der Redactor nicht
   erkennt"): Filter T1–T3 in `core/src/filter/tests.rs`, T4/T5/T8c in
   `ai-providers/src/request_logging.rs`, zwei T6-Tests in
   `app-logic/…/action_exec/tests_files_and_ledger.rs`, zwei T7-Tests in
   `mcp-server/src/tool_server.rs`. Mehrere prüfen zusätzlich das Wort
   `mysql` (ab `info` abwesend, auf `debug` vorhanden).
4. **Fremde Programme** (Beilage, Abschnitt Messung 2):
   - `mysql -p <wert>` und `--password <wert>` mit Leerzeichen sind kein
     Passwort: ohne angehängten Wert fragt der Client interaktiv, der
     folgende Wert ist der Datenbankname (MySQL-Client-Dokumentation zu
     `--password[=password], -p[password]`; gelesen, mysql war lokal nicht
     verfügbar). `ssh -p 2222` ist ein Port.
   - `curl -uadmin:pw`, `curl -sSuadmin:pw` und `curl -u admin:pw` senden
     alle `Authorization: Basic` mit `admin:pw` (gemessen gegen einen
     lokalen `nc`-Listener).
   - `openssl enc -help`: `-k val  Passphrase` (der Wert selbst),
     `-pass val  Passphrase source` (gemessen, OpenSSL 3.6.3).
   - `htpasswd` (Usage, gemessen): `-b[…] [-C cost] [-r rounds] file user
     password` und `-nb[…] [-C cost] [-r rounds] user password`.
5. **Absorb-Schritt:** Nach allen Regeln schluckt
   `absorb_fragments_next_to_placeholders` Zeichen aus
   `[A-Za-z0-9+/.~-]` links und rechts eines Platzhalters. Aus
   `-p[REDACTED]` wird dadurch `[REDACTED]` (aus der Regex gelesen).
6. **Kommentare:** `redactor.rs` nennt Argon2 und phpass ausdrücklich als
   „bewusst NICHT abgedeckt"; der Typ-Doc-Kommentar listet die Muster.
7. Die schon erkannten Formen aus Punkt 2 haben im Repo **keinen** Test
   (`grep` nach `mssql://|sqlserver://|oracle://|jdbc:|Server=` über
   `crates/` → kein Treffer).

## 2. Teil 0

Teil 0: entfällt. Das Verhalten jeder Regel ist mit `redact_text` direkt
messbar; die Tests in §7 sind die Messung.

## 3. Ziel und Nicht-Ziele

Ziel: Die in §1.2 als „nicht erkannt" gelisteten Formen werden redigiert,
bis auf die Nicht-Ziele unten; der Rest der Eingabe bleibt lesbar
(Programm, Schalter, Benutzername, Host).

Nicht-Ziele:
- `user:pw@host` **ohne** Schema — zu nah an harmlosen Formen
  (`scp`-Ziele, IPv6, Zeitangaben); bleibt offen.
- `echo <pw> | sudo -S …` — das Geheimnis ist im Text nicht als solches
  erkennbar.
- `mysql -p <wert>` und `--password <wert>` mit Leerzeichen (§1.4) und
  `ssh -p <port>`.
- Restfall DB-Strings mit rohem `,`/`;`/`"` im Passwort — braucht
  Prozent-Dekodierung oder URL-Parsing (BL-0257).
- Solaris-Crypt-Exoten (`$sha1$`, `$md5$`) — selten; bleiben offen.
- Umbau der Regel-Reihenfolge oder URL-Parsing statt Regex (BL-0257).
- Generisches Hoch-Entropie-Fallback (BL-0118: verworfen).
- Datenbank-Inhalte (BL-0295) — die neuen Regeln wirken dort, wo der
  Redactor heute schon läuft, nicht an neuen Stellen.

## 4. Anforderungen

**A1 — Passwort-Argumente.** MUSS redigiert werden (nur der Wert, der
Rest bleibt stehen):
- A1.1 `mysql`, `mariadb`, `mysqldump`, `mysqladmin`: `-p<wert>`
  (angehängt), auch in einfachen oder doppelten Anführungszeichen.
  `--password=<wert>` ist schon erkannt. Formen mit Leerzeichen bleiben
  unverändert (§1.4).
- A1.2 `sshpass -p <wert>` und `sshpass -p<wert>`.
- A1.3 `curl` mit `-u`/`--user`, getrennt oder angehängt (`-uuser:wert`),
  auch als letzter Buchstabe eines Kurzschalterblocks (`-sSu`, `-sSuuser:…`),
  gefolgt von `user:wert`, auch in Anführungszeichen: nur der Teil nach
  dem ersten `:`.
  `curl -u user` ohne `:` bleibt unverändert.
- A1.4 `htpasswd` mit einem Schalterblock, der `b` enthält (`-b`, `-bc`,
  `-nbB` …): nach Schaltern und ihren Werten (`-C <n>`, `-r <n>`) ist das
  Passwort mit `n` im Block das **zweite**, sonst das **dritte**
  Positionsargument (§1.4). Was danach kommt (`> out`, `| tee`), bleibt.
- A1.5 `redis-cli -a <wert>` und `--pass <wert>`.
- A1.6 `smbclient`/`rpcclient` `-U user%wert` und `--user=user%wert`.
- A1.7 `openssl`: bei `-pass`, `-passin`, `-passout` die Form
  `pass:<wert>`, auch in Anführungszeichen (die Formen `env:`, `file:`, `fd:`, `stdin` bleiben); bei
  `-k <wert>` der Wert selbst (§1.4).
- A1.8 Die Regeln greifen auch mitten in Verkettungen (`&&`, `;`, `|`,
  `$(…)`) und nach `sudo`, `env VAR=x`, absoluten Pfaden
  (`/usr/bin/mysql`).

**A2 — Tokens und Hashes.** MUSS redigiert werden:
- A2.1 Twilio API-Key-SID `SK` + 32 Hex-Zeichen als ganzes Wort.
- A2.2 SendGrid `SG.` + 22 + `.` + 43 Zeichen aus `[A-Za-z0-9_-]`.
- A2.3 Azure-Connection-Strings: Wert von `AccountKey=` und
  `SharedAccessKey=` bis zum nächsten `;`, Leerraum oder Anführungszeichen
  (Base64 mit `+`, `/`, `=`).
- A2.4 Argon2 (`$argon2id$`, `$argon2i$`, `$argon2d$` mit Parametern,
  Salt, Hash) und phpass (`$P$`/`$H$` + 31 Zeichen) — wie die bestehenden
  Crypt-Hashes.
- A2.5 JWT ohne Präfix: drei durch `.` getrennte base64url-Teile, die ersten
  beiden beginnen mit `eyJ`.

**A3 — Nichts wird schwächer.** MUSS: Jede Eingabe der bestehenden
Redactor-Tests liefert dieselbe Ausgabe wie vorher. Kein bestehender
Testerwartungswert wird geändert, außer dem Wächter aus §1.3 (A4).

**A4 — Wächter aus Spec 0094 nachziehen.** MUSS: Die in §1.3 genannten
Log-Tests bekommen eine Geheimnis-Form, die auch nach dieser Spec
unerkannt bleibt: `mysql -u root -p <geheimnis>` (Leerzeichen, §3
Nicht-Ziel), damit ihre Prüfungen auf das Wort `mysql` gültig bleiben;
wo kein `mysql` geprüft wird, darf der Coder eine andere Nicht-Ziel-Form
wählen. Doc-Kommentare dieser Tests werden angepasst. Der Wächtertest
wird auf die neuen Formen umgestellt und prüft weiter, dass sie unerkannt
sind. Die Aussage der Log-Tests (kein Inhalt ab `info`) bleibt und muss
weiter gegen einen Stand ohne Spec 0094 scheitern.

**A5 — Keine Fehlalarme** auf den Gegenbeispielen in T12.

**A6 — Schon erkannte Formen festschreiben.** MUSS: Die in §1.2 als „schon
erkannt" gelisteten Formen bekommen je einen Regressionstest (BL-0118:
„Tests je Muster").

**A7 — Kommentare.** MUSS: Der Hinweis „bewusst NICHT abgedeckt: Argon2,
phpass" und der Typ-Doc-Kommentar in `redactor.rs` werden nachgezogen.

## 5. Design

- Wo eine Regel in `built_in_patterns()` steht, entscheidet der Coder,
  aber er begründet die Position gegen Anker-Diebstahl: Welche bestehenden
  Regeln können den neuen Anker vorher zerschneiden, welche neuen Regeln
  können bestehenden Ankern den Wert nehmen (Kommentar an der Regel,
  wie bei den bestehenden).
- Ersetzt wird der Wert durch den bestehenden Platzhalter; Programm und
  Benutzername bleiben stehen. Bei angehängten Werten ohne Trennzeichen (`-pX`,
  `sshpass -pX`) schluckt der bestehende Absorb-Schritt den Schalter mit (§1.5); das wird
  hingenommen, der Absorb-Schritt bleibt unverändert.

## 6. Sicherheits-Invarianten

- **Redaction nur verschärfen, nie lockern:** A3 plus T11.
- **Keine Wirkung auf Filter und Risiko:** Der Redactor läuft nur auf
  Ausgaben, Log- und KI-Kontext; die Filter-Engine sieht das Original.
  Kein Filter- oder Risikoverhalten und keine Erwartung dort ändert sich;
  in `core/src/filter/tests.rs` wechselt nur die Geheimnis-Form der
  Spec-0094-Log-Tests (A4).
- **Transparenz:** Nutzer und Oberfläche sehen weiterhin das
  Originalkommando (Event, Chat); geändert wird nur, was schon heute
  redigiert wird.

## 7. Tests

Jeder Test läuft über `redact_text` bzw. `redact`; Geheimnis `Geheim-0095`.
T1–T10 müssen gegen den heutigen Stand scheitern (der Coder belegt das je
Test).

- **T1 mysql-Familie:** `mysql -pGeheim-0095 -u root`,
  `mysql -p'Geheim-0095'`, `mysql -p"Geheim 0095"`, `mysql -p'a'"Geheim-0095"`,
  `mysqldump -u r -pGeheim-0095 db`, `mariadb -pGeheim-0095`,
  `/usr/bin/mysql -pGeheim-0095`, `sudo -u x mysql -pGeheim-0095`,
  `command mysql -pGeheim-0095`, `\mysql -pGeheim-0095`,
  `MYSQL -pGeheim-0095` → kein Treffer; `mysql`, `-u root`, `db` stehen
  noch da.
- **T2 sshpass:** `-p Geheim-0095` und `-pGeheim-0095` → kein Treffer;
  `ssh host` steht noch da.
- **T3 curl:** `curl -u admin:Geheim-0095 https://x`,
  `curl --user admin:Geheim-0095 …`, `curl -sSu admin:Geheim-0095 …`,
  `curl -uadmin:Geheim-0095 …`, `curl -u 'admin:Geheim 0095' …` → kein
  Treffer, `admin` steht in allen Formen noch da.
- **T4 htpasswd:** `htpasswd -b f user Geheim-0095`,
  `htpasswd -nbB user Geheim-0095 > out`,
  `htpasswd -bB -C 12 f user Geheim-0095` → kein Treffer; `user`, `out`
  und `12` stehen noch da.
- **T5 redis/smb/openssl:** `redis-cli -a Geheim-0095`,
  `smbclient -U user%Geheim-0095 //h/s`,
  `openssl enc -pass pass:Geheim-0095`, `openssl enc -pass "pass:Geheim 0095"`,
  `openssl rsa -passin pass:Geheim-0095`, `openssl enc -k Geheim-0095`,
  `sshpass -p 'Geheim 0095' ssh h` → kein Treffer.
- **T6 Verkettung:** `cd /x && mysql -pGeheim-0095; echo ok`,
  `echo $(sshpass -p Geheim-0095 ssh h cat f)`,
  `env LANG=C curl -u a:Geheim-0095 x | jq .`, Tab statt Leerzeichen
  (`sshpass\t-p\tGeheim-0095`), mehrere Leerzeichen
  (`curl  -u   a:Geheim-0095`) → kein Treffer.
- **T7 Tokens:** Twilio-SID, SendGrid-Key, Azure `AccountKey=` und
  `SharedAccessKey=` → kein Treffer; `AccountName=` bleibt lesbar.
- **T8 Hashes:** Argon2id-, Argon2i-, phpass-`$P$`- und `$H$`-Hash in einer
  `/etc/shadow`-artigen Zeile → Hash ersetzt, Benutzername bleibt.
- **T9 JWT:** nackter JWT in einer Textzeile und in `?token=`-freiem
  JSON-Wert → ersetzt.
- **T10 Mehrzeilig und Ausgabe:** dieselben Formen in `stdout`/`stderr`
  eines `CommandOutput` über `redact` → kein Treffer.
- **T11 Anker-Diebstahl (adversarial):** Kombinationen, in denen neue und
  bestehende Regeln sich berühren, müssen alle Geheimnisse ersetzen:
  `mysql -p'Geheim-0095' --host=db://u:Zwei-0095@h`,
  `curl -u a:Geheim-0095 https://u:Zwei-0095@h/?password=Drei-0095`,
  `sshpass -p Geheim-0095 ssh h 'cat <<EOF\n-----BEGIN PRIVATE KEY-----…'`,
  ein JWT direkt nach `Authorization: Bearer ` (bestehende Regel) →
  kein Geheimnis im Ergebnis.
- **T12 Keine Fehlalarme:** `ssh -p 2222 host`, `mysql -u root -p mydb`,
  `mysql -P 3306 -h db`, `mysql -P3306 -h db`, `mysql --password mydb`,
  `curl -u admin https://x`, `curl -k https://x`, `sshpass -f pwfile ssh -p 2222 h`,
  `openssl enc -pass env:PW`, `openssl enc -pass file:/k`,
  `htpasswd -D f user`, `scp user@host:/path .`, `git log -p`,
  `grep -p foo`, eine UUID, ein Git-Hash (40 Hex), `SKU-12345`,
  `eyJ` allein → Ausgabe unverändert.
- **T13 Nichts schwächer:** Alle bisherigen Tests in `core/src/ai`
  unverändert grün (Wächter, muss heute nicht scheitern). Der Wächter aus
  §1.3 wird nach A4 umgestellt.
- **T14 Spec-0094-Log-Tests:** nach A4 weiter grün; der Coder belegt
  erneut, dass sie gegen den Stand vor Spec 0094 scheitern.
- **T15 Schon erkannte Formen (A6):** je ein Test für `mssql://`,
  `sqlserver://`, `oracle://`, `jdbc:` (Zugangsdaten und `password=`),
  ADO `Password=…;`, `glpat-`, `sk-ant-`, `sk-proj-`, `x-api-key:`,
  `Authorization: Basic`, `https://`/`ftp://`-Zugangsdaten,
  `wget --password=` → kein Treffer (Wächter, muss heute nicht scheitern).

## 8. Offene Punkte

Keine.

## 9. Klarstellungen

(leer)

## Umsetzung

**Teil 0:** entfällt (§2).

**Reihenfolge:**
1. `test: move the spec 0094 log tests to a secret form the redactor keeps unrecognized [BL-0118]` — A4, T14, A6/T15.
2. `feat(core): redact passwords passed as command-line arguments [BL-0118]` — A1, A7, T1–T6, T10–T12.
3. `feat(core): redact Twilio, SendGrid and Azure keys, Argon2 and phpass hashes and bare JWTs [BL-0118]` — A2, T7–T9, T11, T12.

**Priorität:** ERHÖHT. Angriffsrichtungen für den Review:
- Anker-Diebstahl in beide Richtungen: neue Regel zerschneidet einen
  bestehenden Anker, bestehende Regel nimmt dem neuen Anker den Wert.
- Quoting: `-p"a b"`, `-p'a'"b"`, Escapes (`-p\'x`), Wert mit `;`, `&`, `|`.
- Whitespace: Tab statt Leerzeichen, mehrere Leerzeichen, Zeilenumbruch
  zwischen Schalter und Wert.
- Pfade und Präfixe: `/usr/local/bin/mysql`, `sudo -u x mysql`,
  `command mysql`, `\mysql`.
- Groß-/Kleinschreibung (`MYSQL`, `-P` als Port).
- Laufzeit bei langen Eingaben (die `regex`-Crate sucht linear; trotzdem
  1 MB ohne Treffer vorher/nachher messen).
- Fehlalarme, die Kommandoausgaben unlesbar machen (T12).

**Aufteilung:** ein Lauf, Opus — Kern ist der Redactor; die Test-Umstellung
aus A4 in vier Crates gehört dazu, weil sie an denselben Formen hängt.

**Berührte Module:** `crates/core/src/ai/redactor.rs`,
`crates/core/src/ai/tests.rs`, die Log-Tests aus Spec 0094
(`core/src/filter/tests.rs`, `ai-providers`, `app-logic`, `mcp-server`).

**Melde zurück:** je Regel die gewählte Position mit Begründung gegen
Anker-Diebstahl; je Test den Beleg „scheitert gegen den alten Stand";
Laufzeit von `redact_text` auf 1 MB ohne Treffer vorher/nachher.
