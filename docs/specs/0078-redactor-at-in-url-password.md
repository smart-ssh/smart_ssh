# Spec 0078 — Redactor: `@` in Zugangsdaten einer URL

Status: **freigegeben** (Stefan, 2026-09-24) · Backlog: BL-0248 · Gate: release-1.0/C
Repo: **öffentlich** `smart-ssh` — `crates/core/src/ai/redactor.rs`,
`crates/core/src/ai/tests.rs`, `crates/app-shell/src/orchestration.rs`
(nur Test, T-A13)
Review-Priorität: **ERHÖHT** (Redactor; adversariale Fälle in §6.2)
Zweck: Zugangsdaten in URLs werden auch dann vollständig redigiert, wenn
Passwort, Benutzername oder ein Passwort-Parameter im Query-String ein
unkodiertes `@` enthält.

## 1. Ausgangslage

Die Regeln des Redactors laufen **nacheinander** über den schon ersetzten
Text (`redact_bytes`, `redactor.rs:579-587`). Danach läuft einmal
`absorb_fragments_next_to_placeholders` (`:601`). Für URLs relevant sind:

- DB-Muster: `redactor.rs:334-340`, Kommentarblock davor ab `:264`.
  Passwort `[^@/\s,;"]+@`, **ohne** `?` und `#` als Stoppzeichen.
- strenges URL-Muster (Spec 0068): `:356-362`, Passwort `[^@/\s,;"?#]+@`
- Schlüsselwort-Muster: `:450-452`, `(password|token|api_key|secret|passphrase)… [:=] …`
- breites URL-Muster am Ende: `:543-554`

In allen URL-Mustern schließen Benutzer- und Passwortklasse `@` aus.

**Gemessen** (2026-09-24, `DefaultOutputRedactor::redact_text` auf dem
heutigen Stand, Mess-Crate außerhalb des Repos). Drei Arten von Lücken:

| # | Eingabe | heute |
|---|---|---|
| A | `postgres://app:Xy9@kLm2@db.internal/prod` | `postgres://app:[REDACTED]@kLm2@db.internal/prod` |
| A | `mysql://root:a@b@c@db:3306/x` | `mysql://root:[REDACTED]@b@c@db:3306/x` |
| A | `https://deploy:t0k@n@git.example.com/repo.git` | `https://deploy:[REDACTED]@n@git.example.com/repo.git` |
| B | `postgres://svc@tenant:pw123@db/x` | **unverändert**, `pw123` im Klartext |
| B | `https://user@corp:pw123@host/x` | **unverändert** |
| C | `redis://cache:6379?password=p@ssw0rd` | `redis://cache:[REDACTED]@ssw0rd` |

- **A, `@` im Passwort:** Das Passwort endet am ersten `@`, und der Rest
  steht im Klartext. Dasselbe gilt für `mongodb+srv`, `amqps`, `ftp` und
  `redis://:p@ss@…`.
- **B, `@` im Benutzernamen:** Keines der Muster greift, das Passwort
  steht **vollständig** im Klartext. Die Form `benutzer@server` ist bei
  einigen gehosteten Datenbanken die vorgeschriebene Schreibweise des
  Benutzernamens.
- **C, Passwort-Parameter mit `@` hinter einem Host ohne Pfad:** Das
  DB-Muster läuft vor dem Schlüsselwort-Muster. Weil es `?` nicht
  ausschließt, liest es `cache` als Benutzer und `6379?password=p` als
  Passwort. Damit nimmt es dem Schlüsselwort-Muster den Anker. Dieser Fall
  hat das Item ausgelöst. Mit Pfad (`redis://cache:6379/0?password=…`)
  stoppt das DB-Muster an `/`, und der Fall ist heute schon zu.

Die prozentkodierte Form (`Xy9%40kLm2`) wird heute schon vollständig
redigiert. Die Stoppzeichen `,` `;` `"` sind bewusst gesetzt
(`redactor.rs:305-333`, Tests `tests.rs:698`, `:718`, `:806`) und bleiben.

## 2. Ziel und Nicht-Ziele

**Ziel:** Die Fälle A, B und C werden vollständig redigiert. Außer den in
§5 genannten Restfällen wird keine heute redigierte Eingabe weniger
redigiert.

**Nicht-Ziele:**
- Alle bestehenden Muster bleiben **wörtlich** und an ihrer Stelle. Das
  gilt auch für das DB-Muster ohne `?`/`#`. Die Lösung ergänzt nur (§3).
- Die bekannten Restfälle mit `,` `;` `"` bleiben (`tests.rs:806`).
- Keine Behandlung von Prozentkodierung (nicht betroffen).
- Kein Umbau von `redact_bytes` oder `absorb_fragments_next_to_placeholders`.
- Zugangsdaten außerhalb von URLs (etwa `curl -u me:pw`) sind nicht Teil
  dieser Spec.

## 3. Anforderungen

Zwei **zusätzliche** Regeln. Ihre Position in der Liste ist Teil der
Anforderung und durch Tests gebunden (§6.2, T-A11/T-A12).

- **3.1 Regel N1, Passwort-Parameter im Query-String (Fall C).** Sie wird
  **vor dem Kommentarblock des DB-Musters** eingefügt (`redactor.rs:264`):

  ```
  (?i)(?P<sep>\?(?:[^\s,;"'#?@&]*&)*)(?P<key>password|token|api_key|secret|passphrase)=(?:'[^'\r\n]*@[^'\r\n]*'|"[^"\r\n]*@[^"\r\n]*"|[^&#\s,;"']*@[^&#\s,;"']*)
  ```

  Ersetzung: `${sep}${key}=[REDACTED]`. Die Schlüsselwörter sind genau die
  des Schlüsselwort-Musters (`:451`). Beim Wert unterscheidet N1 wie das
  Schlüsselwort-Muster drei Fälle. Ein Wert in `'…'` oder `"…"` wird als
  Ganzes genommen. Ohne Anführungszeichen endet er an `&` `#`,
  Leerraum, `,` `;` `"` `'`. N1 läuft vor dem DB-Muster, damit dieses
  den Parameter nicht mehr als Passwortteil lesen kann.

  Der Trenner ist ein `?`, danach beliebig viele weitere Parameter mit
  `&`. Ein `&` **ohne** vorangehendes `?` im selben Token zählt nicht
  (Klarstellung, s. §9, Q-BL-0248-02).

  N1 steht **zweimal hintereinander** in der Liste, wörtlich gleich
  (Klarstellung, s. §9, Q-BL-0248-03).

  **Jeder der drei Zweige verlangt mindestens ein `@` im Wert**
  (Klarstellung, s. §9, Q-BL-0248-01). Ohne diese Forderung zerschneidet
  N1 den mehrteiligen Anker der Schlüsselmuster —
  `?secret=-----BEGIN PRIVATE KEY-----` wird bis zum Leerzeichen ersetzt
  (freier Zweig), mit `"…"` bis zum schließenden Quote. Der komplette
  Schlüsselkörper steht danach im Klartext (gemessen).

  Die Forderung schützt den Anker aber **nicht** — das `@` darf vor ihm
  im Wert stehen (`?secret=a@-----BEGIN …`). Geschützt wird er durch die
  Kopien der Schlüsselmuster am Listenanfang (§9, Q-BL-0248-02). Ein
  früher hier stehender Satz („ein Anker ohne `@` ist für N1
  unerreichbar") war falsch und ist gestrichen.

  Die Forderung kostet keine Abdeckung, aber der Grund ist enger als er
  aussieht: Das DB-Muster **läuft** sehr wohl über einen Wert ohne `@`
  hinweg (seine Passwortklasse endet erst am ersten `@`, das irgendwo
  dahinter stehen darf) — alles, worüber es läuft, liegt jedoch innerhalb
  seiner eigenen Ersetzung und ist damit redigiert. Ein Teil des
  Parameterwerts kann nur dann hinter dem `@` stehenbleiben, wenn der Wert
  das `@` selbst enthält, und dann greift N1. Ein Wert ohne `@` wird
  unverändert vom Schlüsselwort-Muster redigiert.

  **Warum die Anführungszeichen:** Ohne sie griffe N1 bei
  `redis://cache:6379?password='p@ss w0rd'` gar nicht — die freie
  Wertklasse schließt `'` aus. Das DB-Muster läse dann
  `6379?password='p` als Passwort, und `ss w0rd'` bliebe im Klartext
  stehen, wo es heute redigiert wird (gemessen).
- **3.2 Regel N2, `@` in Benutzer und Passwort (Fälle A und B).** Sie
  wird **als letzte Regel** der Liste eingefügt, also hinter dem breiten
  URL-Muster (`:543-554`) und vor dem Absorb-Schritt, der ohnehin danach
  läuft:

  ```
  (?i)(?P<scheme>\b[a-z][a-z0-9+.-]*)://(?P<user>[^:/\s,;"?#]*):[^/\s,;"?#]+@
  ```

  Ersetzung: `${scheme}://${user}:[REDACTED]@`. Das ist das strenge
  Muster (`:358`) mit genau einem Unterschied: `@` fehlt in beiden
  ausgeschlossenen Klassen. Der Benutzer endet am ersten `:`. Das
  Passwort reicht gierig bis zum **letzten** `@` vor einem Stoppzeichen.
  Der Benutzername bleibt sichtbar, wie bei den bestehenden Mustern.
  **Warum am Ende:** Am Anfang der Liste schluckt N2 ein späteres
  Schlüsselwort samt Anker. Gemessen:
  `https://u:x@h:1&password=ab@cdSecret` → `https://u:[REDACTED]@cdSecret`,
  `cdSecret` im Klartext. Am Ende hat das Schlüsselwort-Muster den
  Parameter schon ersetzt, und N2 findet kein `@` mehr dahinter. Dasselbe
  gilt mit `|` und `'` statt `&`.
- **3.3** `?` und `#` bleiben in N2 Stoppzeichen. Ohne sie läuft N2 über
  einen Query-String mit `@` hinweg und schluckt den Host. Gemessen an
  einer Variante ohne diese Stoppzeichen:
  `postgres://app:pw@db?sslmode=require&user=x@y` → `postgres://app:[REDACTED]@y`.
- **3.4** Kommentare an N1 und N2 nennen den Grund ihrer Position und die
  Restfälle aus §5, im Stil von `redactor.rs:318-333`.

## 4. Design

- **Warum ergänzen statt ändern:** Konvention des Repos und des Redactors
  ist „alte Prüfung wörtlich behalten, neue dazu". Weil die Regeln
  nacheinander laufen, genügt „nur ergänzt" allein **nicht** als Beweis.
  Eine neue Regel kann einer späteren den Anker nehmen (so geschehen, siehe
  `redactor.rs:482-488`, und oben bei N2 am Anfang). Deshalb stützt sich
  die Invariante auf die Messung in §6 und auf Tests an **beiden** Seiten
  jeder Position, nicht auf die Konstruktion.
- **Warum N1 statt `?`/`#` im DB-Muster auszuschließen:** Das würde ein
  geprüftes Muster ändern. N1 löst Fall C ohne Eingriff und hält das
  DB-Muster wörtlich.
- **Verworfen:** N2 vor dem DB-Muster (Anker-Diebstahl, 3.2). Eine
  DB-Variante von N2 ohne `?`/`#` als Stoppzeichen (3.3).

## 5. Sicherheits-Invarianten

- **Nie weniger redigiert:** An 42 Fällen gemessen, darunter alle
  Gegenproben aus §6.2. Jede geänderte Ausgabe redigiert mehr, keine
  weniger. Die Tests in §6 binden das, auch an den Positionen.
- **Überredaktion, bekannt und hingenommen (nichts leakt):** N2 reicht
  bis zum letzten `@` vor einem Stoppzeichen. Folgt einer URL ohne
  Stoppzeichen ein weiteres `@` (etwa hinter `|` oder `&`), wird der Teil
  dazwischen mit geschwärzt. Das gilt auf der Benutzerseite und auf der
  Passwortseite. Gemessen:
  `ssh://git@host:2222|deploy@server` → `ssh://git@host:[REDACTED]@server`
  und `https://u:p@h:1|user=me@mail` → `https://u:[REDACTED]@mail`.
  Dieselbe Art Nebenwirkung wie die bekannte mit `|`/`&`
  (`redactor.rs:330-333`).
- **Restfall, bleibt:** Ein Passwort, das `@` **und** eines der Zeichen
  `/ ? #` enthält, wird weiter nur teilweise redigiert. Gemessen:
  `postgres://app:a@b?c@db/x` → `postgres://app:[REDACTED]@b?c@db/x`.
  Dasselbe gilt wie bisher für `,` `;` `"`. Test T-R1 hält das fest.
- **Restfall, neu durch N1 (Entscheidung Stefan, 2026-09-24,
  Q-BL-0248-01):** Enthält das Passwort einer Verbindungs-URL wörtlich
  `?<schlüsselwort>=` mit einem der Schlüsselwörter aus §3.1 und danach
  ein `@`, so redigiert N1 ab dem Schlüsselwort, und der Passwort-Präfix
  **vor** dem `?` bleibt im Klartext. Gemessen:
  `postgres://u:a?password=b@h/x` → vorher `postgres://u:[REDACTED]@h/x`,
  jetzt `postgres://u:a?[REDACTED]`. Der Präfix kann beliebig lang sein.

  Das ist die **eine** Stelle, an der weniger redigiert wird als vorher —
  daher die Einschränkung in §2. Ursache ist die Position von N1 vor dem
  DB-Muster, also dieselbe Position, die Fall C löst: Die Zeichenkette hat
  zwei Lesarten (Passwort `a?password=b` mit Host `h`, oder Passwort `a`
  mit Query-String), und ohne echtes URL-Parsing kann kein Muster sie
  unterscheiden. Vorher war die erste Lesart zu und die zweite offen,
  jetzt umgekehrt. Betroffen sind nur DB-Schemata. Test T-R3 hält es fest.

  Nicht betroffen ist der verwandte Fall **ohne** `@` im Parameterwert
  (`postgres://u:SuperSecret123?password=pl/ain&x=y@h/db`): der Präfix
  bleibt dort sichtbar, aber genauso wie vor dieser Spec — hier ändert
  sich nichts.
- **Restfälle, unverändert gegenüber dem Stand vor dieser Spec**
  (Q-BL-0248-03, gemessen; kein Verstoß gegen §2, hier nur festgehalten,
  damit sie nicht unbemerkt kippen):
  - Ein Schlüsselwort-Parameter **ohne** vorangehendes `?` im selben
    Token: `redis://cache:6379&password=p@ssw0rd` →
    `redis://cache:[REDACTED]@ssw0rd`. N1 greift nicht, das DB-Muster
    läuft bis zum `@` im Wert. Zwischenzeitlich war der Fall zu, aber nur
    um den Preis der Lockerung bei `https://u:Geheim&token=b@h/x`.
  - Dasselbe mit `#` statt `&` (`…?db=1#password=p@ss`).
  - Ab dem **dritten** `@`-haltigen Schlüsselwort-Parameter im selben
    Token (§9, Q-BL-0248-03).

## 6. Tests

Alle erwarteten Ausgaben unten sind **gemessen** (Kandidatenregeln um den
echten Redactor herum: N1 vor, N2 nach `redact_text`). Die Regeln vor dem
DB-Muster (`:162-263`, Provider-Schlüssel, Header) haben dabei keine der
Eingaben verändert.

**Prüfweise:** jeder Fall mit `assert_eq` auf die **exakte** Ausgabe.
`assert_fully_redacted` (`tests.rs:843-850`) prüft nur Fenster aus 9
Zeichen und prüft bei kürzeren Geheimnissen nichts. Er reicht hier
deshalb nicht allein.

### 6.1 Neue Fälle (heute rot, Gegenbeweis Pflicht)

| Test | Eingabe | erwartet |
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

### 6.2 Adversarial: Gegenproben und Positionen (Ausgabe = heute)

- **T-A1** Alle bestehenden Redactor-Tests bleiben unverändert grün. Keine
  Assertion wird angepasst.
- **T-A2** `https://u:p@host?next=a@b` → `https://u:[REDACTED]@host?next=a@b`.
  Ebenso `https://u:p@host/path?x=a@b.com`. Scheitert, wenn `?` oder `/`
  in N2 fehlen.
- **T-A3** `postgres://app:pw@db?sslmode=require&user=x@y` →
  `postgres://app:[REDACTED]@db?sslmode=require&user=x@y` (3.3).
- **T-A4** `postgres://app:pw@db1/x admin@example.com`: Die E-Mail-Adresse
  bleibt.
- **T-A5** Unverändert bleiben: `see https://example.com/@user and mailto:a@b`,
  `ssh://git@github.com:22/x`, `git@github.com:org/repo.git and a@b`,
  `https://example.com:8080/path a@b`,
  `{"redis":"redis://cache:6379","admin":"ops@example.com"}` und
  `https://x.com/?q=password&token=` (N1 greift nicht bei leerem Wert).
- **T-A6** `mongodb://u:p@h1:27017,h2@x:27017/db` →
  `mongodb://u:[REDACTED]@h1:27017,h2@x:27017/db`. N2 läuft nicht über das
  Komma.
- **T-A7** `postgres://app:Xy9%40kLm2@db.internal/prod` →
  `postgres://app:[REDACTED]@db.internal/prod`.
- **T-A8** `redis://cache:6379,password=p@ssw0rd` →
  `redis://cache:6379,[REDACTED]` (bestehender Fall `tests.rs:698`, hier
  ausdrücklich mit N1 geprüft).
- **T-A9** Query-Parameter, die heute schon zu sind, bleiben es:
  `redis://cache:6379/0?password=p@ssw0rd&db=1` → `redis://cache:6379/0?[REDACTED]`,
  `postgres://db:5432/x?sslmode=require&password=p@ss;w` →
  `postgres://db:5432/x?sslmode=require&[REDACTED]`,
  `https://api.example.com/v1?token=abc@def&x=1` →
  `https://api.example.com/v1?[REDACTED]`.
- **T-A10** Lange Eingabe: eine URL mit 10 000 `@` im Passwort
  (`https://u:` + `a@` × 10 000 + `host/x`). Das Ergebnis ist
  `https://u:[REDACTED]@host/x`, der Test läuft mit großzügiger Zeitgrenze
  (10 s). Er sichert gegen katastrophales Rückverfolgen, falls jemand die
  Regex-Bibliothek tauscht.
- **T-A11 (Position N2)** `https://u:x@h:1|password=ab@cdSecret` →
  `https://u:[REDACTED]@h:1|[REDACTED]`, ebenso mit `'` und `&` statt `|`.
  Die Position belegen die Varianten mit `|` und `'`. Die `&`-Variante
  fängt schon N1 ab, sie bleibt als Gegenprobe. Scheitert, wenn N2 vor
  dem Schlüsselwort-Muster steht (gemessen: `cdSecret` bleibt dann
  stehen).
- **T-A14 (Anführungszeichen, N1)** Unverändert gegenüber heute:
  `https://x/?password='top secret 123'` → `https://x/?[REDACTED]`, ebenso
  mit `"…"`. `https://x/?password='unterminated p@ss` →
  `https://x/?[REDACTED] p@ss` (heutiges Verhalten des
  Schlüsselwort-Musters, nicht Gegenstand dieser Spec). Scheitert, wenn
  N1 einen Wert in Anführungszeichen anschneidet.
- **T-A12 (Position N1)** T-6 ist der Positionstest für N1. Steht N1 hinter
  dem DB-Muster, ist das DB-Muster schon gelaufen, und `ssw0rd` bleibt
  stehen. Der Coder belegt das im Bericht, indem er N1 testweise
  verschiebt und T-6 rot sieht.
- **T-A13** Persistenz: In `crates/app-shell/src/orchestration.rs` werden
  `test_persisted_command_result_contains_redacted_not_raw_secret` (um
  Z. 9374) und `test_send_to_ai_provider_is_redacted_without_altering_persisted_context`
  (um Z. 12188) um die Eingabe aus T-5a erweitert, oder daneben
  nachgebaut. `pw123` darf weder in der gespeicherten Ausgabe noch im
  Request an die KI vorkommen.

### 6.3 Restfälle festhalten

- **T-R1** `postgres://app:a@b?c@db/x` → `postgres://app:[REDACTED]@b?c@db/x`
  mit dem Kommentar „bekannter Restfall, Spec 0078 §5".
- **T-R2** `ssh://git@host:2222|deploy@server` →
  `ssh://git@host:[REDACTED]@server` und `https://u:p@h:1|user=me@mail` →
  `https://u:[REDACTED]@mail`, mit dem Kommentar „bekannte Überredaktion,
  Spec 0078 §5".
- **T-R3** `postgres://u:a?password=b@h/x` → `postgres://u:a?[REDACTED]`,
  mit dem Kommentar „bekannter Restfall, Spec 0078 §5" und dem Hinweis,
  dass hier als einziger Stelle weniger redigiert wird als vor dieser
  Spec. Kein Gegenbeweis möglich und keiner nötig: der Test hält eine
  bewusst getroffene Entscheidung fest, kein behobenes Verhalten.

## 7. Umsetzungsreihenfolge

Ein Coder-Lauf, **Opus** (Redactor).

1. Tests aus §6.1 anlegen und rot sehen (Gegenbeweis).
2. N1 und N2 an ihren Positionen einfügen, Tests grün, §6.2 und §6.3
   dazu. Positionsbelege nach T-A11/T-A12.
3. spec-reviewer ERHÖHT, ADR (Positionen, Restfälle, warum ergänzt statt
   geändert), Changelog-Fragment. Das Fragment beschreibt nur das
   Verhalten („Zugangsdaten mit `@` in Verbindungs-URLs werden vollständig
   geschwärzt"), ohne die Lücke auszubreiten.

Danach prüfe ich: regression-guard über die Range, silent-failure-hunter
im Diff-Modus, dann mein Gate.

**Veröffentlichung:** Das Item ist privat. Die Spec wird erst unmittelbar
vor dem Coder-Lauf in `docs/specs/` committet. Spec und Fix gehen in
**einem** Push hinaus. Vorher ist der ungepushte Stand anderer Items
gepusht.

## 8. Offene Punkte (K3, Stefan)

Keine. Die Messung hat zusätzlich zum Item drei Dinge ergeben: den
Benutzernamen mit `@` (B), den Positionsfehler von N2 und den Fall mit
Anführungszeichen bei N1. Alle drei sind eingearbeitet, und keiner
verlangt eine Produktentscheidung.

## 9. Klarstellungen

- **2026-09-24 · Q-BL-0248-01 · Entscheidung Stefan, Option 1.** Der
  Restfall „Passwort mit Query-Präfix" (`postgres://u:a?password=b@h/x`)
  wird als bekannter Restfall aufgenommen statt behoben. §2 ist
  entsprechend präzisiert („außer den in §5 genannten Restfällen"), §5 um
  den Fall erweitert, §6.3 um Test T-R3. Grund: Die Zeichenkette ist echt
  zweideutig; sie zu lösen hieße, N1s Position aufzugeben und damit Fall
  C wieder zu öffnen.
- **2026-09-24 · Q-BL-0248-01 · Entscheidung Stefan.** Die Wertklasse von
  N1 in §3.1 verlangt in **allen drei** Zweigen ein `@` im Wert. Die
  ursprünglich vorgeschriebene Fassung (`[^&#\s,;"']+` ohne `@`-Pflicht,
  Quote-Zweige ohne Einschränkung) zerschnitt den Anker der
  Private-Key-Muster, sodass ein kompletter privater Schlüssel im Klartext
  an den KI-Anbieter ging. Gemessen an beiden Ständen, in zwei
  Review-Runden gefunden (freier Zweig, dann Quote-Zweige).
- **2026-09-24 · Runde 1 des `spec-reviewer` · Korrektur an §3.1.** Die
  ursprüngliche Begründung der Anführungszeichen-Alternativen („ohne sie
  nahm N1 bei `?password='top secret 123'` nur `'top`") war sachlich
  falsch: die freie Wertklasse schließt `'` aus, N1 greift dort ohne die
  Alternativen gar nicht, und das Schlüsselwort-Muster redigiert wie
  bisher. Der Nutzen der Alternativen bleibt, der Grund ist ein anderer —
  §3.1 nennt jetzt den zutreffenden (`?password='p@ss w0rd'`).
- **2026-09-24 · Runde 1 des `spec-reviewer` · Status von T-A14.** T-A14
  ist ein **Wächter**, kein Gegenbeweis für die Quote-Alternativen: seine
  Fälle sind auch ohne sie grün. Der Gegenbeweis steckt in den
  Query-Parameter-Fällen aus §6.1 (T-7a/T-7b). Die Formulierung in §6.2
  („Scheitert, wenn N1 einen Wert in Anführungszeichen anschneidet")
  trifft so nicht zu.

- **2026-09-24 · Q-BL-0248-02 · K2:** N1 zerschnitt einen Schlüsselblock
  auch dann, wenn vor `-----BEGIN` ein `@` im Wert steht
  (`?secret=a@-----BEGIN PRIVATE KEY-----…`, frei, in Anführungszeichen,
  PGP, ohne END). Außerdem nahm N1 dem strengen URL-Muster den Anker, wenn
  das Passwort `&<schlüsselwort>=` enthält (`https://u:Geheim&token=b@h/x`).
  Beides wurde vor dieser Spec redigiert. Korrektur:
  - **(A)** Die vier Private-Key- und PGP-Muster kommen zusätzlich als
    Kopie an den **Anfang** der Liste. Die Originale bleiben wörtlich an
    ihrer Stelle.
  - **(B)** N1 zählt `&` nur nach einem `?` im selben Token:
    `(?i)(?P<sep>\?(?:[^\s,;"'#?@&]*&)*)(?P<key>password|token|api_key|secret|passphrase)=(?:'[^'\r\n]*@[^'\r\n]*'|"[^"\r\n]*@[^"\r\n]*"|[^&#\s,;"']*@[^&#\s,;"']*)`,
    Ersetzung `${sep}${key}=[REDACTED]`.

  Tests, jeweils heute rot:
  - die vier Schlüssel-Eingaben, vollständig geschwärzt,
  - `https://u:Geheim&token=b@h/x`, `ssh://u:Geheim&password=b@h/x` und
    `postgres://u:a&token=b@h/x`, jeweils → `…:[REDACTED]@h/x`,
  - `api-key: -----BEGIN PRIVATE KEY-----…`, vollständig geschwärzt (war
    schon vor dieser Spec offen und ist mit A zu),
  - `redis://cache:6379?db=1&password=p@ssw0rd` → `redis://cache:6379?db=1&[REDACTED]`
    (Gegenprobe, **nicht** rot gegen den Stand davor — sie ist erst rot
    gegen den Stand vor dieser Spec; nachgemessen).

  Der Restfall aus Q-BL-0248-01 ist unverändert und auf `?` beschränkt.

- **2026-09-24 · Q-BL-0248-03 · K2 (Fund `spec-reviewer`, Runde 3,
  nachgemessen):** Die Trennergruppe aus Q-BL-0248-02 schließt `@` aus
  und kann deshalb nicht über einen schon `@`-haltigen Parameter
  hinweglaufen; `replace_all` sucht nur vorwärts. Bei **zwei**
  `@`-haltigen Schlüsselwort-Parametern im selben Token erreichte N1
  deshalb nur den ersten, das DB-Muster fraß danach den Anker des
  zweiten, und dessen Schwanz stand im Klartext. Gemessen:
  `redis://cache:6379?password=p@ss&token=abc@SECRETTAIL` → vor dieser
  Spec `redis://cache:[REDACTED]@ss&[REDACTED]`, danach
  `redis://cache:[REDACTED]@SECRETTAIL`. Verstoß gegen §2.

  Korrektur: **N1 steht zweimal hintereinander in der Liste**, wörtlich
  gleich. Nach dem ersten Durchlauf steht am ersten Parameter
  `[REDACTED]` ohne `@`, und die Trennergruppe kommt daran vorbei.
  Ergebnis jetzt `redis://cache:6379?[REDACTED]`, also besser als vor
  dieser Spec. Eine zusätzliche Anwendung derselben Regel kann per
  Konstruktion nur mehr redigieren.

  Restfall: Drei und mehr `@`-haltige Schlüsselwort-Parameter im selben
  Token bräuchten je eine weitere Anwendung; ab dem dritten bleibt es
  beim Verhalten vor dieser Spec (§5). Eine Schleife wäre die saubere
  Form, hieße aber `redact_bytes` umzubauen — von §2 ausgeschlossen.

  Ebenfalls in dieser Runde festgehalten, **kein** Verstoß gegen §2, weil
  gleich dem Stand vor dieser Spec: die `&`-Form ohne `?`
  (`redis://cache:6379&password=p@ssw0rd` → `redis://cache:[REDACTED]@ssw0rd`)
  und die `#`-Form (`…?db=1#password=p@ss`). Beide in §5 aufgenommen.
