# ADR 0068: Filterregeln mit einem Muster, das sich nicht übersetzen lässt

Status: Angenommen
Bezug: docs/specs/0077-filter-rule-invalid-pattern.md (§1, §4, §9 mit den
Klarstellungen Q-BL-0249-01, Q-BL-0249-02, Q-BL-0249-03), Spec 0060
(strenger Glob-Zweig), Spec 0009 (Regelverwaltung), Spec 0024 §5
(Fehlercodes), Spec 0016 (Logging), CLAUDE.md („never loosen an existing
check")

Spec 0077 hält fest, dass ein Glob- oder Regex-Muster erst bei der
Auswertung übersetzt wurde und ein Übersetzungsfehler dort zu „passt nicht"
wurde. Für eine Allow-Regel war das die gewollte, sichere Richtung; für
Deny und Bestätigen hieß dasselbe Verhalten, dass aus der Regel
stillschweigend keine Regel wurde. Dieses ADR protokolliert die
Entscheidungen, die bei der Umsetzung getroffen oder offen gelassen wurden.

## 1. „Wie nicht vorhanden", keine Ersatz-Eskalation

Entschieden in der Spec (§1, §8 Punkt 1): Eine Regel mit einem Muster, das
sich nicht übersetzen lässt, verhält sich bei der Auswertung so, als gäbe es
sie nicht. Es gibt **keine** Ersatz-Eskalation auf Bestätigen oder Ablehnen
für den Geltungsbereich der Regel.

Begründung, hier festgehalten, weil sie beim Lesen des Codes nicht sichtbar
ist: Eine kaputte Regel soll nicht mehr bewirken als eine fehlende. Eine
Ersatz-Eskalation hätte aus einem Tippfehler eine wirksame Sperre gemacht,
die niemand angelegt hat — und sie hätte die Entscheidung für Regelmengen
verändert, die heute schon funktionieren.

Die Folge: An der Auswertung ist **nichts** geändert worden. `evaluate_
rules_explained`, `code_priority`, die Bucket-Reihenfolge und die
`unwrap_or(false)`-Stellen in `pattern.rs` sind unverändert (Spec 0077,
3.2.4). Die einzige Änderung im Auswertungspfad ist ein Aufruf von
`report_invalid_patterns`, der die Regelmenge unverändert weiterreicht.
Belegt durch T-A6: eine Matrix aus Aktion × Musterzustand × Mustertyp,
deren erwartete Werte am unveränderten Stand gemessen wurden.

## 2. Die Prüfung ist strenger als jeder einzelne Zweig — die Auswertung
nicht

`Pattern::validate` (3.1.1) übersetzt genau die Zweige, die die Auswertung
übersetzt: `Regex::new` bzw. `Glob::new`, und bei einem pfadförmigen Glob
zusätzlich den strengen Zweig aus Spec 0060. Scheitert **einer** davon, gilt
das Muster als ungültig und wird nicht gespeichert.

Die Auswertung ist an dieser Stelle bewusst **milder**: Ein pfadförmiger
Glob, bei dem nur einer der beiden Zweige übersetzt, greift weiter über den
Zweig, der übersetzt (3.2.1). Beides zusammen ist kein Widerspruch, sondern
die einzige Kombination, die in beide Richtungen sicher ist:

- Würde die Prüfung nur einen Zweig verlangen, ließe sich ein Muster
  speichern, das im anderen Zweig nie greift — die Lücke bliebe offen.
- Würde die Auswertung eine solche Regel ganz streichen, würde eine heute
  greifende Deny-Regel schwächer. Das schließt CLAUDE.md aus („never loosen
  an existing check"), und T-A12 belegt es in beide Richtungen: einmal
  trägt der strenge Zweig, einmal der permissive.

Praktisch heißt das: Ein solches Muster lässt sich nicht mehr **neu**
anlegen, eine vorhandene Regel dieser Art bleibt aber genau so wirksam wie
bisher.

## 3. Bewusst offen gelassen: die Restlücke aus §4 der Spec

Eine Regel, die vor dieser Änderung gespeichert wurde oder aus einer
künftigen Organisations-Quelle stammt, kann weiterhin ein Muster tragen,
das sich nicht übersetzen lässt, und ist dann wirkungslos wie bisher. Es
gibt bewusst **keine** Migration, die solche Zeilen löscht oder umschreibt
(Spec 0077, §2 „Nicht-Ziele").

Was stattdessen passiert: Die Regel wird sichtbar gemacht — in der
Regelliste markiert (3.2.3) und bei jeder Auswertung auf Fehlerebene
protokolliert (3.2.2). Die Entscheidung, es dabei zu belassen, liegt in der
Spec (§1, §4); dieses ADR hält nur fest, dass sie bewusst getroffen wurde
und nicht übersehen ist.

## 4. Das Protokoll trägt die Regel, nie das Kommando — und seit
Q-BL-0249-03 auch nie das Muster im Klartext

3.2.2 und §5 der Spec verlangen, dass der Eintrag nur Regel-Kennung, Aktion
und einen Hinweis auf den Fehler enthält. Das Kommando bleibt draußen: Der
Eintrag ist eine neue Datensenke, und ein Kommando kann ein Geheimnis in
einem Argument tragen.

**Ursprünglich vorgesehen** war dafür der Fehlertext der Bibliothek
(`regex`/`globset`) — der zitiert das Muster wörtlich, was für die Diagnose
zunächst richtig schien, weil ein Muster selbst geschriebene Konfiguration
ist, kein Fremdinhalt. Beim Review dieses Schritts wurde die Folge davon
ausdrücklich festgehalten: Ein Muster ist trotzdem Text, den ein Mensch
geschrieben hat, und es **kann** ein Geheimnis enthalten — etwa eine Regel,
die auf ein Kommando mit einem Passwort im Argument zielt und durch einen
Tippfehler ungültig ist. Die Schwärzung aus Spec 0016 greift an dieser
Stelle nicht: Sie sitzt im Weg der KI-Ausgaben, nicht an diesem
Protokolleintrag. Ein solches Muster hätte also bei jeder Auswertung erneut
im Klartext in der Protokolldatei gestanden.

**Entschieden (Stefan, Q-BL-0249-03, Spec §9):** Dieser Fund wurde nicht
hingenommen, sondern behoben. Der Eintrag trägt seither **nie** den
Fehlertext der Bibliothek, sondern einen von drei festen, textfreien
Kurztexten (`Pattern::compile_failure_reason`, `pattern.rs`): „regex does
not compile", „glob does not compile", „glob does not compile (strict
branch)". Diese Klassifizierung prüft bewusst dieselben Zweige wie
`validate()` noch einmal selbst, statt aus dem `PatternError` abgeleitet zu
werden — so kann kein Bibliothekstext auch nur mittelbar ins Log gelangen.
Das DTO (3.2.3) und die Markierung in der Regelliste bleiben unverändert bei
`validate()` und damit beim vollen Bibliothekstext; nur dieses eine Log
ändert sich. Wer ein Geheimnis im Muster hat, sieht es also weiterhin in der
Oberfläche (und damit potenziell in einem Screenshot oder Support-Anhang) —
das ist die bewusst engere, gezielt auf die neue Datensenke beschränkte
Korrektur, nicht eine Aussage über die Oberfläche.

Der Eintrag entsteht in `evaluate_explained_inner` direkt nach dem Laden der
Regeln, nicht in der Bucket-Schleife. Die kehrt beim ersten Treffer zurück
und läuft je Teilkommando mehrfach; dort säße der Eintrag also mal zu oft
und mal gar nicht. T-A4 hält das fest: Die ungültige Regel liegt dort hinter
dem Treffer und würde in der Bucket-Schleife nie gemeldet.

## 5. Abweichungen von der Spec bei der Umsetzung

### 5.1 T-6c legt die Zeile über die Speicher-API an, nicht per rohem SQL

Die Spec verlangt für T-6c wörtlich ein rohes `INSERT INTO filter_rules`
nach dem Muster in `persistence-sqlite/src/tests.rs`. Das ist in `app-shell`
nicht möglich, ohne `sqlx` als Abhängigkeit aufzunehmen — `app-shell` hat
sie weder als Abhängigkeit noch als dev-dependency, und Spec 0077 gibt als
einzige neue Abhängigkeit `tracing-subscriber` für `core` frei.

Stattdessen legt der Test die Zeile über `SqlitePolicyStore::create` an
(Helfer `insert_rule_bypassing_layer_one`), also über die echte
Speicher-API und damit an Schicht 1 vorbei. Der Akzeptanzfall bleibt
derselbe: Die Zeile entsteht in genau dem Spaltenformat, das eine echte
Installation geschrieben hätte, `rules_for` lädt sie unverändert, die echte
`FilterEngine` wertet sie aus. Geprüft wird weiterhin, dass `list_all` sie
liefert, die Entscheidung dieselbe ist wie ohne die Regel, ein Eintrag auf
Fehlerebene entsteht und `list_rules` sie markiert.

### 5.2 `RuleWriteError` ohne `thiserror`

`app-shell` hängt nicht von `thiserror` ab. `Display`, `Error` und die
beiden `From`-Impls sind deshalb von Hand geschrieben statt abgeleitet —
gleichwertig, aber ohne neue Abhängigkeit.

### 5.3 Die Log-Aufzeichnung der Tests ist jetzt geteilt

Die Aufzeichnung, die bisher nur im Testmodul von `orchestration.rs` lag,
ist nach `test_support::log_capture` gewandert und wird von den Tests aus
Spec 0077 mitbenutzt. Grund: Ein globaler `tracing`-Default lässt sich pro
Prozess nur einmal setzen. Zwei Aufzeichnungen im selben Testbinary
gewinnen je nach Testreihenfolge gegeneinander, und das Modul, das verliert,
schreibt in den thread-lokalen Puffer des fremden Subscribers und sieht
seine eigenen Zeilen nie. Dieselbe Lehre steht in
`ai_providers::test_support`.

Der bestehende Redaction-Test aus Spec 0016 prüft unverändert dieselbe
Zusicherung; die Aufzeichnung ist weiterhin ungefiltert, damit sie sowohl
seine Einträge auf Infoebene als auch die auf Fehlerebene aus 3.2.2 sieht.

## 6. Behobene Funde aus dem Review

Zwei Funde sind in diesem Schritt behoben worden, beide rein textlich und
ohne Verhaltensänderung:

- **Der Protokolltext behauptete zu viel.** Der Eintrag aus 3.2.2 lautete
  „rule cannot match". Für den Einzelzweig-Fall, den 3.2.1 bewusst am Leben
  lässt, ist das falsch: Die Regel greift dort weiterhin über den Zweig, der
  übersetzt — T-A12 belegt es in beide Richtungen. Der Eintrag widersprach
  damit einem grünen Test im selben Commit und hätte jemanden, der das
  Protokoll liest, glauben lassen, eine wirksame Regel sei wirkungslos. Der
  Text nennt jetzt den Zweig, der scheitert; die Begründung steht am
  Doc-Kommentar der Funktion, damit eine spätere Kürzung sie nicht
  wiederholt.
- **Ein Doc-Kommentar in `accept_and_create_rule` war seit Spec 0021
  falsch.** Er behauptete, ein Fehlschlag beim Anlegen der Regel verhindere
  das Auflösen der Bestätigung — seit Spec 0021 gilt das Gegenteil, und der
  Kommentar an der `resolve`-Stelle sagte drei Zeilen tiefer das Richtige.
  Der Fund ist älter als dieser Schritt, aber dieser Schritt fasst genau
  diese Funktion an.

## 7. Funde aus dem Review, bewusst nicht in diesem Schritt behoben

Keiner davon ist sicherheitsrelevant; jeder ist ein Kandidat für ein eigenes
Item.

- ~~Der Protokolleintrag ist weder gekürzt noch entdoppelt.~~ **Erledigt
  durch Q-BL-0249-03:** Der Eintrag trägt seit dieser Klarstellung nur noch
  einen von drei festen Kurztexten (§4) statt des Bibliothekstexts — er
  entsteht zwar weiterhin bei jeder Auswertung neu, kann aber wegen der
  festen Länge die Protokolldatei nicht mehr über eine lange Altzeile
  aufblähen. Der ursprüngliche Fund (unbegrenzte Länge **und** Zitat des
  Musters) ist damit gegenstandslos.
- **Die Speicher-API hat kein eigenes Geländer.** `SqlitePolicyStore::create`
  und `update` sind weiterhin öffentlich und prüfen nichts; dass kein
  Schreibweg an der Prüfung vorbeiführt, hängt an der Disziplin in
  `app-shell`. Heute trägt das: Es gibt genau zwei produktive Aufrufer, beide
  prüfen vorher. Nicht behoben, weil 3.1.2 die Prüfung ausdrücklich in
  `filter_rules` verortet. Als Tiefenverteidigung wäre ein `ValidatedRule`-
  Typ oder die Prüfung an der Speichergrenze das, was eine künftige zweite
  Schreibstelle automatisch erfasst — das ist eine Architekturänderung und
  gehört nicht in einen Fix.
- **Der Fehlercode hängt an der Disziplin der Aufrufer.** `CommandError` hat
  einen pauschalen `From` für alles, was `Display` ist, und der setzt
  `code: None`. Schreibt jemand später ein `?` über einem `RuleWriteError`,
  statt die Umwandlungsfunktion aus 3.1.3 zu rufen, übersetzt der pauschale
  `From` das klaglos, und die Oberfläche verliert den Code wieder — ohne dass
  der Übersetzer meckert. Heute ist es an allen drei Stellen richtig, und
  T-6d hält das fest. Ein Schutz zur Übersetzungszeit ist nicht gebaut
  worden: Er bräuchte dieselbe Änderung am Fehlertyp wie das Geländer an der
  Speicher-API, und 3.1.3 nimmt diese Lage ausdrücklich hin. Gehört mit dem
  vorigen Punkt in ein Item „Tiefenverteidigung Regel-Schreibweg".
- **Die Auswertung übersetzt jetzt jedes Muster der Regelmenge.** Vorher
  übersetzte die Bucket-Schleife nur bis zum ersten Treffer. Die Abwägung
  hier ist eine eigene, nicht eine der Spec: §2 schließt das
  Zwischenspeichern von Mustern aus, aber das war als „kein Zwischenspeicher
  nötig" gemeint, nicht als „zusätzliche Übersetzungsarbeit kostet nichts".
  Nicht behoben, weil die Meldung aus 3.2.2 ohne diese Schleife nicht
  vollständig wäre (sie soll jede Regel melden, nicht nur die vor dem ersten
  Treffer) und weil ein Zwischenspeicher eine Änderung am `Rule`-Typ wäre,
  die über einen Fix hinausgeht. Der saubere Ort dafür wäre, ein Muster
  einmal zu übersetzen und am `Rule` mitzuführen.
- **Die Testen-Ansicht markiert nichts.** Sie zeigt für eine Regel mit
  ungültigem Muster weiterhin nur „passt nicht". 3.2.3 verlangt die Markierung
  ausdrücklich nur für die Regelliste.
- **Die Allow-Spalte von T-A6 ist degeneriert.** Weil die Basisregel der
  Matrix immer AutoExec liefert, steht in allen Allow-Zeilen derselbe Wert;
  die Spalte fängt eine Ersatz-Eskalation ab, aber keine Abschwächung. Die
  Form der Matrix gibt T-A6 wörtlich vor. Die Lücke, die dadurch entstünde,
  deckt T-A5 ab: eine Allow-Regel mit ungültigem Muster allein führt zu
  Confirm, nie zu AutoExec.

## 8. Nicht behoben, weil außerhalb dieses Schritts

`crates/core/src/risk/tests.rs`,
`test_secret_check_stays_fast_on_adversarial_long_input` misst eine feste
Laufzeitschranke von drei Sekunden und ist dadurch von der Auslastung der
Maschine abhängig. Der Test scheiterte in einem von drei Läufen dieses
Schritts unter voller Parallelität und war isoliert sofort wieder grün. Er
gehört nicht zu Spec 0077 und wurde hier nicht angefasst; er ist im Bericht
zu diesem Schritt als eigener Befund vermerkt.

## 9. Nachtrag: Klarstellungen Q-BL-0249-02 und Q-BL-0249-03

### 9.1 Q-BL-0249-02: Prioritäts-Pfeile brechen vor dem ersten Schreiben ab

`movePriority` (`FilterRulesView.tsx`) tauscht die Priorität zweier
benachbarter Regeln über zwei `updateRule`-Aufrufe. Die Klarstellung
verlangt, an einer Regel mit `patternError` beide Pfeile zu deaktivieren
(mit `title`) **und** vor dem ersten `updateRule` abzubrechen, wenn eine der
beiden am Tausch beteiligten Regeln ein ungültiges Muster trägt — auch wenn
nur die Nachbarregel betroffen ist und der eigene Pfeil deshalb noch
anklickbar wäre. 3.1.2 bleibt davon unberührt: Die eigentliche Sperre liegt
in `filter_rules::update_rule`, das jeden Schreibweg vor dem Speichern
prüft; der Frontend-Guard verhindert nur, dass ein halb angewandter Tausch
entsteht (`updateRule(a)` liefe durch, `updateRule(b)` würde von Schicht 1
abgewiesen — zwei Regeln blieben dann mit vertauschten, aber inkonsistent
angewendeten Prioritäten zurück).

**Bewusst nicht zusätzlich behoben (Review-Fund, `review-03.md`):** Der
Pfeil einer benachbarten, selbst gültigen Regel bleibt anklickbar, auch wenn
ein Klick wegen der kaputten Nachbarregel folgenlos bleibt (stummes
`return`, keine Meldung). Das ist wörtlich das, was die Klarstellung
verlangt — sie deaktiviert nur die Pfeile „an einer Regel mit
`patternError`", nicht die einer Nachbarregel. Den Nachbarpfeil ebenfalls zu
deaktivieren (mit demselben `title`) wäre eine über die Klarstellung
hinausgehende Änderung des sichtbaren Verhaltens und bräuchte eine eigene
Entscheidung, keine, die der Coder aus dem Review-Fund ableiten sollte.

### 9.2 Q-BL-0249-03: siehe §4

Die Entscheidung und ihre Begründung stehen jetzt in §4 (dort aktualisiert,
statt als separater Nachtrag dupliziert, weil §4 sonst sich selbst
widersprochen hätte).

### 9.3 Aus dem Review dieses Nachlaufs bewusst zurückgestellt

Aus `review-03.md`, keiner sicherheitsrelevant:

- **Kein Drift-Schutz zwischen `validate()` und `compile_failure_reason()`.**
  Beide prüfen heute identische Zweige in identischer Reihenfolge, rein
  textuell dupliziert. Ein künftiger zusätzlicher Prüfzweig in `validate()`
  (z. B. ein eigenes Größenlimit) würde `compile_failure_reason()`
  stillschweigend zurücklassen — eine Regel wäre dann in der Liste markiert,
  aber nie im Log gemeldet. Kein Sicherheitsproblem (die Auswertung selbst
  bleibt unverändert), aber ein Diagnose-Fehler. Nicht behoben, weil er eine
  Änderung an der Duplizierungs-Entscheidung selbst wäre (etwa ein
  Property-Test über eine Musterliste, der `validate().is_err() ==
  compile_failure_reason().is_some()` behauptet) und über den Umfang dieser
  Klarstellung hinausgeht.
- **Keine Leak-Assertion für die beiden Glob-Zweige.** Der neue Test
  (`test_spec_0077_q_bl_0249_03_pattern_error_log_never_quotes_the_pattern`)
  deckt nur den Regex-Fall ab, verstärkt um die exakte Feldwert-Prüfung. Die
  Spec verlangt „ein ungültiges Muster, das ein Geheimnis enthält" — das ist
  mit einem Fall erfüllt; die beiden Glob-Zweige teilen sich dieselbe
  Klassifizierungsfunktion und damit dieselbe Eigenschaft (nie
  Bibliothekstext), nur ohne eigenen Test dafür.
- **`!!rule.patternError` statt `rule.patternError !== null`**
  (`FilterRulesView.tsx`, drei Stellen). Ein leerer String würde als „kein
  Fehler" behandelt. `PatternError::from_library` erzeugt nie eine leere
  Meldung, der Fall ist praktisch unerreichbar — kosmetisch, nicht behoben.
- **Regel-Kennung einer künftigen `Organization`-Quelle im Log.** Heute sind
  alle Regel-IDs vom Store erzeugte UUIDs (`filter_rules.rs`). Eine spätere
  `RuleOrigin::Organization`-Quelle (privates Repo, nicht Teil dieser Spec)
  könnte ihre eigenen IDs mitbringen; ob die vertrauenswürdig genug für ein
  Log sind, ist nirgends entschieden. Außerhalb des Scopes dieses Nachlaufs.
