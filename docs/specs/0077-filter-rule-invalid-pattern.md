# Spec 0077 — Filter-Regeln mit ungültigem Muster

Status: **freigegeben** (Stefan, 2026-09-24) · Backlog: BL-0249 · Gate: release-1.0/C
Repo: **öffentlich** `smart-ssh` — `crates/core/src/filter/` (Muster, Auswertung),
`crates/app-shell/src/filter_rules.rs` und `error.rs` (Anlegen/Ändern),
Frontend (`errorCodes.ts`, `locales/{de,en}/common.json`)
Review-Priorität: **ERHÖHT** (Filter-Engine; adversariale Fälle in §6.3)
Zweck: Ein Glob- oder Regex-Muster, das sich nicht übersetzen lässt, wird
beim Speichern abgewiesen. Ist es trotzdem in der Regelliste, wird die
Regel bei der Auswertung behandelt, als gäbe es sie nicht, und das
geschieht laut.

## 1. Ausgangslage

`Pattern::matches` (`crates/core/src/filter/pattern.rs:37-47`) übersetzt
Glob- und Regex-Muster erst beim Auswerten. Ein Übersetzungsfehler wird
mit `unwrap_or(false)` zu „passt nicht". Dasselbe gilt für den strengen
Glob-Zweig in `matches_for_user_rule` (`pattern.rs:118-124`). Der
Doc-Kommentar (`pattern.rs:15-18`) begründet das nur für **Allow**: „eine kaputt konfigurierte
Allow-Regel darf niemals versehentlich zu AutoExec führen". Für Deny und
Confirm ist dasselbe Verhalten eine Abschwächung: Aus der Regel wird
stillschweigend keine Regel.

Beim Anlegen oder Ändern wird das Muster nirgends geprüft. Der Weg ist
`RuleInput` (`crates/app-shell/src/dto.rs:805-812`) → `pattern_from_parts`
(`dto.rs:784-789`, baut `Pattern::Regex(pattern_value)` direkt) →
`filter_rules::create_rule`/`update_rule` (`filter_rules.rs:17-39`) →
`SqlitePolicyStore::create`/`update`. Ein zweiter Weg dorthin ist die
Schnellregel aus dem Bestätigungsdialog: `accept_and_create_rule`
(`commands.rs:3045 ff.`) → `rule_suggestions::create_quick_rule`
(`rule_suggestions.rs:95-110`, immer Allow) → `filter_rules::create_rule`.
Ihre Vorschläge baut `rule_suggestions` ungeprüft aus Kommando-Token
(`"{token} *"`, `"{t0} {t1} *"`, `rule_suggestions.rs:56, 69`). Enthält ein
Token `[` oder `{`, ist der vorgeschlagene Glob ungültig. Gemessen:
`Glob::new("ls [abc *")` → „unclosed character class; missing ']'". Das Formular prüft nur, ob das Feld
leer ist (`FilterRulesView.tsx:350-353`, HTML-`required`). Außerhalb von
`pattern.rs` gibt es in diesen Dateien kein `Regex::new`/`Glob::new`
(per grep gemessen).

**Gemessen** (2026-09-24, echte `FilterEngine::evaluate` über eine
Mess-Crate außerhalb des Repos, Kommando `systemctl stop nginx`):

| Regeln | Ergebnis |
|---|---|
| Allow `systemctl *` + Deny mit gültigem Regex | Deny |
| Allow `systemctl *` + Deny mit Regex, der nicht übersetzt | **AutoExec** |
| nur Deny mit ungültigem Muster | Confirm (wie ohne Regel) |

Eine Deny-Regel, die eine breitere Allow-Regel einschränkt, ist bei einem
Tippfehler wirkungslos, und das Kommando läuft ohne Bestätigung. Sichtbar
ist davon nichts: Die Regel wird gespeichert und angezeigt, und die
Testen-Ansicht zeigt dasselbe stille „passt nicht".

Weitere Quellen für Regeln: `PolicySource` (`engine.rs:44-57`). Im Repo
gibt es als Implementierungen nur `SqlitePolicyStore`
(`policy_store.rs:350`) und das Test-Double in `filter/tests.rs:898`. Eine
spätere `RuleOrigin::Organization`-Quelle liefert ihre Regeln über
dieselbe Auswertung, an Schicht 1 vorbei.

Es gibt kein eigenes Größenlimit für Regex (`RegexBuilder`/`size_limit`
kommen im Repo nicht vor). Die Voreinstellung von `regex` lehnt ein zu
großes Muster ab. **Gemessen:** `a{1000}{1000}` →
„Compiled regex exceeds size limit of 10485760 bytes". Das ist für diese
Spec derselbe Fall wie ein Syntaxfehler.

**Gemessen, zweiter Befund** (2026-09-24; zweite Tabellenzeile am selben
Tag korrigiert, Q-BL-0249-01, §9): Bei einem pfadförmigen Glob kann genau
**einer** der beiden Zweige scheitern, und zwar in beide Richtungen. Mit
Allow `rm *` daneben:

| Deny-Muster | `Glob::new` | strenger Zweig (normalisiertes Muster) | Kommando | heute |
|---|---|---|---|---|
| `rm /x/[a/../b` | Fehler | ok (`rm /x/b`) | `rm /x/b` | **Deny** (über den strengen Zweig) |
| `rm /x/[a/b]/../c` | ok | Fehler (`rm /x/[a/c`) | `rm /x/a/../c` | **Deny** (über den permissiven Zweig) |
| `rm /x/[a/b]/../c` | ok | Fehler | `rm /x/c` | AutoExec (kein Zweig passt) |

Die Richtung „nur der strenge Zweig scheitert" entsteht so:
`normalize_lexical_path` zerlegt das Token an `/`, das Segment `..` löscht
das vorangehende Segment `b]`, und die öffnende `[` bleibt ohne Gegenstück
stehen. Umgekehrt repariert dieselbe Auflösung `rm /x/[a/../b` zu
`rm /x/b`, das der permissive Zweig nicht übersetzt. Ein Muster, dessen
`..` **innerhalb** einer Klammer steht (`rm /x/{a/..}/b`), ist kein Fall
dieser Art: Das Segment heißt dort `..}`, wird nicht als Aufwärts-Segment
erkannt, und das Muster übersetzt in beiden Zweigen.

Eine Regel, die in einem Zweig nicht übersetzt, kann also heute trotzdem
greifen — über den jeweils anderen Zweig. Schicht 2 darf ihr das nicht
wegnehmen (3.2.1).

**Entscheidung (Stefan, 2026-09-24):** Eine Regel mit ungültigem Muster
wird bei der Auswertung behandelt, **als würde sie nicht existieren**.
Keine Ersatz-Eskalation auf Confirm oder Deny. Die Lücke schließt damit
Schicht 1 für jede neu angelegte oder geänderte Regel. Eine schon
gespeicherte ungültige Regel bleibt wirkungslos wie heute, wird aber
sichtbar gemacht (3.2).

## 2. Ziel und Nicht-Ziele

**Ziel:**
1. Ein Muster, das sich nicht übersetzen lässt, wird gar nicht erst
   gespeichert.
2. Kommt eine Regel doch mit einem solchen Muster in die Auswertung,
   verhält sie sich, als gäbe es sie nicht. Keine Entscheidung wird
   dadurch milder als heute. Das ist **sichtbar**: im Log mit Regel-ID
   und in der Regelliste (3.2.3).

**Nicht-Ziele:**
- `Pattern::matches` für `crate::risk` bleibt unverändert (siehe
  Doc-Kommentar `pattern.rs:20-36`). Dessen Muster sind fest eingebaut.
  Ein Test stellt sicher, dass sie alle übersetzen (§6.2).
- Keine Änderung an Matching-Semantik, Bucket-Reihenfolge, Scope,
  Priorität, original/stripped/resolved oder an Spec 0060.
- Muster werden nicht zwischengespeichert oder vorab übersetzt (das wäre
  eine Performance-Frage und hat mit diesem Fund nichts zu tun).
- Kein eigenes Größenlimit für Regex (Entscheidung Stefan, §8).
- Keine Ersatz-Eskalation (Confirm oder Deny) für ungültige Regeln bei
  der Auswertung (Entscheidung Stefan, §1).
- Keine Migration, die alte ungültige Regeln löscht oder umschreibt.

## 3. Anforderungen

### 3.1 Schicht 1: beim Anlegen und Ändern abweisen

- **3.1.1** `core` MUSS eine öffentliche Prüfung anbieten, etwa
  `Pattern::validate(&self) -> Result<(), PatternError>`. Sie übersetzt
  **genau die Varianten, die die Auswertung übersetzt**:
  - Regex: `Regex::new(pattern)`.
  - Glob: `Glob::new(pattern)`. Ist das Muster pfadförmig
    (`is_path_shaped_pattern`), zusätzlich der strenge Zweig, also
    `GlobBuilder::new(&normalize_path_shaped_tokens(pattern))
    .literal_separator(true).build()`.
  - Exact ist immer gültig.

  `PatternError` trägt den Fehlertext der Bibliothek.
- **3.1.2** `filter_rules::create_rule` und `update_rule` MÜSSEN
  `validate` aufrufen, **bevor** der Store etwas schreibt. Bei einem
  Fehler wird nichts gespeichert (`create`), und die gespeicherte Regel
  bleibt unverändert (`update`). Die Schnellregel läuft über
  `create_rule` und ist damit abgedeckt. Ihr Fehlerweg bleibt wie in
  `accept_and_create_rule`: Die Bestätigung wird trotzdem aufgelöst, und
  der Fehler kommt getrennt zurück.
- **3.1.3** Fehlertyp: `create_rule`/`update_rule` liefern heute
  `PolicyStoreError`, und der kann keinen Code tragen (`error.rs:126-130`
  setzt `code: None`). Sie bekommen einen eigenen Fehlertyp in
  `filter_rules`, etwa `RuleWriteError { InvalidPattern(PatternError),
  Store(PolicyStoreError) }`. `CommandError` hat einen
  pauschalen `impl<E: Display> From<E>` mit `code: None`
  (`error.rs:102-110`, `:126-130`). Ein eigener `From<RuleWriteError>` ist
  daneben nicht möglich (E0119), und `?`/`map_err(Into::into)` würden den
  Code **still** verlieren. Deshalb MUSS es eine ausdrückliche
  Umwandlungsfunktion geben, nach dem Muster von
  `keychain_aware_credential_error` (`error.rs:58`). Sie setzt für
  `InvalidPattern` den Code `FILTER_RULE_PATTERN_INVALID`
  (`CommandError::with_code`, `error.rs:94`) mit dem Fehlertext der
  Bibliothek als `message`. `Store` wandelt sie um wie heute. **Alle drei
  Commands** rufen sie auf: `create_rule` und `update_rule`
  (`commands.rs:2960-2962`, `:2971-2973`) sowie `accept_and_create_rule`
  (`commands.rs:3090`, `Ok(rule_result?)`). `create_quick_rule`
  übernimmt denselben Fehlertyp.
- **3.1.4** Frontend: Der Code kommt in `errorCodes.ts` und je eine Zeile
  in `locales/de` und `locales/en`. `translateErrorCode` ersetzt den Text
  bei einem bekannten Code vollständig (`errorCodes.ts:143-144`), und
  das Formular übersetzt heute gar nicht (`FilterRulesView.tsx:315`,
  `setError(commandErrorMessage(err))`). Deshalb gilt: Das Formular zeigt
  für diesen Code den übersetzten Satz **und darunter** den
  `message`-Text (den Fehlertext der Bibliothek mit der Stelle), an der
  vorhandenen Fehlerstelle (`FilterRulesView.tsx:436`). Andere Codes im
  Formular bleiben wie heute. Die Schnellregel zeigt ihren Fehler dort,
  wo heute ein Fehler von `accept_and_create_rule` erscheint, übersetzt.
  Dazu reicht `ChatPanel.tsx:616` `commandErrorCode(err)` weiter statt
  `code: null`. Dort erscheint nur der übersetzte Satz, ohne den
  Fehlertext der Bibliothek (`ChatPanel.tsx:1031` ersetzt die `message`).
  Das ist bewusst so: Dort wird ein Vorschlag angelegt, kein
  selbstgeschriebenes Muster.
- **3.1.5** `rule_suggestions` SOLL keinen Vorschlag anbieten, der
  `validate` nicht besteht. Der Vorschlag wird dann weggelassen, nicht
  verändert.
- **3.1.6 Wortlaut:** Ein Schlüssel `errors.FILTER_RULE_PATTERN_INVALID`
  für alle Stellen. Der Satz muss im Formular, bei der Schnellregel und
  in der Regelliste passen. Vorgabe (de): „Ungültiges Muster in einer
  Filterregel. Bitte das Muster korrigieren." Im Formular steht darunter der
  Fehlertext (3.1.4). Die englische Fassung sinngemäß.

### 3.2 Schicht 2: bei der Auswertung wie nicht vorhanden, aber laut

- **3.2.1** Eine Regel, deren Muster `validate` nicht besteht, verhält
  sich in `evaluate_rules_explained` so, als gäbe es sie nicht, **soweit
  ihr Muster nicht übersetzt**. Das ist für
  ein vollständig ungültiges Muster **genau das heutige Verhalten** (beide
  Wege liefern „passt nicht"). An der Entscheidungslogik ändert sich also
  nichts. Insbesondere:
  - Keine neue Entscheidung und kein neuer Entscheidungs-Code.
    `code_priority` bleibt unverändert.
  - **Einzelzweig-Fall:** Ein pfadförmiger Glob, bei dem nur **ein**
    Zweig nicht übersetzt, greift weiter über den Zweig, der übersetzt,
    genau wie heute (gemessen: `rm /x/[a/../b` → Deny, §1). Ihn ganz zu
    streichen, würde eine heute greifende Deny-Regel abschwächen, und das
    schließt `CLAUDE.md` aus („never loosen an existing check"). „Wie
    nicht vorhanden" gilt also für das, was nicht übersetzt, nicht für den
    Teil, der heute wirkt.
- **3.2.2** Jede Regel mit ungültigem Muster MUSS bei der Auswertung mit
  `tracing::error!` gemeldet werden. **Ort:** `evaluate_explained_inner`,
  direkt nach `let rules = self.store.rules_for(&scope).await;`
  (`engine.rs:195`). Dort läuft eine Schleife einmal über `rules`, ruft
  `validate` auf und loggt. `rules` wird unverändert weitergereicht. Nicht
  in der Bucket-Schleife: Die kehrt beim ersten Treffer zurück
  (`engine.rs:506-518`), würde also Regeln hinter dem Treffer nie
  melden, und läuft je Teilkommando mehrfach (`:302/307/391`). Gemeldet
  wird mit Regel-ID, Aktion und
  Fehlertext der Bibliothek (**eingeschränkt durch Q-BL-0249-03, §9:** statt
  des Fehlertexts der Bibliothek einer von drei festen, textfreien
  Kurztexten — der Fehlertext zitiert das Muster wörtlich, und ein Muster
  kann ein Geheimnis enthalten). Das **Kommando wird nicht geloggt**, sondern
  nur die Regel. Das Kommando könnte ein Geheimnis enthalten, und dieses
  Log ist eine neue Datensenke. Das gilt für alle drei Aktionen.
- **3.2.3** Die Regelliste markiert eine Regel mit ungültigem Muster sichtbar:
  - `RuleDto` bekommt ein Feld `patternError: Option<String>`, belegt aus
    `validate` beim Auflisten.
  - `FilterRulesView` zeigt an der Regel einen Hinweis mit dem Text aus
    3.1.6 und dem Fehlertext als Detail.
  - Bearbeiten und Löschen bleiben möglich. Speichern verlangt nach
    Schicht 1 ein gültiges Muster.
- **3.2.4** `Pattern::matches` und die `unwrap_or(false)`-Stellen in
  `pattern.rs` bleiben **wörtlich** stehen. Für `crate::risk` ändert sich
  nichts.

## 4. Design

- **Warum Schicht 1 die Lücke schließt:** Der gemessene Fund entsteht, wenn
  jemand eine Deny-Regel mit Tippfehler speichert. Schicht 1 verhindert
  genau das, beim Formular und bei der Schnellregel.
- **Was bleibt (bewusst, Entscheidung Stefan):** Eine Regel, die vor
  diesem Fix gespeichert wurde oder aus einer künftigen
  Organisations-Quelle kommt, kann weiter ein ungültiges Muster haben. Sie
  ist dann wirkungslos, wie heute. Eine Deny-Regel dieser Art neben einer
  breiteren Allow-Regel lässt das Kommando also weiter ohne Bestätigung
  durch. Sichtbar wird das über das Log (3.2.2) und über die Markierung in
  der Liste (3.2.3). Eine Ersatz-Eskalation wurde
  erwogen (Confirm für den ganzen Scope der Regel) und verworfen: Eine
  kaputte Regel soll nicht mehr bewirken als eine fehlende.
- **Warum `validate` in `core`:** Dieselbe Prüfung brauchen Schicht 1
  (`app-shell`) und das Log bzw. die Liste. Nach `CLAUDE.md` gehört Logik
  nach `core`, und `app-shell` übersetzt nur.
- **Verworfen:** Den Fehler in `Pattern::matches` zu `true` machen. Das
  würde auch Allow-Regeln und `crate::risk` treffen und eine ungültige
  Allow-Regel zu AutoExec machen.

## 5. Sicherheits-Invarianten

- **Nicht lockern:** Für jede Regelmenge und jedes Kommando ist die
  Entscheidung nach der Änderung **gleich** der heutigen. Schicht 2 ändert
  die Auswertung nicht (3.2.1), und Schicht 1 verhindert nur das
  Speichern. Tests: T-A6 und T-A12.
- **Allow-Regeln werden nicht weiter:** Sie verhalten sich unverändert
  wie heute. Im Einzelzweig-Fall greift auch eine Allow-Regel über den
  strengen Zweig (`pattern.rs:127`). Das bleibt so, denn die Auswertung
  wird nicht angefasst.
- **Hard-Blacklist und `crate::risk` bleiben unberührt**, ebenso ihre
  Reihenfolge vor den Nutzerregeln.
- **Neue Datensenke:** das Log aus 3.2.2. Dort landen nur Regel-ID,
  Aktion und Fehlertext des Musters, nie das Kommando (**eingeschränkt durch
  Q-BL-0249-03, §9:** auch der Fehlertext des Musters landet dort nicht mehr
  im Klartext, nur einer von drei festen Kurztexten).
- **Bekannte, hingenommene Restlücke:** eine schon gespeicherte ungültige
  Deny-Regel neben einer Allow-Regel (§4, „Was bleibt").

## 6. Tests

Jeder Test nennt, woran er bei einer falschen Umsetzung scheitert.

### 6.1 Schicht 1 (`app-shell`, `filter_rules`)

- **T-1** `create_rule` mit Regex `^systemctl stop (.*` →
  `FILTER_RULE_PATTERN_INVALID`, und `list_all` ist danach leer.
  *Scheitert heute* (die Regel wird gespeichert). Gegenbeweis Pflicht.
- **T-2** Dasselbe mit einem ungültigen Glob (`systemctl [stop`).
- **T-3** `update_rule` einer gültigen Regel auf ein ungültiges Muster →
  Fehler, und die gespeicherte Regel hat danach noch das **alte** Muster
  (per `get` geprüft). Scheitert, wenn erst geschrieben und dann geprüft
  wird.
- **T-4** Beide Einzelzweig-Richtungen aus §1 werden abgewiesen:
  `rm /x/[a/b]/../c` (nur der strenge Zweig scheitert) — scheitert, wenn
  `validate` nur `Glob::new` aufruft — und `rm /x/[a/../b` (nur der
  permissive Zweig scheitert) — scheitert, wenn `validate` nur den
  strengen Zweig baut. Beide Vorbedingungen (welcher Zweig übersetzt)
  hält der Test selbst als Zusicherung fest, damit er nicht still zu
  einem Test über ein durchweg ungültiges Muster wird.
- **T-5** Gültige Glob-, Regex- und Exact-Muster werden weiter angenommen.
  Die Stichprobe umfasst `*`, `**`, Klammern, Anker, Unicode und ein
  pfadförmiges Muster. Das fängt eine zu strenge Prüfung ab.
- **T-6** (Frontend, Vitest) `translateErrorCode` liefert für
  `FILTER_RULE_PATTERN_INVALID` in `de` und `en` einen Text, nicht den
  Rohcode. `localeKeyParity` bleibt grün. Ein Komponententest des
  Formulars zeigt bei diesem Code den übersetzten Satz **und** den
  `message`-Text. Scheitert, wenn nur einer von beiden erscheint.
- **T-6a** `create_quick_rule` mit einem ungültigen Glob → Fehler
  `InvalidPattern`, nichts gespeichert.
- **T-6e** (3.2.3) `list_rules` liefert für eine Regel mit ungültigem
  Muster `patternError` gesetzt und für eine gültige `None`. Ein
  Komponententest der Liste zeigt bei gesetztem `patternError` den Hinweis
  mit Fehlertext. Scheitert, wenn das Feld nicht belegt oder nicht
  angezeigt wird.
- **T-6d** Die Umwandlungsfunktion aus 3.1.3 liefert für ein ungültiges
  Muster `CommandError.code == Some("FILTER_RULE_PATTERN_INVALID")` und
  den Fehlertext der Bibliothek als `message`. Außerdem belegt der Coder
  im Bericht per grep, dass die drei Commands sie aufrufen. Scheitert,
  wenn der pauschale `From` den Code verschluckt.
- **T-6b** (3.1.5) `rule_suggestions` für das Kommando `ls [abc def`
  enthält keinen Vorschlag, der `validate` nicht besteht. Heute enthält es
  `ls [abc *`, und das übersetzt nicht (gemessen, §1).
- **T-6c** (in `app-shell`, Store über `connect` wie in
  `filter_rules.rs:97-106`, weil dort `tracing-subscriber` schon als
  Abhängigkeit da ist) Alte Datenbank, echt: eine Zeile mit ungültigem Regex als Deny
  wird per rohem `INSERT INTO filter_rules` angelegt, nach dem Muster in
  `persistence-sqlite/src/tests.rs:532`. Über `SqlitePolicyStore` als
  `PolicySource` und die echte `FilterEngine` ergibt sich mit einer
  Allow-Regel daneben dieselbe Entscheidung wie ohne die Regel (AutoExec).
  Die Auswertung bricht nicht ab, die übrigen Regeln greifen weiter, und
  es gibt ein Ereignis auf ERROR-Ebene mit der Regel-ID. `rules_for` lädt
  die Zeile weiter und verwirft sie nicht (`pattern_from_db` prüft das
  Muster nicht). Das ist der Akzeptanzfall „ältere Datenbank" aus dem
  Item. `list_rules` liefert für diese Zeile
  `patternError` gesetzt.

### 6.2 Fest eingebaute Muster

- **T-7** Alle eingebauten Muster übersetzen mit `Glob::new` bzw.
  `Regex::new`. Das ist genau die Übersetzung, die `Pattern::matches`
  macht, und nur die benutzen diese Listen. Der strenge Zweig wird hier
  **nicht** verlangt, weil ihn weder Blacklist noch `crate::risk`
  benutzen. Zwei Testorte, weil die Listen `pub(super)` sind:
  `filter/` für die Blacklist (`blacklist.rs:24`) und `risk/` für
  `server_risk_patterns`/`data_risk_patterns` (`risk/patterns.rs:226,
  344`). Scheitert, wenn ein eingebautes Muster still „passt nie" ist.

### 6.3 Schicht 2, adversarial (`core`, `filter/tests.rs`, echte `FilterEngine`)

Die ungültigen Regeln kommen über das `InMemoryPolicySource`-Double in die
Auswertung, an Schicht 1 vorbei (Fall „Organisations-Quelle"). Den Fall
„alte Datenbank" deckt T-6c ab.

- **T-A1** Allow `systemctl *` + Deny `^systemctl stop (.*` (ungültig),
  Kommando `systemctl stop nginx` → **AutoExec**, dasselbe wie mit Allow
  allein. Dazu ein ERROR-Ereignis mit der ID der Deny-Regel. Scheitert,
  wenn eine Ersatz-Eskalation eingebaut wird oder das Log fehlt.
- **T-A2** Wie T-A1 mit einem ungültigen Glob. **T-A3** wie T-A1 mit einer
  Confirm-Regel.
- **T-A4** Gültige Deny-Regel **und** ungültige Deny-Regel mit
  **niedrigerer Priorität**, Kommando passt auf die gültige → **Deny**
  durch die gültige Regel **und** ein ERROR-Ereignis für die ungültige.
  Scheitert, wenn eine ungültige Regel die Auswertung abbricht oder die
  übrigen Regeln überspringt, und wenn das Log in der Bucket-Schleife
  sitzt (3.2.2).
- **T-A5** Allow mit ungültigem Muster allein, Kommando `ls -la` →
  Confirm „keine Regel" wie bisher, nie AutoExec. Dazu ein ERROR-Ereignis
  mit der Regel-ID.
- **T-A6** Tabelle: Aktion {Allow, Confirm, Deny} × Muster {gültig
  passend, gültig nicht passend, ungültig, nur in einem Zweig ungültig}
  × Typ {Glob, Regex}, jeweils neben einer Allow-Regel, die passt. Für
  „nur in einem Zweig ungültig" die beiden gemessenen Muster aus §1; mit
  Regex entfällt diese Spalte (ein Regex hat keinen zweiten Zweig). Für
  jede Zeile ist die Entscheidung **gleich** der des unveränderten
  Stands. Der Test hält sie als erwarteten Wert fest, der vor der
  Änderung gemessen wurde.
- **T-A9** Der Regex `a{1000}{1000}` (gemessen: „Compiled regex exceeds size
  limit") als Deny-Regel → wie T-A1.
- **T-A10** Log: Die Auswertung aus T-A1 mit dem Kommando
  `systemctl stop nginx --password=hunter2` erzeugt ein Ereignis auf
  **ERROR-Ebene** mit der Regel-ID. Kein Ereignis auf ERROR-Ebene enthält
  `hunter2`. Nur ERROR-Ereignisse werden geprüft, denn
  `evaluate_explained` loggt das Kommando schon heute auf info
  (`engine.rs:149-155`), und das ist nicht Gegenstand dieser Spec.
  Aufgezeichnet wird mit einem Test-Subscriber. Dafür darf `core`
  `tracing-subscriber` als **dev-dependency** bekommen. Der Workspace
  benutzt es bereits (`ai-providers`, `app-shell`), das Muster steht in
  `ai-providers/src/test_support.rs:36-50`. Mit der Freigabe dieser Spec
  ist das erlaubt.
- **T-A12** Die gemessenen Einzelzweig-Fälle aus §1, jeweils mit Allow
  `rm *` daneben:
  - Deny `rm /x/[a/../b`, Kommando `rm /x/b` → weiterhin **Deny** mit
    `FILTER_RULE_DENY`, dazu ein ERROR-Ereignis mit der Regel-ID.
    Scheitert, wenn „wie nicht vorhanden" die ganze Regel streicht
    (3.2.1).
  - Deny `rm /x/[a/b]/../c`, Kommando `rm /x/a/../c` → **Deny** über den
    permissiven Zweig, dazu ein ERROR-Ereignis. Belegt 3.2.1 in der
    anderen Richtung als der erste Fall (dort trägt der strenge Zweig,
    hier der permissive).
  - Dieselbe Deny-Regel, Kommando `rm /x/c` → **AutoExec** wie heute
    (kein Zweig passt), mit ERROR-Ereignis. Scheitert, wenn eine
    Ersatz-Eskalation eingebaut wird oder das Log fehlt.

  Gegenbeweis Pflicht für die ersten beiden Fälle: rot gegen eine
  Fassung, die Regeln mit ungültigem Muster vor der Auswertung verwirft.

(T-A7, T-A8 und T-A11 der vorigen Fassung sind mit der Ersatz-Eskalation
entfallen. Die Nummern bleiben frei, damit Verweise stabil bleiben.)

## 7. Umsetzungsreihenfolge

Ein Coder-Lauf, **Opus** (Filter-Engine).

1. `core`: `Pattern::validate` plus `PatternError`, Tests T-5 (core-Teil)
   und T-7.
2. `core`: Log in der Auswertung (3.2.2), Tests T-A1 bis T-A12.
   Gegenbeweis für den ersten Fall von T-A12.
3. `app-shell`: Schicht 1 in `create_rule`/`update_rule`, Fehlertyp mit
   Code, Schnellregel und Vorschläge, Tests T-1 bis T-4 und T-6a bis
   T-6d. Gegenbeweis für T-1. `patternError` in `RuleDto` (3.2.3).
4. Frontend: Code, Übersetzungen, Anzeige im Formular, Markierung in der
   Liste, T-6 und T-6e.
5. spec-reviewer ERHÖHT, ADR (die Entscheidung „wie nicht vorhanden" und
   die Restlücke aus §4), Changelog-Fragment. Das Fragment beschreibt nur
   das sichtbare Verhalten („ungültige Muster werden beim Speichern
   abgewiesen"), nicht die Lücke.

Danach prüfe ich: regression-guard über die Range, silent-failure-hunter
im Diff-Modus, dann mein Gate.

**Veröffentlichung:** Das Item ist privat. Diese Spec wird erst
unmittelbar vor dem Coder-Lauf in `docs/specs/` des Repos committet. Sie
und der Fix gehen in **einem** Push hinaus. Vorher MUSS der ungepushte
Stand anderer Items gepusht sein, damit kein anderer Push sie vorzeitig
mitnimmt.

## 8. Offene Punkte (K3, Stefan)

Keine mehr. Entschieden am 2026-09-24:
1. Ungültige Regel bei der Auswertung wie nicht vorhanden (§1).
2. Markierung in der Regelliste: **ja** (3.2.3).
3. Eigenes Größenlimit für Regex: **nein** (Voreinstellung von `regex`
   genügt, T-A9).

## 9. Klarstellungen

(wird während der Umsetzung nachgetragen: Datum · Frage-ID · Antwort)

- **2026-09-24 · Q-BL-0249-01 · K1:** Die zweite Zeile der
  Einzelzweig-Tabelle in §1 war falsch. `rm /x/{a/..}/b` übersetzt in
  **beiden** Zweigen: `normalize_lexical_path` zerlegt das Token an `/`,
  das Segment heißt dort `..}` und gilt nicht als Aufwärts-Segment, das
  Muster bleibt also unverändert. Das in §1 beobachtete AutoExec stimmt,
  hat aber eine andere Ursache — das Muster ist gültig und passt schlicht
  nicht (`{a/..}` verlangt wörtlich `a/..`). Der echte Fall „nur der
  strenge Zweig scheitert" ist `rm /x/[a/b]/../c`: normalisiert
  `rm /x/[a/c`, offene `[` ohne Gegenstück. §1, T-4, T-A6 und T-A12
  benennen jetzt diesen Fall. **Unverändert:** 3.1.1 (die Prüfung selbst,
  beide Zweige), 3.2.1 und jede Aussage über das Verhalten des Produkts.
  Die Abdeckung wird größer statt kleiner — T-A12 belegt 3.2.1 jetzt in
  beide Richtungen und T-4 beide Richtungen von 3.1.1.

- **2026-09-24 · Q-BL-0249-02 · K2:** In der Regelliste sind die
  Pfeiltasten ↑/↓ an einer Regel mit `patternError` deaktiviert, mit einem
  `title`, der den Grund nennt. `movePriority` bricht **vor dem ersten**
  `updateRule` ab, wenn eine der beiden Regeln `patternError` trägt. 3.1.2
  bleibt wörtlich: Jeder Schreibweg prüft. Test: Komponente mit einer
  markierten Regel. Die Pfeile sind deaktiviert, und `updateRule` wird
  nicht aufgerufen, auch nicht für die Nachbarregel.
- **2026-09-24 · Q-BL-0249-03 · Stefan:** Der Log-Eintrag aus 3.2.2
  enthält **weder das Muster noch einen Fehlertext, der es zitiert**. Die
  Fehlertexte von `regex` und `globset` zitieren das Muster wörtlich,
  deshalb stehen im Eintrag Regel-ID, Aktion und ein fester Kurztext je
  Fall (etwa „regex does not compile“, „glob does not compile (strict
  branch)“). `patternError` im DTO und die Anzeige in der Regelliste
  bleiben unverändert. Test: ein ungültiges Muster, das ein Geheimnis
  enthält. Kein ERROR-Ereignis enthält das Geheimnis. Der Test scheitert
  gegen `ee017af`.
