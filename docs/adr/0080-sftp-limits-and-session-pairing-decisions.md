# 0080-sftp-limits-and-session-pairing-decisions

## Status
Accepted

## Kontext

Spec 0086 fasst vier Punkte zusammen: Größengrenzen, die auch nach dem Lesen
halten (A1), die Transfer-Meldung beim Widerruf des erhöhten Modus (A2), die
Paarung von Transport und normalem SFTP-Kanal einer Sitzung (A3) und
Kommentar-Verweise auf Module, die es nicht gibt (A4). Dieses ADR hält die
Entscheidungen fest, die die Spec offen gelassen hat, dazu die von ihr
ausdrücklich verlangten Punkte (§4, A3.4, T5) und die Funde des
spec-reviewers, die bewusst nicht behoben wurden.

## Entscheidung 1 (A3): `DerefMut` für `Session` entfällt ganz — nicht nur im Produktivbau

Die Spec verlangt (A3.1), dass Code außerhalb von `app-logic` den Transport
einer bestehenden Sitzung weder ersetzen noch entnehmen noch mit dem einer
anderen tauschen kann, und zwar auf jedem Weg — auch über einen Tausch aller
mitgegebenen Bestandteile. Beides läuft über `DerefMut`: ohne ihn ist von
außen kein Feld einer `SessionParts` schreibbar, und `mem::swap(&mut **a,
&mut **b)` scheitert am Typ.

**Gewählt:** `DerefMut` wird **unbedingt** entfernt. Tests bekommen den
Schreibzugang über `Session::parts_mut_for_tests` hinter
`cfg(any(test, feature = "test-support"))` (A3.3 erlaubt das ausdrücklich,
Muster wie `set_sftp_for_tests`). 168 Teststellen wurden von
`session.feld = x` auf `session.parts_mut_for_tests().feld = x` umgestellt —
dieselbe Zahl, die Spec §1 gemessen hat. (`grep` findet 169 Nennungen; die
169. steht im Doc-Kommentar von `parts_mut_for_tests` als Beispiel.)

**Verworfen:** `DerefMut` hinter `cfg(any(test, feature = "test-support"))`
stehen zu lassen. Das hätte die Testumstellung erspart, wäre aber eine Falle:
Gemessen ist das Feature `test-support` von `app-logic` im **Doctest-Lauf von
`cargo test --workspace` aktiv** (Feature-Unification über `app-shell`s
dev-dependency), bei `cargo test -p app-logic --doc` dagegen nicht. Die
`compile_fail`-Fälle (a)–(d) aus T11 hätten damit genau in dem Lauf, der das
Gate bildet, **leer bestanden** — sie hätten übersetzt, und der Doctest hätte
mit „compiled successfully, expected failure" gemeldet oder, bei anderer
Formulierung, gar nichts geprüft. Eine Zusage, die nur außerhalb des Gates
gilt, ist keine.

**Verworfen:** Den Transport als privates Feld an `Session` zu legen (wie den
normalen SFTP-Kanal) und `DerefMut` zu behalten. Das hätte (a), (b), (d) und
(e) geschlossen, aber nicht (c): Ein `mem::swap(&mut **a, &mut **b)` hätte
weiter übersetzt und alle übrigen Bestandteile zweier Sitzungen getauscht —
`server_id`, `context`, `redactor`, `filter_engine` — während Transport und
Kanal stehen bleiben. Das ist dieselbe Art von Fehlpaarung, nur umgekehrt
herum, und T11 verlangt für (c) ausdrücklich `compile_fail`.

## Entscheidung 2 (A3): der Transport bekommt einen eigenen Typ mit Guard

`SessionParts.transport` ist nicht mehr `AsyncMutex<Box<dyn SshTransport>>`,
sondern `SessionTransport` mit privatem Innerem. `SessionTransport::lock`
gibt einen `TransportGuard` heraus, der `Deref`/`DerefMut` auf
`dyn SshTransport + 'static` implementiert — auf das **Trait-Objekt**, nicht
auf die `Box`. Damit scheitern `mem::replace`/`swap`/`take` über die Sperre an
`T: Sized`, während alle Methoden von `SshTransport` unverändert erreichbar
bleiben. Das war nötig, weil der Transport schon mit `&Session` sperrbar ist,
also auch hinter dem `Arc` in der Registry: der alte
`MutexGuard<Box<dyn SshTransport>>` gab `&mut Box<…>` heraus, und damit war
der Wert ersetzbar, ohne `&mut Session` zu besitzen.

`DerefMut` auf dem Guard statt einer Methode `transport(&mut self)` (Muster
`NormalSftpGuard::sftp`): Die Spec empfiehlt das in §4, und es hält die
sieben Sperrstellen im Produktivcode unverändert — `guard.execute(…)`
funktioniert weiter. Die Schutzwirkung ist dieselbe, sie hängt am Typ der
herausgegebenen Referenz, nicht am Zugriffsweg.

**Bedingung für die Zukunft (spec-reviewer, Runde 1):** Diese Zusage hängt
daran, dass `SshTransport` **kein** `Any` als Supertrait und keine
`as_any`-Methode hat. Gäbe es die, ließe sich das Trait-Objekt auf den
konkreten Typ herunterwerfen und dann als `&mut Concrete` (dann `Sized`)
überschreiben. Kein `compile_fail`-Fall würde das bemerken. Wer `SshTransport`
um einen `Any`-Zugang erweitert, muss die Fälle an `SessionTransport::lock`
neu bewerten.

## Entscheidung 3 (A3.4): die Zwillinge der `lock_sftp`-Fälle

Die vier `compile_fail`-Fälle aus Spec 0085 A3.1 (`session.sftp = …`,
`mem::replace`, `mem::swap`, `mem::take` am Feld) bleiben unverändert. Ihre
kompilierenden Zwillinge schrieben bisher ein anderes, öffentliches Feld
(`session.tags = …`) — das ging nur über `DerefMut` und übersetzt jetzt nicht
mehr. A3.4 erlaubt die Anpassung und verlangt, dass sie hier steht.

**Gewählt:** Die Zwillinge führen dieselbe Mechanik an einem **lokalen**
`NormalSftpChannel` vor (`local = Default::default()`,
`mem::replace(&mut local, …)`, `mem::swap(x, y)`, `mem::take(local)`). Der
einzige Unterschied zur verbotenen Zeile ist damit wieder der Feldzugriff, und
der Zwilling belegt, dass `Default`/`replace`/`swap`/`take` für diesen Typ
sehr wohl übersetzen.

**Verworfen:** Zwillinge, die bloß `session.lock_sftp()` aufrufen (die erste
Fassung dieses Laufs). Sie erfüllen A3.4 wörtlich — genau eine Zeile
unterscheidet sich —, hätten aber Beweiskraft verloren: Fiele eines Tages
`#[derive(Default)]` an `NormalSftpChannel` weg, bestünden die Fälle
„Zuweisung ans Feld" und „`mem::take` am Feld" still aus dem falschen Grund.
Fund des spec-reviewers, Runde 1; behoben. Nachgewiesen wurde die
Beweiskraft, indem das Feld `sftp` vorübergehend `pub` gemacht wurde: dann
übersetzen genau diese vier Fälle, der fünfte (unsized `mem::swap` über die
Guard-Referenz) weiter nicht.

## Entscheidung 4 (A3.3): ein breiter Test-Zugang, kein Satz feldweiser Setter

`Session::parts_mut_for_tests` gibt `&mut SessionParts` heraus, also
Schreibrechte auf **alle** mitgegebenen Bestandteile — auch auf
`filter_engine`, `ai_provider` und `sudo_password`. Ein Aufruf aus
Produktivcode wäre ein vollständiger Filter-Bypass.

**Gewählt:** Der breite Zugang bleibt, mit zwei Schranken. Erstens das
Feature: kein Produktivbau aktiviert `test-support`, `app-shell` setzt es nur
unter `[dev-dependencies]`. Zweitens der Name: `_for_tests` macht einen
Aufruf im Produktivcode einem Menschen sofort auffällig.

**Verworfen:** Ein Satz feldweiser Setter (`set_server_id_for_tests`, …),
wie der spec-reviewer es als Härtung vorschlägt. Er wäre enger, ist aber
nicht Teil von Spec 0086 — A3.3 nennt genau diesen Weg als zulässig — und
träfe 168 Aufrufstellen. Eine Scope-Erweiterung dieser Größe in einem Lauf,
der eine Sicherheits-Invariante umbaut, erhöht das Risiko mehr, als sie es
senkt. **Bleibt offen** und gehört in den Backlog.

**Zur Kenntnis:** Das lokale Gate aus `CLAUDE.md` (fmt, clippy
`--all-targets`, test) hat das Feature durchgängig an und würde einen
Produktiv-Aufruf grün durchlassen. Abgelehnt wird er erst von
`cargo build --workspace` — das läuft in CI, steht aber nicht im lokalen Gate.
In diesem Lauf wurde `cargo build --workspace` deshalb zusätzlich von Hand
gefahren. Die `CLAUDE.md` selbst wurde **nicht** geändert: das ist eine
Prozessänderung außerhalb von Spec 0086. Backlog-Kandidat.

## Entscheidung 5 (A1.2/T5): die Grenze von „Lokal öffnen" ist ein Parameter

`open_for_editing_impl` nimmt `max_bytes: u64`, der Produktivpfad
(`sftp_open_for_editing`) gibt immer `MAX_EDIT_OPEN_BYTES` (50 MiB) mit.

Grund: Die Prüfung **nach** dem Lesen (Datei ist zwischen `stat` und
`read_file` gewachsen) ließe sich sonst nur mit einem echten 50-MB-Puffer je
Testfall abdecken. T5 erlaubt den Parameter ausdrücklich und verlangt die
Begründung hier. Geprüft wird der Vergleich, nicht die Zahl; dass die Zahl
stimmt, prüft die Vorab-Variante von T6 mit der echten Konstante (`stat`
meldet genau 50 MiB) plus ein Test, der Meldungstext und Konstante aneinander
bindet.

Die Meldung `TOO_LARGE_FOR_EDITING` ist ein fester Text und wird **nicht**
aus `max_bytes` formatiert: sonst nannte sie in Tests eine Grenze, die es im
Produktivbetrieb nicht gibt, und A1.2 gibt den Wortlaut ohnehin vor. Damit
Text und Konstante nicht auseinanderlaufen, bindet
`test_the_rejection_message_names_the_actual_limit` beide aneinander (Fund
des spec-reviewers, Runde 1; behoben).

`MAX_EDIT_OPEN_BYTES` ist eine eigene Konstante ohne gemeinsamen Code mit
`MAX_TEXT_PREVIEW_BYTES` oder dem KI-Lesepfad (A1.4). Das sind zwei
verschiedene Fragen — was in die Zwischenablage passt, und was in einen
lokalen Editor geladen wird —, und ein gemeinsamer Pfad hieße, dass eine
manuelle Aktion über KI-Infrastruktur läuft (Spec 0054,
Sicherheitsmodell).

## Entscheidung 6 (A2): die Übersetzung sitzt am Kanal, nicht im Transfer-Kern

`BrowserChannel::transfer_error_message` bildet den Text für das
`error`-Feld von `sftp-transfer-finished`: ist der Widerrufs-Vermerk des
Befehls gesetzt, `ELEVATED_CHANNEL_INACTIVE`, sonst der Fehlertext unverändert.

Sie liegt in `commands/elevation.rs`, neben `with_browser_channel`, und liest
**denselben** Vermerk. Damit gibt es weiter nur eine Quelle der Wahrheit für
„dieser Befehl wurde widerrufen" und keine Regel je Befehl (ADR 0078 §4). Die
Spec lässt den Weg frei (§4) und nennt diesen als naheliegend.

**Stille Annahme, die dieser Weg macht (spec-reviewer, Runde 1):** Der
Vermerk wird nie gesetzt, ohne dass die betroffene Operation im selben Zug
fehlschlägt. Heute gilt das — er wird ausschließlich in der
Vor-Operations-Prüfung gesetzt, immer gemeinsam mit einem sofortigen `Err`.
Ein künftiger Transfer-Befehl, der einen Widerrufsfehler abfängt und
weiterläuft, würde den **nächsten** Fehler fälschlich als „erhöhter Modus
nicht mehr aktiv" beschriften. Ereignis und Befehlsergebnis blieben dabei
konsistent (Spec 0085 A1.2 übersetzt das Ergebnis genauso), die Meldung wäre
aber sachlich falsch. Wer einen solchen Befehl schreibt, muss diese Stelle
mitprüfen.

`UserChanged` (erhöhter Modus läuft als anderer Nutzer) setzt den Vermerk
nicht und behält seinen Text mit Präfix — so gewollt, Nicht-Ziel in §2.

## Entscheidung 7 (T8): die Transfer-Kerne nehmen den Emitter als Parameter

`download_one_file` und `upload_impl` bauten ihren Emitter aus einem
`AppHandle` selbst. Dadurch war kein Test in der Lage, das gesendete Ereignis
zu prüfen — genau deshalb blieb der Präfix-Fehler unbemerkt. Beide nehmen
jetzt `&dyn EventEmitter`; die Zwischenfunktionen `download_recursive` und
`download_entry_to` ebenso, die vier Produktiv-Aufrufstellen übergeben
`&TauriEventEmitter(app.clone())` — derselbe Emitter wie vorher. Keine
Testsonderbahn, `EventEmitter` ist die Abstraktion, die der Produktivpfad
ohnehin benutzt.

## Bewusst nicht behoben

- **`app_logic::orchestration::SIDE_CALL_MAX_TOKENS` ist `pub(crate)`**
  (spec-reviewer, Runde 1). Zwei Kommentare in `ai-providers` nennen diesen
  Pfad; das Element existiert dort, ist aus `ai-providers` aber nicht
  erreichbar. A4.1 verlangt, dass ein Verweis „auf ein Element zeigt, das es
  am genannten Ort gibt" — nicht, dass es von der verweisenden Kiste aus
  aufrufbar ist. Ein Doc-Kommentar ist eine Orientierungshilfe für Menschen;
  der Verweis ist jetzt richtig statt, wie vorher, auf ein nicht existierendes
  Modul zu zeigen. Keine Änderung.
- **Der Kreis `ai_providers::test_support` ↔ `ssh_manager_core::filter::tests`**
  (spec-reviewer, Runde 1) ist aufgelöst: Die `set_global_default`/`Once`-
  Begründung steht vollständig in `ai_providers::test_support`, der Verweis
  von dort auf `filter::tests` ist nur noch als weiteres Beispiel
  gekennzeichnet.

  **Korrektur (spec-reviewer, Runde 2):** Die erste Fassung dieses ADR
  behauptete, damit stünde die Begründung dort vollständig. Das war falsch —
  die **zweite**, unabhängige Begründung („warum global statt `with_default`":
  `tracing-core` cacht das Callsite-Interesse prozessweit) war beim Entfernen
  des Verweises verlorengegangen und stand nur noch in
  `app_logic::test_support::log_capture`. A4.2 verlangt, einen Verweis mit
  existierendem Ziel **umzuschreiben** statt zu löschen; das Ziel existiert
  (`crates/app-logic/src/test_support.rs`, `mod log_capture`). Der Verweis
  zeigt jetzt dorthin und nennt den Kern der Begründung mit, damit sie auch
  ohne Sprung lesbar ist. Behoben.

- **Zwei Doctest-Signaturen in `crates/app-logic/src/session.rs` sind 101
  Spalten breit** (spec-reviewer, Runde 2; `cargo fmt` formatiert
  Doc-Codeblöcke nicht). Nicht umgebrochen — und zwar nicht aus Bequemlichkeit:
  Die Beweiskraft der Zwillinge (Entscheidung 3) hängt daran, dass verbotener
  Fall und Zwilling **identische** `use`-Zeile und identische Signatur haben.
  Nur deshalb kann ein `compile_fail`-Fall nicht aus einem anderen Grund als
  dem Feldzugriff scheitern: jeder Import- oder Typfehler macht den
  kompilierenden Zwilling rot, statt den verbotenen Fall leer bestehen zu
  lassen. Einen der beiden Blöcke umzubrechen, hieße diese Parität für eine
  Spaltenbreite aufzugeben. Zwei Zeichen zu breit, dafür eine tragende Zusage
  — die Wahl ist eindeutig.

## Konsequenzen

- Von außen ist kein Feld einer bestehenden `Session` mehr schreibbar. Neuer
  Code in `app-shell`, der ein Session-Feld setzen will, muss entweder eine
  ganze Sitzung ersetzen (Transport und Kanal wandern zusammen) oder die
  Änderung nach `app-logic` verlagern. Das ist gewollt.
- `SessionParts` bleibt mit `pub`-Feldern konstruierbar; nur das nachträgliche
  Schreiben ist weg. `connect()` in `app-shell` baut die Sitzung unverändert.
- „Lokal öffnen" lehnt Dateien über 50 MiB ab. Für Nutzer, die bisher eine
  sehr große Datei lokal geöffnet haben, ist das eine Verhaltensänderung
  (Changelog-Fragment `changelog.d/0086-sftp-size-limits.md`).
- Offen und im Backlog: feldweise Test-Setter statt
  `parts_mut_for_tests` (Entscheidung 4), und `cargo build --workspace` im
  lokalen Gate.
