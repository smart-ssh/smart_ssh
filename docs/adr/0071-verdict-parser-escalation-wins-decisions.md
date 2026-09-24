# ADR 0071: Urteils-Parser der Zweitmeinung — Entscheidungen bei der Umsetzung von Spec 0074

Status: Angenommen
Bezug: docs/specs/0074-verdict-parser-escalation-wins.md, ADR 0024
(„nur Eskalation, nie Abschwächung"), Spec 0026 Abschnitt 3, Spec 0039
Abschnitt 5.2

Spec 0074 stellt beide Urteils-Parser in `crates/app-shell/src/
risk_second_opinion.rs` von „erstes Wort gewinnt" auf „eskalierendstes Wort
gewinnt" um. Die Umsetzung folgt der Spec wörtlich (A1–A5, I1–I4). Dieses
ADR hält fest, was die Spec offen ließ, und welche Funde des
`spec-reviewer` bewusst stehen bleiben.

## 1. Ein gemeinsamer Parser-Kern statt zweier paralleler Fassungen

Die Spec beschreibt zwei Funktionen mit gleichem Aufbau und verlangt für
beide dieselbe Änderung, sagt aber nichts darüber, ob sie getrennt bleiben.
Beide teilen sich jetzt `parse_escalating_verdict`, das über `V: Ord`
parametrisiert ist; `parse_second_opinion` reicht `RiskLevel` hinein,
`parse_injection_check` `bool`.

Grund: Bei zwei getrennten Fassungen derselben Sicherheitslogik wird
erfahrungsgemäß nur eine gepflegt — genau so ist die Lücke aus BL-0121
entstanden (Spec 0039 übernahm den Parser aus Spec 0026 „wie er ist",
samt Fehler). Ein Kern kann nicht halb repariert werden. Die Ordnung
„eskalierend = größer" ist für beide Typen schon von der Sprache gegeben
(`None < Yellow < Red` durch die Deklarationsreihenfolge in
`crates/core/src/risk/types.rs`, `false < true` für `bool`), es braucht
also keine eigene Vergleichslogik, die falsch sein könnte.

## 2. Bei gleichrangigen Urteilswörtern gewinnt das erste — für die Begründung

A4 verlangt die Begründung „nach dem Wort, das gewonnen hat", sagt aber
nicht, welches Vorkommen gemeint ist, wenn dasselbe Urteil mehrfach
vorkommt. Entschieden: das **erste**. Die Bedingung lautet deshalb
`verdict > best_verdict` und nicht `>=`.

Grund: Nur so ist das Verhalten für Antworten mit genau einem Urteilswort
wortgleich mit dem bisherigen — und das ist der Normalfall, weil beide
Prompts um genau ein Urteil bitten. Auf das Urteil selbst hat die
Entscheidung keinen Einfluss, nur auf den Text der Begründung. Als
Eigenschaft geprüft: Stimmen altes und neues Urteil überein, ist die
Begründung wortgleich (`test_t1_*`).

## 3. Randfall der Rückfallregel: mehr Text als vorher

Steht das gewinnende Wort am Ende der Antwort, bleibt dahinter nichts, und
die Begründung wird — wie bisher — die **volle** Antwort. Neu ist, dass
dieser Fall jetzt auch eintreten kann, wenn vorne ein schwächeres Urteil
stand; die angezeigte Begründung enthält dann auch Text vor dem Urteil,
also gegebenenfalls vom Modell zitierten Inhalt.

Bewusst so belassen: Es ist A4 wörtlich, und der Weg ist unbedenklich —
die Begründung des Injektions-Checks wird am Aufrufer verworfen
(`orchestration.rs`, `_reason`), die der Zweitmeinung geht nur als Event
ins Frontend und landet dort als Tooltip, nicht in der persistierten
Historie, nicht im Log, und wird nirgends ausgeführt (I4). Der zugrunde
liegende Inhalt ist vor dem Provider-Aufruf bereits redigiert. Das
Verhalten ist jetzt durch einen Test festgehalten, statt unbemerkt zu
bleiben.

## 4. Bewusst nicht behoben: der Preis der Falsch-Eskalation ist größer als die Spec ihn zeichnet

Spec §4.2 benennt den Preis der Umstellung — zitierter Inhalt kann jetzt
eine falsche Eskalation auslösen — und illustriert ihn an einem `red` im
Kommandotext. Der `spec-reviewer` hat den in der Praxis wahrscheinlich
häufigeren Fall herausgearbeitet: Der Injektions-Check läuft auch auf
SFTP-Dateiinhalten und Kommandoausgaben. Zitiert das Modell in seiner
Begründung eine gewöhnliche Konfigurationszeile wie `PermitRootLogin yes`,
`UsePAM yes` oder ein YAML `enabled: yes` und schließt danach mit „nein",
lautet das Ergebnis jetzt `true`, und die nächste Auto-Ausführung wird zu
einer Bestätigung. Vorher trat das praktisch nie ein, weil die Modelle das
Urteil prompt-gemäß nach vorn stellen.

Das bleibt so. Die Richtung ist die richtige, und §4.2 trägt den Preis
ausdrücklich: Eine falsche Eskalation ist sichtbar und mit einem Klick
erledigt, eine verschluckte nicht. Ein Zurück zu „erste gewinnt" schließt
die Spec ausdrücklich aus.

Die Größenordnung ist aber ein Betriebsrisiko (Confirm-Fatigue schwächt
die letzte Verteidigungslinie), und die Spec selbst nennt die Antwort
darauf: ein **strukturiertes Antwortformat** für Zweitmeinung und
Injektions-Check. Empfehlung an den Product Owner, als eigenes Item zu
ziehen — es schließt zugleich die unter Punkt 5 genannten Restlücken. Ob
und wann, ist keine Entscheidung dieses Laufs.

## 5. Bewusst nicht behoben: Restlücken der Wortbereinigung

A5 schreibt die bisherige Bereinigung fest (nur alphanumerische Zeichen,
kleingeschrieben, Vergleich auf Gleichheit). Damit bleiben Antwortformen
unerkannt, in denen ein Urteilswort nicht als eigenes Wort dasteht:
`"Antwort:ja"` wird zu `antwortja`, ebenso `"ja,aber"`, `"<answer>ja
</answer>"` oder ein JSON `{"answer":"ja"}` am Stück. Dasselbe gilt für
Homoglyphen (kyrillisches `а` in `jа`) und vollbreite Zeichen (`ｊａ`).

Alle diese Fälle waren vor Spec 0074 genauso unerkannt — die Bereinigung
ist unverändert, diese Spec hat daran nichts verschlechtert. Sie zu
schließen hieße, die Gleichheitsprüfung aufzuweichen, und genau davor warnt
ADR 0024: Eine Teilstring-Suche träfe „redirect", „nonetheless", „nobody",
„yesterday". Der saubere Weg ist wieder das strukturierte Antwortformat aus
Punkt 4. Zero-Width-Zeichen und geschützte Leerzeichen sind dagegen
robust, weil sie herausgefiltert bzw. als Worttrenner erkannt werden.

## 6. Grenze des Monotonie-Tests, benannt statt kaschiert

Spec §6.1 nennt T1 den „eigentlichen Nachweis von I1". Das trifft es nicht
genau, und der Test sagt jetzt selbst, was er kann. Sein Vergleichsmaßstab
ist der alte Parser, im Testmodul nachgebaut. Die Ungleichung
`current >= legacy` allein ist deshalb gegen den **ungefixten** Stand nicht
falsifizierbar — dort sind beide Seiten identisch, die Eigenschaft gilt
trivial. Der direkte Beleg, dass der Fix wirkt, sind die Beispiele T2, T7,
T8, T10, X1 und X6.

Der Zähler am Ende ändert das Bild allerdings, und zwar in beide
nützlichen Richtungen: Er verlangt mindestens einen Fall, in dem der neue
Parser tatsächlich höher meldet. Gegen „erstes Wort gewinnt" bleibt er
zwangsläufig bei null, also **scheitert T1 dort** — der Test ist damit
doch falsifizierend, nur nicht über die Ungleichung, sondern über die
Abdeckung. Zugleich verhindert er, dass ein später degenerierter Generator
den Test still zur leeren Hülle macht.

Beides ist nachgemessen, nicht hergeleitet: Gegen eine „letztes Wort
gewinnt"-Fassung scheitern beide T1-Tests an der Ungleichung, mit
konkretem Gegenbeispiel; gegen die First-wins-Fassung scheitern sie am
Zähler. Dass der Zähler nur bei echter Eskalation hochgeht, folgt aus
seiner Stellung im `else`-Zweig hinter der Ungleichung.
