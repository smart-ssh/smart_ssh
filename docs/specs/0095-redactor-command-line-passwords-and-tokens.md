# Spec 0095 — Redactor: Passwort-Argumente von Kommandozeilenprogrammen und weitere Token-Formen

Status: umgesetzt
Zweck: Der Redactor erkennt Passwörter, die als Argument gängiger Kommandozeilenprogramme übergeben werden, sowie weitere Token- und Hash-Formen. Programm, Schalter, Benutzername und Host bleiben lesbar.
Bezüge: Spec 0006 (Redaction), Spec 0078 (Zugangsdaten in URLs), Spec 0094 (Log), ADR 0069.

## 1. Passwort-Argumente

**A1.** Es wird nur der Wert redigiert; der Rest der Eingabe bleibt stehen.

- A1.1 `mysql`, `mariadb`, `mysqldump`, `mysqladmin`: `-p<wert>` (angehängt),
  auch in einfachen oder doppelten Anführungszeichen. `--password=<wert>`
  wurde schon vorher erkannt. Formen mit Leerzeichen (`mysql -p mydb`,
  `--password mydb`) bleiben unverändert: Ohne angehängten Wert fragt der
  Client interaktiv, der folgende Wert ist der Datenbankname.
- A1.2 `sshpass -p <wert>` und `sshpass -p<wert>`.
- A1.3 `curl` mit `-u`/`--user`, getrennt oder angehängt (`-uuser:wert`),
  auch als letzter Buchstabe eines Kurzschalterblocks (`-sSu`, `-sSuuser:…`),
  gefolgt von `user:wert`, auch in Anführungszeichen: redigiert wird der Teil
  nach dem ersten `:`. `curl -u user` ohne `:` bleibt unverändert.
- A1.4 `htpasswd` mit einem Schalterblock, der `b` enthält (`-b`, `-bc`,
  `-nbB` …): Nach den Schaltern und ihren Werten (`-C <n>`, `-r <n>`) ist das
  Passwort mit `n` im Block das zweite, sonst das dritte Positionsargument.
  Was danach kommt (`> out`, `| tee`), bleibt.
- A1.5 `redis-cli -a <wert>` und `--pass <wert>`.
- A1.6 `smbclient`/`rpcclient` `-U user%wert` und `--user=user%wert`.
- A1.7 `openssl`: bei `-pass`, `-passin`, `-passout` die Form `pass:<wert>`,
  auch in Anführungszeichen (`env:`, `file:`, `fd:`, `stdin` bleiben); bei
  `-k <wert>` der Wert selbst.
- A1.8 Die Regeln greifen auch mitten in Verkettungen (`&&`, `;`, `|`,
  `$(…)`) und nach `sudo`, `env VAR=x`, `command`, absoluten Pfaden
  (`/usr/bin/mysql`), mit Tab oder mehreren Leerzeichen, unabhängig von der
  Groß-/Kleinschreibung des Programmnamens.

Bei angehängten Werten ohne Trennzeichen (`-pX`, `sshpass -pX`) nimmt die
nachgelagerte Bereinigung der Platzhalter-Nachbarschaft den Schalter mit;
das wird hingenommen.

## 2. Tokens und Hashes

**A2.** Redigiert werden:

- A2.1 Twilio API-Key-SID: `SK` + 32 Hex-Zeichen als ganzes Wort.
- A2.2 SendGrid: `SG.` + 22 + `.` + 43 Zeichen aus `[A-Za-z0-9_-]`.
- A2.3 Azure-Connection-Strings: der Wert von `AccountKey=` und
  `SharedAccessKey=` bis zum nächsten `;`, Leerraum oder Anführungszeichen.
  `AccountName=` bleibt lesbar.
- A2.4 Argon2 (`$argon2id$`, `$argon2i$`, `$argon2d$` mit Parametern, Salt,
  Hash) und phpass (`$P$`/`$H$` + 31 Zeichen), wie die übrigen Crypt-Hashes.
- A2.5 Ein JWT ohne Präfix: drei durch `.` getrennte base64url-Teile, die
  ersten beiden beginnen mit `eyJ`.

## 3. Bereits erkannte Formen

**A6.** Diese Formen werden erkannt und bleiben es: `mssql://`,
`sqlserver://`, `oracle://`, `jdbc:`-URLs (Zugangsdaten und `password=`),
ADO-Strings `Password=…;`, `glpat-`, `sk-ant-`, `sk-proj-`, `x-api-key:`,
`Authorization: Basic`, Zugangsdaten in `https://`- und `ftp://`-URLs,
`wget --password=`.

## 4. Sicherheitszusagen

- **A3 — Nichts wird schwächer.** Jede Eingabe, die vor dieser Spec
  redigiert wurde, wird weiter mindestens so weit redigiert.
- Neue und bestehende Regeln dürfen einander den Anker nicht nehmen
  (Anker-Diebstahl): Kombinationen, in denen mehrere Geheimnisse in einer
  Zeile stehen — Passwort-Argument neben URL-Zugangsdaten, `?password=`,
  einem Private-Key-Block oder einem `Authorization: Bearer`-JWT —, werden
  vollständig redigiert. Die Reihenfolge der Regeln ist daher Teil des
  Verhaltens.
- **A5 — Keine Fehlalarme.** Harmlose Eingaben bleiben wörtlich unverändert,
  z. B. `ssh -p 2222 host`, `mysql -u root -p mydb`, `mysql -P3306 -h db`,
  `curl -u admin https://x`, `curl -k https://x`, `sshpass -f pwfile ssh -p 2222 h`,
  `openssl enc -pass env:PW`, `htpasswd -D f user`, `scp user@host:/path .`,
  `git log -p`, `grep -p foo`, UUIDs, Git-Hashes, `SKU-12345`.
- Der Redactor wirkt nur auf Ausgaben, Log- und KI-Kontext. Die
  Filter-Engine sieht das Originalkommando; Nutzer und Oberfläche sehen es
  weiterhin unverändert.
- Die Laufzeit bleibt linear in der Eingabelänge.

## 5. Grenzen

Bewusst nicht erkannt:

- `user:pw@host` ohne Schema (zu nah an harmlosen Formen wie `scp`-Zielen,
  IPv6, Zeitangaben).
- `echo <pw> | sudo -S …`: das Geheimnis ist im Text nicht als solches
  erkennbar.
- `mysql -p <wert>` und `--password <wert>` mit Leerzeichen sowie
  `ssh -p <port>`.
- Datenbank-Strings mit rohem `,`, `;` oder `"` im Passwort (bräuchte
  Prozent-Dekodierung oder URL-Parsing).
- Solaris-Crypt-Formen (`$sha1$`, `$md5$`).
- Ein generischer Hoch-Entropie-Fallback.

## 6. Abnahmefälle

Jeder Fall läuft über den Redactor; Geheimnis `Geheim-0095`.

- **T1 mysql-Familie:** `mysql -pGeheim-0095 -u root`, `mysql -p'Geheim-0095'`,
  `mysql -p"Geheim 0095"`, `mysqldump -u r -pGeheim-0095 db`,
  `mariadb -pGeheim-0095`, `/usr/bin/mysql -pGeheim-0095`,
  `sudo -u x mysql -pGeheim-0095`, `MYSQL -pGeheim-0095` → kein Treffer;
  `mysql`, `-u root` und `db` bleiben.
- **T2 sshpass:** `-p Geheim-0095` und `-pGeheim-0095` → kein Treffer;
  `ssh host` bleibt.
- **T3 curl:** `curl -u admin:Geheim-0095 https://x`, `--user`, `-sSu`,
  `-uadmin:…`, `-u 'admin:Geheim 0095'` → kein Treffer; `admin` bleibt.
- **T4 htpasswd:** `htpasswd -b f user Geheim-0095`,
  `htpasswd -nbB user Geheim-0095 > out`, `htpasswd -bB -C 12 f user Geheim-0095`
  → kein Treffer; `user`, `out`, `12` bleiben.
- **T5 redis/smb/openssl:** `redis-cli -a …`, `smbclient -U user%…`,
  `openssl enc -pass pass:…`, `openssl rsa -passin pass:…`,
  `openssl enc -k …`, `sshpass -p 'Geheim 0095' ssh h` → kein Treffer.
- **T6 Verkettung:** hinter `&&`, in `$(…)`, nach `env LANG=C`, mit Tab oder
  mehreren Leerzeichen → kein Treffer.
- **T7 Tokens:** Twilio-SID, SendGrid-Key, `AccountKey=`, `SharedAccessKey=` →
  kein Treffer; `AccountName=` bleibt.
- **T8 Hashes:** Argon2id-, Argon2i-, phpass-`$P$`- und `$H$`-Hash in einer
  `/etc/shadow`-artigen Zeile → Hash ersetzt, Benutzername bleibt.
- **T9 JWT:** ein JWT ohne Präfix in einer Textzeile und in einem JSON-Wert →
  ersetzt.
- **T10 Ausgabe:** dieselben Formen in stdout und stderr eines
  Kommando-Ergebnisses → kein Treffer.
- **T11 Anker-Diebstahl:** Kombinationen mehrerer Geheimnisse in einer Zeile
  (siehe §4) → kein Geheimnis im Ergebnis.
- **T12 Keine Fehlalarme:** die Beispiele aus A5 und weitere harmlose
  Formen (`mysql -P 3306 -h db`, `mysql --password mydb`,
  `openssl enc -pass file:/k`, ein Git-Hash, `eyJ` allein) → unverändert.
- **T15 Bereits erkannte Formen:** je ein Fall je Eintrag aus §3 → kein
  Treffer.
