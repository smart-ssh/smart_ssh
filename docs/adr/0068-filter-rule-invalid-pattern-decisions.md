# ADR 0068: Filterregeln mit einem Muster, das sich nicht übersetzen lässt

Status: Angenommen
Bezug: docs/specs/0077-filter-rule-invalid-pattern.md (§1, §4, §9 mit der
Klarstellung Q-BL-0249-01), Spec 0060 (strenger Glob-Zweig), Spec 0009
(Regelverwaltung), Spec 0024 §5 (Fehlercodes), Spec 0016 (Logging),
CLAUDE.md („never loosen an existing check")

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

## 4. Das Protokoll trägt die Regel, nie das Kommando

3.2.2 und §5 der Spec verlangen, dass der Eintrag nur Regel-Kennung, Aktion
und den Fehlertext der Bibliothek enthält. Das Kommando bleibt draußen: Der
Eintrag ist eine neue Datensenke, und ein Kommando kann ein Geheimnis in
einem Argument tragen.

Der Fehlertext der Bibliothek enthält Teile des **Musters**. Das ist so
gewollt — ohne die Fundstelle im Muster wäre der Eintrag für die Diagnose
wertlos, und ein Muster ist selbst geschriebene Konfiguration, kein
Fremdinhalt.

Die Folge davon ist ausdrücklich festzuhalten, weil sie beim Lesen von 3.2.2
nicht ins Auge springt: Ein Muster ist Text, den ein Mensch geschrieben hat,
und es **kann** ein Geheimnis enthalten — etwa eine Regel, die auf ein
Kommando mit einem Passwort im Argument zielt und durch einen Tippfehler
ungültig ist. Die Schwärzung aus Spec 0016 greift an dieser Stelle nicht: Sie
sitzt im Weg der KI-Ausgaben, nicht an diesem Protokolleintrag. Ein solches
Muster stünde also im Klartext in der Protokolldatei, und zwar bei jeder
Auswertung erneut. Das ist die bewusst getroffene Abwägung aus §5 der Spec
(„Dort landen nur Regel-ID, Aktion und Fehlertext des Musters"), aber sie
verdient eine ausdrückliche Zeile, weil sie neu ist.

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

- **Der Protokolleintrag ist weder gekürzt noch entdoppelt.** Der Fehlertext
  wird ungekürzt geschrieben und entsteht bei jeder Auswertung neu. Bei einer
  Altzeile mit einem sehr langen ungültigen Muster wächst die Protokolldatei
  entsprechend. Nicht behoben, weil 3.2.2 ausdrücklich „den Fehlertext der
  Bibliothek" verlangt — eine Kürzung wäre eine Abweichung von der Spec, und
  eine Entdopplung bräuchte Zustand über Auswertungen hinweg, den die
  Auswertung heute nicht hat. Es ist eine Frage der Protokoll-Hygiene, nicht
  der Vertraulichkeit: Was drinsteht, ändert sich dadurch nicht.
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
