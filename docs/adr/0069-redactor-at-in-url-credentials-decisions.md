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

**Nachtrag (Q-BL-0248-02): die `@`-Forderung allein genügt nicht, und die
Begründung dazu war falsch.** Der `regression-guard` fand über
`ee017af..387a91e` eine dritte Ausprägung, vom Architekten vorher/nachher
gemessen: `?secret=a@-----BEGIN PRIVATE KEY-----`. Der Satz „ein Anker
ohne `@` ist für die Regel unerreichbar" stimmt nicht — nicht der **Anker**
muss das `@` enthalten, sondern der **Wert**, und der beginnt vor dem
Anker. Das `a@` erfüllt die Bedingung, der Anker liegt mitten im Treffer.
Dasselbe quotiert, als PGP-Block und abgeschnitten ohne `END`.

Damit ist die Ursache dort behoben, wo sie sitzt (§11): Kopien der vier
Schlüsselmuster laufen jetzt ganz am Anfang der Liste. Die `@`-Forderung
bleibt, aber sie trägt nicht mehr den Schutz des Ankers — sie begrenzt die
Regel nur noch auf ihren Zweck.

Der ursprüngliche Vorschlag des Reviewers aus Runde 1 war genau das, und
er wurde damals **nicht** umgesetzt, weil er bestehende Muster verschiebt
und Spec §2 widerspricht („wörtlich und an ihrer Stelle"). Die jetzige
Lösung hält beides ein: **Kopien** vorn, **Originale** unverändert an
ihrer Stelle. Rückblickend war die Ablehnung in Runde 1 zu eng — der
Reviewer hatte die Ursachenklasse richtig benannt, und zwei weitere
Ausprägungen sind danach noch aufgetreten.

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

## 6. Bewusst hingenommen: Passwort mit einem Query-Präfix

**Entschieden (Stefan, 2026-09-24, Q-BL-0248-01, Option 1):** Der Fall
bleibt als bekannter Restfall stehen. Spec §2 ist entsprechend präzisiert
(„außer den in §5 genannten Restfällen"), §5 um den Fall erweitert, §6.3
um Test T-R3.

`postgres://u:a?password=b@h/x` wurde vor Spec 0078 zu
`postgres://u:[REDACTED]@h/x` und wird jetzt zu `postgres://u:a?[REDACTED]`
— der Passwort-Präfix vor dem `?` steht neu im Klartext. Der Präfix kann
beliebig lang sein, also ein vollständiges Passwort.

**Das ist die einzige Stelle, an der Spec 0078 weniger redigiert als der
Stand davor.** Nicht behebbar, ohne die Position der Query-String-Regel
aufzugeben (und damit den auslösenden Fall des Items wieder zu öffnen):
Die Zeichenkette hat zwei Lesarten — Passwort `a?password=b` mit Host
`h`, oder Passwort `a` mit Query-String —, und ohne echtes URL-Parsing
kann kein Muster sie unterscheiden. Vor Spec 0078 war die erste Lesart zu
und die zweite offen, jetzt umgekehrt. Betroffen sind nur DB-Schemata und
nur Passwörter, die wörtlich `?password=`/`&token=`/… **mit einem `@` im
Wert** enthalten.

Geprüft und verworfen, um den Fall zu schließen: die Regel hinter das
DB-Muster (öffnet Fall C, belegt); ihre Wertklasse weiter verengen (hilft
nicht, der Wert erfüllt jede Verengung, die Fall C noch löst); den
Separator kontextabhängig prüfen (die `regex`-Crate kennt kein
Lookbehind). Auch `?`/`#` im DB-Muster als Stoppzeichen zu ergänzen löst
ihn nicht — dann greift dort gar kein URL-Muster mehr, und der Präfix
bleibt genauso sichtbar (Herleitung des Architekten in der Frage-Datei).

**Davon zu unterscheiden, weil es NICHT neu ist:** Ohne `@` im
Parameterwert bleibt der Präfix ebenfalls sichtbar, sobald der Wert ein
`/` enthält und dahinter irgendwo ein `@` steht —
`postgres://u:SuperSecret123?password=pl/ain&x=y@h/db` →
`postgres://u:SuperSecret123?[REDACTED]`, weil das DB-Muster am `/` im
Wert hängenbleibt. Gemessen an allen Ständen: **vor Spec 0078 identisch**.
Hier ändert sich nichts.

**Korrektur einer eigenen Fehlaussage.** Die erste Fassung dieses ADR und
die Kurzfassung des Coder-Berichts verallgemeinerten von diesem zweiten
Beispiel auf den ganzen Fall und behaupteten, gegenüber dem Stand vor Spec
0078 verschlechtere sich nichts. Das ist falsch — für das erste Beispiel
(`@` im Parameterwert) verschlechtert es sich sehr wohl. Der Architekt hat
das nachgemessen und die Aussage widerlegt; sie war aus einer Messung des
zweiten Beispiels hochgerechnet, statt das erste erneut zu messen. Genau
der Fehlertyp, gegen den die Messpflicht im Skill steht. T-R3 hält
deshalb **beide** Beispiele fest, damit sie nicht wieder verwechselt
werden.

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
- **Unicode-Homoglyphen im Userinfo-Teil** (`＠` U+FF20): kein Muster
  greift. Nicht durch diesen Schritt verursacht, nicht Gegenstand der
  Spec.
- **netrc und `client-key-data`** haben ebenfalls mehrteilige Anker und
  stehen weiterhin hinten. Sie sind hier nicht mit nach vorn gezogen, weil
  für sie keine Lockerung gemessen wurde und ihre Anker keine
  `=`-Trennung haben, über die eine Wert-Regel stolpert. Wenn die
  Ursachenklasse einmal vollständig geschlossen werden soll, gehören sie
  dazu — eigenes Item, eigene Messrunde.

## 10. Spec-Text nachgezogen

Der Reviewer hatte beanstandet, dass die committete Spec dem Code an drei
Stellen widersprach und §9 („Klarstellungen") leer war. Mit der
Entscheidung zu Q-BL-0248-01 ist das nachgezogen:

- §2 präzisiert („außer den in §5 genannten Restfällen"),
- §3.1 auf die bestätigte Wertklasse mit `@`-Pflicht in allen drei
  Zweigen, samt der korrigierten Begründung für die
  Anführungszeichen-Alternativen,
- §5 um den Restfall „Passwort mit Query-Präfix" (§6 hier),
- §6.3 um Test T-R3,
- §9 um vier Klarstellungen mit Datum und Frage-ID, darunter der Status
  von T-A14 als Wächter.

Dazu kam die Klarstellung zu Q-BL-0248-02 (§11).

Spec und Code decken sich damit wieder.

## 11. Schlüsselmuster zuerst, und `&` nur nach `?` (Q-BL-0248-02)

Der `regression-guard` fand über `ee017af..387a91e` zwei Lockerungen durch
die Query-String-Regel, beide vom Architekten vorher/nachher gemessen und
als K2 entschieden (Spec §9). Beide verletzten Spec §2.

**(A) Kopien der vier Schlüsselmuster an den Anfang der Liste.** Die
Originale bleiben wörtlich an ihrer Stelle; vorn steht nur eine
zusätzliche, frühere Anwendung. Damit ist ein Schlüsselblock geschwärzt,
bevor irgendeine wertverbrauchende Regel seinen Anker sieht.

Das schließt die Ursachenklasse statt einzelner Ausprägungen. Sie ist im
Redactor dreimal aufgetreten: bei der frühen `api-key`-Kopie (schon **vor**
Spec 0078 offen, `api-key: -----BEGIN …` ließ den Schlüsselkörper stehen)
und zweimal bei der Query-String-Regel. Der bestehende Fall ist als
Nebenwirkung mit zu — eine Verbesserung gegenüber dem Stand vor dieser
Spec, gemessen und durch einen Test gebunden.

Die Kopien stehen **vor** den Shadow-Hash-Mustern, die ihrerseits „bewusst
als erste" dokumentiert sind. Das stört einander nicht: Ein Crypt-Hash
enthält keinen PEM-Anker und ein PEM-Block keine `$id$`-Struktur, die
beiden Mustergruppen können sich also nicht gegenseitig anschneiden. Die
Begründung für „ganz vorn" ist bei beiden dieselbe.

**(B) Die Query-String-Regel zählt `&` nur nach einem `?` im selben
Token.** Vorher griff sie auch mitten in einem Passwort, das ein
`&<schlüsselwort>=` enthält, und nahm dem strengen URL-Muster den Anker:
`https://u:Geheim&token=b@h/x` → `https://u:Geheim&[REDACTED]`, wo vor
Spec 0078 `https://u:[REDACTED]@h/x` stand. Die Zeichenklasse zwischen `?`
und `&` schließt `@` aus, damit der Trenner nicht selbst über Zugangsdaten
läuft.

Nebeneffekt, gewollt: Der in §6 beschriebene Restfall ist damit wieder auf
die `?`-Form beschränkt, so wie Spec §5 ihn beschreibt. Gemessen und
bestätigt: Bei einem Nicht-DB-Schema (`https://u:a?token=b@h/x`) blieb der
Präfix schon vor Spec 0078 stehen — die Einschränkung „nur DB-Schemata" in
§5 stimmt also weiterhin.

**Korrektur an einer eigenen Aussage:** Eine frühere Fassung dieses
Abschnitts behauptete „Die `&`-Form ist zu". Das ist falsch. `(B)` schließt
die **Lockerung** bei `https://u:Geheim&token=b@h/x`, öffnet aber die
Spiegelseite wieder: `redis://cache:6379&password=p@ssw0rd` →
`redis://cache:[REDACTED]@ssw0rd`. Gemessen ist das identisch mit dem Stand
**vor** Spec 0078 — also kein Verstoß gegen §2, aber auch keine
Verbesserung. Zwischenzeitlich (`387a91e`) war der Fall zu, und zwar genau
durch die Lockerung, die `(B)` beseitigt. Steht jetzt als Restfall in Spec
§5. Fund des `spec-reviewer`, dritte Runde.

**Zweite Nebenwirkung von (A), nachgetragen:** Wo ein früheres Muster den
Anker bisher zerschnitt und der Rest lesbar blieb, schwärzt das gierige
Rückfallmuster jetzt alles ab dem `BEGIN` bis zum Textende. Das ist die
gewollte Fail-safe-Richtung (Spec 0002 §1) und dieselbe Überredaktion, die
das Rückfallmuster an seiner alten Stelle schon immer hatte — sie tritt nur
jetzt häufiger ein. Nichts leakt dadurch.

Alle drei Tests sind gegen `387a91e` rot gesehen worden.

## 12. N1 läuft zweimal (Q-BL-0248-03)

Die Trennergruppe aus §11 schließt `@` aus und kann deshalb nicht über
einen schon `@`-haltigen Parameter hinweglaufen; `replace_all` sucht nur
vorwärts und findet hinter dem ersten Treffer kein `?` mehr. Bei **zwei**
`@`-haltigen Schlüsselwort-Parametern im selben Token erreichte N1 damit
nur den ersten, das DB-Muster fraß danach den Anker des zweiten, und dessen
Schwanz stand im Klartext:

| Stand | `redis://cache:6379?password=p@ss&token=abc@SECRETTAIL` |
|---|---|
| vor Spec 0078 | `redis://cache:[REDACTED]@ss&[REDACTED]` |
| `387a91e` | `redis://cache:6379?[REDACTED]` |
| nach (B), vor dieser Korrektur | `redis://cache:[REDACTED]@SECRETTAIL` ← Verstoß gegen §2 |
| jetzt | `redis://cache:6379?[REDACTED]` |

Fund des `spec-reviewer` (dritte Runde, ERHÖHT), von mir an allen vier
Ständen nachgemessen — der Reviewer konnte in dieser Runde nicht ausführen
und hat das gesagt.

**Korrektur: N1 steht zweimal hintereinander in der Liste, wörtlich
gleich.** Nach dem ersten Durchlauf steht am ersten Parameter `[REDACTED]`,
das kein `@` enthält, und die Trennergruppe kommt daran vorbei. Eine
zusätzliche Anwendung derselben Regel kann per Konstruktion nur mehr
redigieren, nie weniger — das ist der ganze Grund, warum diese Form der
Korrektur vertretbar ist, ohne die Kette erneut zu vermessen.

**Restfall:** Drei und mehr `@`-haltige Schlüsselwort-Parameter im selben
Token bräuchten je eine weitere Anwendung; ab dem dritten bleibt es beim
Verhalten vor Spec 0078. Die saubere Form wäre eine Schleife, bis sich
nichts mehr ändert — das hieße `redact_bytes` umzubauen, was Spec §2
ausdrücklich ausschließt. Deshalb die feste zweite Anwendung statt einer
dritten, vierten und fünften: sie deckt den gemessenen Fall ab, und alles
darüber ist in §5 als Restfall benannt statt stillschweigend offen.

**Die Begründung des `@`-Ausschlusses im Trenner trägt nicht** (auch ein
Fund der dritten Runde): Der Trenner wird über `${sep}` wörtlich
zurückgeschrieben, kann also nichts zerstören, worüber er läuft. Sein
einziger realer Effekt ist die Reichweitenbegrenzung — genau die, die den
Fund oben verursacht hat. Der Ausschluss bleibt trotzdem stehen, weil er
Teil der gemessenen Fassung aus Q-BL-0248-02 ist und ihn zu entfernen eine
eigene Messrunde bräuchte; der Kommentar im Code nennt jetzt aber den
zutreffenden Grund statt des falschen.
