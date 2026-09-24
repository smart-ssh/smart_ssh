# ADR 0069: `@` in den Zugangsdaten einer URL — Entscheidungen bei der Umsetzung

Status: Angenommen
Bezug: docs/specs/0078-redactor-at-in-url-password.md (§2–§6), Spec 0068
(strenges URL-Muster, Reihenfolge der Muster mit Header-Namen), Spec 0006
§5 (Redaction vor Persistenz und vor jedem Provider-Aufruf),
CLAUDE.md („never loosen an existing check", „Escalation only goes one
direction")

Spec 0078 ergänzt den Redactor um zwei Regeln, damit Zugangsdaten einer
Verbindungs-URL auch dann vollständig geschwärzt werden, wenn Passwort,
Benutzername oder ein Passwort-Parameter im Query-String ein unkodiertes
`@` enthalten. Dieses ADR protokolliert, was dabei entschieden, abgewichen
und bewusst offen gelassen wurde.

## 1. Warum „nur ergänzt" hier kein Beweis ist

Die Regeln des Redactors laufen **nacheinander** über den schon ersetzten
Text (`redact_bytes`). Eine neue Regel kann deshalb einer späteren den
Anker nehmen und dadurch Klartext stehen lassen, wo vorher redigiert
wurde — obwohl kein bestehendes Muster angefasst wurde. Genau das ist im
Redactor schon zweimal passiert (s. die Kommentare an den Shadow-Hash-
Mustern und am DB-Muster) und in diesem Schritt ein drittes Mal (§4).

Die Invariante „nie weniger redigiert" stützt sich hier deshalb **nicht**
auf die Konstruktion, sondern auf Messung: jede erwartete Ausgabe aus
Spec §6 wurde gegen den echten Redactor geprüft, und für die
Positionsfragen gibt es Tests auf **beiden** Seiten jeder Position.

## 2. Position der Query-String-Regel: vor dem DB-Muster

Entschieden in der Spec (§3.1), hier belegt. Das DB-Muster schließt `?`
nicht aus und liest bei einer URL ohne Pfad
(`redis://cache:6379?password=p@ssw0rd`) `6379?password=p` als Passwort.
Damit nimmt es dem Schlüsselwort-Muster den Anker, und der Rest bliebe im
Klartext.

Beleg statt Behauptung: Die Regel wurde testweise hinter das DB-Muster
verschoben; `test_redactor_redacts_a_password_query_parameter_containing_
an_at_sign` wurde daraufhin rot, mit `ssw0rd` im Klartext. Danach
zurückverschoben.

Die Alternative — `?`/`#` als Stoppzeichen im DB-Muster nachtragen —
wurde **nicht** gewählt: das DB-Muster ist über vier Review-Runden gegen
echte Regressionen verengt worden, und es zu ändern hieße, ein geprüftes
Muster anzufassen, statt zu ergänzen (Spec §4).

## 3. Position der neuen URL-Regel: als letzte der Liste

Entschieden in der Spec (§3.2), hier belegt. Weiter vorn schluckt die
Regel ein späteres `password=` samt dessen Anker.

Beleg: Die Regel wurde testweise an den Anfang verschoben (vor das
DB-Muster); `test_redactor_does_not_let_the_at_url_rule_swallow_a_later_
password_keyword` wurde rot, `cdSecret` blieb im Klartext. Danach
zurückverschoben.

Als letzte Regel kann sie strukturell keiner späteren den Anker nehmen —
nach ihr läuft nur noch `absorb_fragments_next_to_placeholders`, und der
kann nur zusätzlich schwärzen, nie etwas freilegen. Die in Spec §5
dokumentierte Überredaktion („bis zum letzten `@`") kann deshalb kein
danebenstehendes Geheimnis unredigiert lassen.

## 4. Abweichung von Spec §3.1: die Query-String-Regel verlangt ein `@` im Wert

**Das ist die einzige Abweichung vom wörtlichen Text der Spec.**

Spec §3.1 schreibt für den unquotierten Wert die Klasse `[^&#\s,;"']+`
vor. In dieser Fassung zerschneidet die Regel den mehrteiligen Anker der
Private-Key-Muster, die weiter unten in der Liste stehen: Bei

```
https://vault.example/api?secret=-----BEGIN PRIVATE KEY-----
MIIEvQ…
-----END PRIVATE KEY-----
```

ersetzt sie `?secret=-----BEGIN` (die Klasse erlaubt `-`, stoppt aber an
Leerraum). Danach greift weder das PEM-Muster noch sein
Fail-safe-Rückfallmuster, und der **komplette Schlüsselkörper** steht im
Klartext — dort, wo er vor Spec 0078 vollständig redigiert wurde. Dasselbe
mit `-----BEGIN PGP PRIVATE KEY BLOCK-----`.

Fund des `spec-reviewer` (erste Runde, ERHÖHT), gegen den Vor- und den
Nachher-Stand nachgemessen statt übernommen. Eintrittswahrscheinlichkeit
niedrig, Schadenshöhe maximal.

Behoben, indem **jeder der drei Zweige** der Wertklasse mindestens ein
`@` verlangt. Die erste Nachbesserung verengte nur den freien Zweig; die
zweite Review-Runde zeigte, dass der quotierte Zweig denselben Anker
weiterhin zerschnitt (`?secret="-----BEGIN PRIVATE KEY-----"`, Körper
außerhalb der Quotes) — gemessen, gegen den Stand vor Spec 0078
vollständig redigiert, danach im Klartext. Erst mit der Forderung in allen
drei Zweigen gilt der Satz, der die Regel trägt:

> Diese Regel ersetzt ausschließlich Werte, die ein `@` enthalten. Ein
> Anker ohne `@` ist für sie unerreichbar.

Begründung, warum das keine Abdeckung kostet — enger gefasst als in der
ersten Fassung dieses ADR, weil der Reviewer die dortige Formulierung zu
Recht als falsch beanstandet hat: Das DB-Muster **läuft** sehr wohl über
einen Wert ohne `@` hinweg (seine Passwortklasse endet erst am ersten
`@`, das irgendwo dahinter stehen darf). Alles, worüber es läuft, liegt
aber **innerhalb seiner Ersetzung** und ist damit redigiert. Ein Teil des
Parameterwerts kann nur dann hinter dem `@` stehenbleiben, wenn der Wert
das `@` selbst enthält — und dann greift die Regel. Ein Wert ohne `@`
wird unverändert vom Schlüsselwort-Muster redigiert (Test
`…_still_redacts_a_query_parameter_without_an_at_sign`, ein Wächter, kein
Gegenbeweis: grün vor und nach der Verengung).

Gegenbeweis für beide Runden geführt: `…_does_not_cut_a_private_key_
armor_anchor` ist rot gegen die Spec-Fassung **und** rot gegen die
Fassung, die nur den freien Zweig verengt.

Der Vorschlag des Reviewers, stattdessen die PEM-/PGP-Muster an den Anfang
der Liste zu ziehen, wurde **nicht** umgesetzt: er verschiebt bestehende
Muster und widerspricht damit Spec §2 („wörtlich und an ihrer Stelle").
Er bleibt als eigenes Thema sinnvoll, weil er zugleich einen **bereits
bestehenden** Geschwisterfall bei `api-key: -----BEGIN …` schlösse.

## 5. Sachlich falsche Begründung in Spec §3.1, korrigiert

Spec §3.1 begründet die Anführungszeichen-Alternativen damit, die Regel
nähme bei `?password='top secret 123'` sonst nur `'top`. Das trifft nicht
zu: die freie Wertklasse schließt `'` und `"` aus, die Regel griffe dort
also gar nicht, und das Schlüsselwort-Muster redigiert wie bisher.

Die Alternativen sind trotzdem nötig, nur aus einem anderen Grund: ohne
sie griffe die Regel bei `redis://cache:6379?password='p@ss w0rd'` nicht,
das DB-Muster läse `6379?password='p` als Passwort, und `ss w0rd'` bliebe
im Klartext. Der Kommentar im Code nennt jetzt diesen Grund; der
Testkommentar zu T-A14 sagt außerdem, dass T-A14 ein Wächter ist und
nicht der Gegenbeweis für die Alternativen (der steht bei den
Query-Parameter-Fällen). Fund des `spec-reviewer`, erste Runde.

## 6. Bewusst nicht behoben: Passwort mit einem Query-Präfix

`postgres://u:a?password=b@h/x` wurde vor Spec 0078 zu
`postgres://u:[REDACTED]@h/x` und wird jetzt zu `postgres://u:a?[REDACTED]`
— der Passwort-Präfix vor dem `?` steht neu im Klartext.

Das ist eine echte Verringerung und widerspricht dem absoluten Wortlaut
von Spec §2. Es ist **nicht** behoben, weil es sich nicht beheben lässt,
ohne die Position der Query-String-Regel aufzugeben (und damit den
auslösenden Fall des Items wieder zu öffnen): Die Zeichenkette hat zwei
Lesarten — Passwort `a?password=b` mit Host `h`, oder Passwort `a` mit
Query-String —, und ohne echtes URL-Parsing kann kein Muster sie
unterscheiden. Vor Spec 0078 war die erste Lesart zu und die zweite offen,
jetzt umgekehrt.

Betroffen sind nur DB-Schemata und nur Passwörter, die wörtlich
`?password=`/`&token=`/… enthalten.

**Zweite Ausprägung derselben Familie** (zweite Review-Runde, gemessen):
Der Präfix bleibt auch dann sichtbar, wenn der Parameterwert **kein** `@`
enthält, sobald er ein `/` enthält und irgendwo dahinter ein `@` steht —
`postgres://u:SuperSecret123?password=pl/ain&x=y@h/db` →
`postgres://u:SuperSecret123?[REDACTED]`. Das DB-Muster stoppt am `/` im
Wert und findet das `@` dahinter nicht mehr. Der Präfix kann also ein
**vollständiges Passwort beliebiger Länge** sein, nicht nur ein Zeichen.

Wichtig für die Einordnung, gemessen an allen drei Ständen: Gegenüber dem
Stand **vor Spec 0078** ist dieses Verhalten **unverändert** (auch dort
bleibt `SuperSecret123` sichtbar). Spec §2 („keine *heute* redigierte
Eingabe wird weniger redigiert") ist damit gewahrt. Nur gegenüber der
ersten, zu breiten Fassung der Query-String-Regel — die den Fall
versehentlich mit schloss, um den Preis des Private-Key-Lecks aus §4 — ist
es ein Rückschritt. Diese Fassung war nie ein tragfähiger Bezugspunkt.

Der Punkt liegt als Frage **Q-BL-0248-01** zur Entscheidung vor
(Einstufung K3). Bis dahin ist er hier festgehalten, nicht stillschweigend
hingenommen. Ein Test dazu wird erst angelegt, wenn entschieden ist, ob
das Verhalten so bleiben soll — ein Test, der es jetzt festschreibt, würde
die Entscheidung vorwegnehmen.

## 7. Bekannte Restfälle und Überredaktion, die bleiben

Aus Spec §5, im Code kommentiert und durch Tests festgehalten
(`…_known_remaining_case_…`, `…_known_over_redaction_…`):

- Ein Passwort, das `@` **und** eines der Zeichen `/ ? #` enthält, wird
  weiter nur teilweise redigiert. Dieselben Zeichen müssen Stoppzeichen
  bleiben, sonst läuft die neue Regel über einen Query-String hinweg und
  schluckt den Host. Ebenso bleibt es bei `,` `;` `"` wie beim DB-Muster.
- Überredaktion: folgt einer URL ohne Stoppzeichen ein weiteres `@` (etwa
  hinter `|`), wird der Teil dazwischen mit geschwärzt. Dabei leakt
  nichts — dieselbe Art Nebenwirkung, die am DB-Muster schon dokumentiert
  ist.
- Ein leerer Parameterwert (`?token=` am Zeilenende) wird nicht erfasst;
  es gibt nichts zu redigieren.

## 8. Nebenbefund: eine Prüfung, die nicht scheitern konnte

`test_persisted_command_result_contains_redacted_not_raw_secret` prüfte
das Geheimnis nur gegen die JSON-Form der gespeicherten Nachricht.
`CommandOutput::stdout` ist ein `Vec<u8>` und serialisiert als
Zahlen-Array (`[86,101,…]`) — ein Geheimnis im Kommando-Output taucht dort
nie als lesbare Zeichenkette auf, die Prüfung **konnte** also gar nicht
anschlagen. Der Test prüft jetzt zusätzlich die dekodierten Bytes; in
dieser Fassung wird er ohne die neuen Regeln rot (belegt).

## 9. Aus dem Review übernommen, aber nicht in diesem Schritt

- **`Bearer <token>` hinter einem Schlüsselwort-Parameter**
  (`?token=Bearer abcdef…`): das Schlüsselwort-Muster verbraucht `Bearer`
  und stoppt am Leerzeichen, das Bearer-Muster steht danach ohne Anker da.
  Besteht unabhängig von Spec 0078 und wird davon nicht verändert.
- **Muster mit mehrteiligem Anker generell vor die wertverbrauchenden
  Muster ziehen** (PEM, PGP, netrc, `client-key-data`) — s. §4. Schlösse
  die Ursachenklasse statt einzelner Ausprägungen.
- **Unicode-Homoglyphen im Userinfo-Teil** (`＠` U+FF20): kein Muster
  greift. Nicht durch diesen Schritt verursacht, nicht Gegenstand der
  Spec.

## 10. Was der committete Spec-Text noch nicht sagt

Der Reviewer weist zu Recht darauf hin, dass die committete Spec (§3.1,
§6.2 T-A14) dem Code an drei Stellen widerspricht und §9
(„Klarstellungen") leer ist: die Wertklasse der Query-String-Regel (§4
hier), die Begründung der Anführungszeichen-Alternativen (§5 hier) und
der Status von T-A14 als Wächter.

Die Spec wird hier **bewusst nicht** nachgezogen: Die Abweichung der
Wertklasse und der Restfall aus §6 liegen als K3-Frage bei Stefan
(Q-BL-0248-01), und der Architekt hat die Spec-Änderung ausdrücklich bis
zu dieser Entscheidung zurückgestellt. Bis dahin ist dieses ADR die
Stelle, an der die drei Punkte stehen. Wird die Frage entschieden, gehören
sie in Spec §9.
