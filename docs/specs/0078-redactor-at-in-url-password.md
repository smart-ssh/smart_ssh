# Spec 0078 — Redactor: `@` in Zugangsdaten einer URL

Status: umgesetzt
Zweck: Zugangsdaten in URLs werden auch dann vollständig redigiert, wenn Passwort, Benutzername oder ein Passwort-Parameter im Query-String ein unkodiertes `@` enthält.
Bezüge: Spec 0006 (Redaction), Spec 0068, Spec 0095 (weitere Redactor-Formen), ADR 0069 (Positionen der Regeln, Messungen, Restfälle).

## 1. Verhalten

Der Redactor ersetzt in einer URL des Aufbaus `schema://benutzer:passwort@host`
das Passwort durch den Platzhalter `[REDACTED]`; der Benutzername bleibt
sichtbar. Das gilt auch in folgenden Fällen, die ein Muster nach dem Schema
„das Passwort endet am ersten `@`" nicht vollständig fassen würde:

- **A — `@` im Passwort.** Das Passwort reicht bis zum letzten `@` vor dem
  Host: `postgres://app:Xy9@kLm2@db.internal/prod` →
  `postgres://app:[REDACTED]@db.internal/prod`. Das gilt für alle
  Schemata (`mysql`, `mongodb+srv`, `amqps`, `redis`, `https`, `ftp` …).
- **B — `@` im Benutzernamen.** Die Form `benutzer@server` ist bei einigen
  gehosteten Datenbanken die vorgeschriebene Schreibweise:
  `postgres://svc@tenant:pw123@db/x` → `postgres://svc@tenant:[REDACTED]@db/x`.
- **C — Passwort-Parameter mit `@` im Query-String**, auch hinter einem Host
  ohne Pfad: `redis://cache:6379?password=p@ssw0rd` →
  `redis://cache:6379?[REDACTED]`. Als Schlüsselwörter gelten `password`,
  `token`, `api_key`, `secret` und `passphrase`. Der Wert endet an `&`, `#`,
  Leerraum, `,`, `;` oder einem Anführungszeichen; ein Wert in `'…'` oder
  `"…"` wird als Ganzes genommen. Der Trenner (`?`, davor stehende
  Parameter und `&`) bleibt stehen, ersetzt wird nur `schlüsselwort=wert`.
  Ein `&` ohne vorangehendes `?` im selben Token zählt nicht als Trenner.

Die prozentkodierte Form (`Xy9%40kLm2`) wird ebenfalls vollständig redigiert.

## 2. Stoppzeichen

- Für das Passwort einer URL (A, B) gelten `/`, `?`, `#`, Leerraum, `,`, `;`
  und `"` als Ende. Ohne `?` und `#` liefe die Regel über einen
  Query-String mit `@` hinweg und verschluckte den Host:
  `postgres://app:pw@db?sslmode=require&user=x@y` →
  `postgres://app:[REDACTED]@db?sslmode=require&user=x@y`.
- Folgt auf eine URL ein weiteres `@` (E-Mail-Adresse, `ssh://git@github.com:22/x`,
  `git@github.com:org/repo.git`), bleibt es unverändert.
- `,`, `;` und `"` bleiben bewusst Stoppzeichen (siehe §5).

## 3. Reihenfolge der Regeln

Die Regeln des Redactors laufen nacheinander über den schon ersetzten Text;
eine frühe Regel kann einer späteren den Anker nehmen. Die Reihenfolge ist
deshalb Teil des Verhaltens:

- Die Behandlung von Passwort-Parametern im Query-String (Fall C) läuft vor
  der allgemeinen Datenbank-URL-Regel, sonst liest diese `6379?password=p` als
  Passwort und nimmt dem Schlüsselwort-Regel den Anker. Sie läuft zweimal
  hintereinander, damit auch zwei `@`-haltige Parameter im selben Token
  erfasst werden (`redis://cache:6379?password=p@ss&token=abc@SECRETTAIL` →
  `redis://cache:6379?[REDACTED]`).
- Sie verlangt ein `@` im Wert. Ohne diese Bedingung zerschnitte sie den
  mehrteiligen Anker der Private-Key-Muster (`?secret=-----BEGIN PRIVATE KEY-----`),
  und der Schlüsselkörper bliebe im Klartext. Zusätzlich stehen die
  Private-Key- und PGP-Muster als Kopie am Listenanfang, damit ein
  `@` vor dem Anker (`?secret=a@-----BEGIN …`) den Schlüssel nicht freilegt.
- Die Behandlung von `@` in Benutzer und Passwort (A, B) läuft als letzte
  Regel. Früher platziert nähme sie dem Schlüsselwort-Muster den Anker:
  `https://u:x@h:1|password=ab@cdSecret` →
  `https://u:[REDACTED]@h:1|[REDACTED]`.

## 4. Sicherheitszusagen

- Es wird nie weniger redigiert als vor dieser Spec, mit der einen Ausnahme
  in §5 („Passwort mit Query-Präfix"). Jede geänderte Ausgabe redigiert
  mehr, keine weniger.
- Nichts leakt durch die bekannte Überredaktion (§5).
- Die Regeln sind auf katastrophales Zurückverfolgen geprüft: eine URL mit
  10 000 `@` im Passwort wird in Sekunden verarbeitet.

## 5. Grenzen (bekannte Restfälle)

- **Passwort mit `@` und `/`, `?` oder `#`** wird nur teilweise redigiert:
  `postgres://app:a@b?c@db/x` → `postgres://app:[REDACTED]@b?c@db/x`.
  Dasselbe gilt wie bisher für `,`, `;` und `"`.
- **Überredaktion (nichts leakt):** Folgt einer URL ohne Stoppzeichen ein
  weiteres `@` (etwa hinter `|` oder `&`), wird der Teil dazwischen mit
  geschwärzt: `ssh://git@host:2222|deploy@server` →
  `ssh://git@host:[REDACTED]@server`; `https://u:p@h:1|user=me@mail` →
  `https://u:[REDACTED]@mail`.
- **Passwort mit Query-Präfix** — die eine Stelle, an der weniger redigiert
  wird als vor dieser Spec: Enthält das Passwort einer Datenbank-URL wörtlich
  `?<schlüsselwort>=` und danach ein `@`, redigiert die Parameter-Regel ab dem
  Schlüsselwort, und der Passwort-Präfix davor bleibt sichtbar:
  `postgres://u:a?password=b@h/x` → `postgres://u:a?[REDACTED]`; mit
  `&`-Kette bleibt auch diese stehen (`postgres://u:Secret1?x=Secret2&token=b@h/db`
  → `postgres://u:Secret1?x=Secret2&[REDACTED]`). Die Zeichenkette ist ohne
  echtes URL-Parsing zweideutig; sie zu lösen hieße, Fall C wieder zu öffnen.
  Betroffen sind nur Datenbank-Schemata.
- **Unverändert gegenüber dem Stand davor:** Ein Schlüsselwort-Parameter
  ohne `?` im selben Token (`redis://cache:6379&password=p@ssw0rd` →
  `redis://cache:[REDACTED]@ssw0rd`) und die `#`-Form; ab dem dritten
  `@`-haltigen Schlüsselwort-Parameter im selben Token.
- Zugangsdaten außerhalb von URLs (etwa `curl -u me:pw`) sind Gegenstand von
  Spec 0095.

## 6. Beispiele

Jede Ausgabe ist gemessen. Die Kennungen werden von Tests und Kommentaren
zitiert.

### 6.1 Neu redigiert

| Kennung | Eingabe | Ausgabe |
|---|---|---|
| T-1 | `postgres://app:Xy9@kLm2@db.internal/prod` | `postgres://app:[REDACTED]@db.internal/prod` |
| T-2a | `mysql://root:a@b@c@db:3306/x` | `mysql://root:[REDACTED]@db:3306/x` |
| T-2b | `mongodb+srv://u:p@ss@cluster0.example.net/db` | `mongodb+srv://u:[REDACTED]@cluster0.example.net/db` |
| T-2c | `amqps://guest:g@st@mq:5671` | `amqps://guest:[REDACTED]@mq:5671` |
| T-2d | `redis://:p@ss@cache:6379/0` | `redis://:[REDACTED]@cache:6379/0` |
| T-2e | `https://deploy:t0k@n@git.example.com/repo.git` | `https://deploy:[REDACTED]@git.example.com/repo.git` |
| T-2f | `ftp://anon:mail@example.com@ftp.example.org/pub` | `ftp://anon:[REDACTED]@ftp.example.org/pub` |
| T-3 | `DATABASE_URL=postgres://app:Xy9@kLm2@db.internal/prod` | `DATABASE_URL=postgres://app:[REDACTED]@db.internal/prod` |
| T-4 | `https://u:a@b@host:8443 and https://v:c@d@other` | `https://u:[REDACTED]@host:8443 and https://v:[REDACTED]@other` |
| T-5a | `postgres://svc@tenant:pw123@db/x` | `postgres://svc@tenant:[REDACTED]@db/x` |
| T-5b | `https://user@corp:pw123@host/x` | `https://user@corp:[REDACTED]@host/x` |
| T-5c | `https://user@corp:p@ss@host/x` | `https://user@corp:[REDACTED]@host/x` |
| T-6 | `redis://cache:6379?password=p@ssw0rd` | `redis://cache:6379?[REDACTED]` |
| T-7a | `redis://cache:6379?password='p@ss w0rd'` | `redis://cache:6379?[REDACTED]` |
| T-7b | `redis://cache:6379?password='p@ss&w0rd'&db=1` | `redis://cache:6379?[REDACTED]` |

### 6.2 Gegenproben und Positionen

- **T-A2** `https://u:p@host?next=a@b` → `https://u:[REDACTED]@host?next=a@b`;
  ebenso `https://u:p@host/path?x=a@b.com`.
- **T-A3** `postgres://app:pw@db?sslmode=require&user=x@y` →
  `postgres://app:[REDACTED]@db?sslmode=require&user=x@y`.
- **T-A4** `postgres://app:pw@db1/x admin@example.com`: die E-Mail-Adresse
  bleibt.
- **T-A5** Unverändert bleiben: `see https://example.com/@user and mailto:a@b`,
  `ssh://git@github.com:22/x`, `git@github.com:org/repo.git and a@b`,
  `https://example.com:8080/path a@b`,
  `{"redis":"redis://cache:6379","admin":"ops@example.com"}`,
  `https://x.com/?q=password&token=` (leerer Wert).
- **T-A6** `mongodb://u:p@h1:27017,h2@x:27017/db` →
  `mongodb://u:[REDACTED]@h1:27017,h2@x:27017/db` (kein Lauf über das Komma).
- **T-A7** `postgres://app:Xy9%40kLm2@db.internal/prod` →
  `postgres://app:[REDACTED]@db.internal/prod`.
- **T-A8** `redis://cache:6379,password=p@ssw0rd` → `redis://cache:6379,[REDACTED]`.
- **T-A9** Schon vorher geschlossene Fälle bleiben es:
  `redis://cache:6379/0?password=p@ssw0rd&db=1` → `redis://cache:6379/0?[REDACTED]`;
  `postgres://db:5432/x?sslmode=require&password=p@ss;w` →
  `postgres://db:5432/x?sslmode=require&[REDACTED]`;
  `https://api.example.com/v1?token=abc@def&x=1` → `https://api.example.com/v1?[REDACTED]`.
- **T-A10** Eine URL mit 10 000 `@` im Passwort → `https://u:[REDACTED]@host/x`,
  innerhalb von 10 s.
- **T-A11 (Position der letzten Regel)** `https://u:x@h:1|password=ab@cdSecret` →
  `https://u:[REDACTED]@h:1|[REDACTED]`; ebenso mit `'` und `&` statt `|`.
- **T-A12 (Position der Query-Regel)** T-6 ist der Positionstest: Stünde die
  Regel hinter der Datenbank-URL-Regel, bliebe `ssw0rd` stehen.
- **T-A13** Ein Passwort wie in T-5a (`pw123`) steht weder in der
  gespeicherten Kommando-Ausgabe noch im Request an die KI.
- **T-A14** `https://x/?password='top secret 123'` → `https://x/?[REDACTED]`,
  ebenso mit `"…"`; `https://x/?password='unterminated p@ss` →
  `https://x/?[REDACTED] p@ss`.

### 6.3 Restfälle

- **T-R1** `postgres://app:a@b?c@db/x` → `postgres://app:[REDACTED]@b?c@db/x`.
- **T-R2** `ssh://git@host:2222|deploy@server` → `ssh://git@host:[REDACTED]@server`;
  `https://u:p@h:1|user=me@mail` → `https://u:[REDACTED]@mail`.
- **T-R3** `postgres://u:a?password=b@h/x` → `postgres://u:a?[REDACTED]`.
