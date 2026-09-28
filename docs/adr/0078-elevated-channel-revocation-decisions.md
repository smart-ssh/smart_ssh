# ADR 0078 — Entscheidungen beim Abbruch laufender Befehle bei Widerruf (Spec 0085, Teil 1)

Status: angenommen · 2026-09-28 · Spec: `docs/specs/0085-elevated-channel-revocation-follow-ups.md`
Betrifft: `crates/app-shell/` (`commands/elevation.rs`, `commands/sftp.rs`,
`elevated_sftp.rs`), `crates/app-logic/` (`session.rs`), `crates/core/`
(ein Kommentar), `crates/ssh-transport/` (nur Tests)

Gilt für **Teil 1** der Spec (A1, A2, A3). Teil 2 (A4, A5, Changelog-Fragment)
läuft separat.

## 1. Die Sperre des erhöhten Kanals wird je Operation genommen, nicht je Befehl

A1.3 verlangt: „Deaktivieren wartet höchstens auf die gerade laufende einzelne
SFTP-Operation, nicht auf den Rest des Befehls." Vorher hielt ein Befehl die
Kanal-Sperre über seine gesamte Dauer — bei einer Rekursion über viele
Einträge also über alle Operationen hinweg. Die Umstellung ist damit nicht
optional, sie ist die Anforderung.

**Folge, die die Spec nicht abwog und die hier festgehalten wird:** Zwei
gleichzeitige erhöhte Browser-Befehle können ihre Operationen jetzt
verschränken. Vorher war das innerhalb der App ausgeschlossen. Konkret
verschränkbar (vom spec-reviewer, Runde 1, benannt):

- `sftp_read_text` prüft die Größe per `stat` und liest danach — wächst die
  Datei dazwischen, greift die Grenze `MAX_TEXT_PREVIEW_BYTES` nicht.
- `delete_recursive` zählt/listet in einem ersten Lauf und löscht im zweiten —
  kommt dazwischen ein Eintrag hinzu, wird entweder etwas gelöscht, das in der
  Vorschau nicht stand, oder `remove_dir` scheitert mit „nicht leer".
- `chmod_recursive` sammelt Pfade und setzt danach — ein zwischenzeitlich
  gelöschter Pfad lässt den Lauf mit Teilergebnis scheitern.

Geprüft und unkritisch: der Zip-Slip-Schutz (`safe_local_segment` arbeitet auf
den Namen genau der Liste, die unter derselben Sperre gelesen wurde), der
`lstat`-Symlink-Schutz aus Spec 0054 (ein nachträglich untergeschobener
Symlink wird als Link entfernt, nie verfolgt) und die Reihenfolge
bottom-up/tiefste-zuerst innerhalb eines Befehls.

Keiner dieser Fälle ist eine Rechte-Eskalation: alles läuft unter demselben
Ziel-Nutzer, den der Nutzer eingeschaltet hat. Ein fremder Prozess auf dem
Server konnte dieselben Rennen immer schon auslösen. Nicht behoben, weil ein
Fix Verhalten ändern würde, das die Spec nicht beschreibt (s. §4).

## 2. Eine Widerrufsprüfung, unter der Sperre — nicht zwei

Zuerst standen zwei Prüfungen da: eine vor dem Warten auf die Kanal-Sperre
(damit ein schon widerrufener Zugriff nicht erst wartet) und eine danach. Der
spec-reviewer zeigte, dass die zweite — die sachlich entscheidende, weil
`tokio::sync::Mutex` fair ist und ein wartender Zugriff sonst noch **vor** dem
Widerruf an die Reihe käme — von keinem Test erreichbar war: die erste fing
jeden Fall vorher ab.

Entschieden: die Vorprüfung entfällt. Sie sicherte nichts, was die Prüfung
unter der Sperre nicht sichert (dort liegen Prüfen und Benutzen in derselben
kritischen Sektion), sie sparte nur Wartezeit — und sie verdeckte die
eigentliche Prüfung vor jedem Test. Verworfen wurde die Alternative, die
Vorprüfung zu behalten und für das verbleibende Fenster einen Test mit zwei
gleichzeitigen Befehlen zu bauen: dafür hätte es einen zweiten test-only
Haltepunkt gebraucht, und eine Prüfung, die nur unter sehr eng gestellten
Umständen greift, bleibt schwer prüfbar.

## 3. Wie die Lückenlosigkeit hergestellt ist (A1.1)

Der Zugang zum erhöhten Kanal gibt keinen rohen Kanal mehr heraus, sondern
einen Wert, der selbst `SftpSession` ist (`BrowserSftp`) und vor **jeder**
Operation prüft. Dadurch gilt die Prüfung auch für Befehle mit mehreren
Operationen (`sftp_read_text`, `sftp_open_for_editing`) und für Befehle, die
es noch nicht gibt, ohne dass jemand daran denken muss.

Zwei Geländer dazu, beide auf Hinweis des spec-reviewers:

- Der Wrapper schreibt alle zehn Trait-Methoden aus, statt sie per Makro zu
  erzeugen. `#[async_trait]` expandiert ohnehin vor `macro_rules!`; unabhängig
  davon soll an dieser Stelle jede Operation im Quelltext stehen.
- Die Zusage „eine neue Trait-Methode erzwingt hier einen Eintrag" gilt nur,
  solange `SftpSession` keine Default-Rümpfe hat — `SshTransport` hat welche.
  Deshalb steht ein Hinweis am Trait selbst.
  **Abweichung von der Spec:** §4 sagt „`crates/core` wird nicht geändert".
  Das bezieht sich auf die Übersetzung des Abbruchs; hier ist es ein
  Kommentar, keine Verhaltensänderung. Sichtbar gemacht statt still getan.

## 4. Bewusst nicht behobene Funde aus Runde 1

Alle vier sind vom Reviewer als „Blocker: nein" bzw. „zurückstellbar"
eingestuft.

- **Größengrenze in `sftp_read_text` nicht TOCTOU-fest** (s. §1). Ein Fix
  (Größe nach dem Lesen erneut prüfen) wäre billig und würde nur verschärfen,
  ändert aber das Verhalten eines Befehls, das die Spec nicht beschreibt: eine
  Datei, die zwischen `stat` und `read_file` über die Grenze wächst, würde
  künftig abgelehnt statt kopiert. Das ist eine Produktentscheidung, keine
  Ableitung — gehört also vorgelegt, nicht selbst getroffen. Empfehlung: als
  eigenes Backlog-Item, zusammen mit dem Altbestand, dass
  `sftp_open_for_editing` gar keine Grenze hat.
- **Das Ereignis `sftp-transfer-finished` trägt beim Widerruf den Wortlaut mit
  `Channel-Fehler: `-Präfix**, das Befehlsergebnis dagegen wörtlich
  `ELEVATED_CHANNEL_INACTIVE`. A1.2 bindet nur das Befehlsergebnis; die
  Meldung in der Transferliste ist nicht falsch, nur länger. Ein Angleichen
  wäre neuer Nutzertext an einer Stelle, die die Spec nicht nennt
  (§2, Nicht-Ziel „kein neuer Nutzertext").
- **Kein Doctest-Zwillingspaar für den Tausch ganzer `Session`- oder
  `SessionParts`-Werte.** Der Tausch zweier ganzer `Session`-Werte ist für
  jeden Rust-Typ möglich und in sich schlüssig (A wird vollständig B, kein
  Kanal landet bei einem fremden Transport). Der teilweise Tausch über
  `DerefMut` erzeugt zwar ein Gespann aus fremdem Transport und eigenem Kanal
  — genau das war aber vorher schon möglich, weil alle Felder `pub` waren und
  es geblieben sind (`mem::swap(&mut a.transport, &mut b.transport)`). A3.1
  richtet sich gegen das Einsetzen eines **fremden Kanals**, und dafür gibt es
  von außen keinen Weg mehr: `install` ist `pub(crate)`, die Testhilfe hängt
  am Feature `test-support`.
- **Der Abbruch-Vermerk schlägt auch auf ein `Ok(None)` durch:** Wird der
  Größen-Vorablauf in `sftp_download` durch einen Widerruf abgebrochen und
  bricht der Nutzer danach den Speichern-Dialog ab, kommt ein Fehler statt
  „nichts passiert". §4 der Spec verlangt die Übersetzung ausdrücklich „auch
  dann, wenn der Befehl den SFTP-Fehler selbst abfängt"; die Aussage ist wahr
  (der erhöhte Modus war wirklich nicht mehr aktiv). Eine Ausnahme dafür wäre
  genau die „eigene Regel je Befehl", die §4 verbietet.

## 5. `Session` wird zweiteilig, statt 33 Felder zu spiegeln (A3)

A3.1 verlangt, dass Code außerhalb von `app-logic` den normalen SFTP-Kanal
weder setzen noch ersetzen noch herausnehmen kann. Ein privates Feld ist dafür
nötig (nur die Sichtbarkeit des Felds verhindert `mem::swap` zwischen zwei
Sitzungen; am Typ der herausgegebenen Referenz hängt der Rest). Ein privates
Feld verbietet aber das Struct-Literal von außen, also braucht es einen
Konstruktor.

Gewählt: die bisherige Struktur heißt `SessionParts` (reine Umbenennung, alle
Doc-Kommentare bleiben, wo sie waren), und `Session` enthält sie plus das
private Kanal-Feld, mit `Deref`/`DerefMut` auf `SessionParts`. Damit
funktioniert `session.<feld>` an mehreren hundert Stellen unverändert, und die
Umstellung kostet fünf Konstruktionsstellen.

Verworfen: ein Parameter-Struct, das die 33 Felder ein zweites Mal deklariert.
Das wären rund 75 Zeilen Doppelpflege für dieselbe Wirkung, mit der üblichen
Folge, dass die Kopie irgendwann abdriftet.

Bekannter Preis: `Deref` auf einen Nicht-Zeiger ist unüblich und macht
Fehlermeldungen etwas indirekter. Eine Stelle musste weichen: das
Struct-Update (`..base`) in `orchestration::test_support::
session_with_second_opinion` setzt seine zwei Felder jetzt nach dem Bau.

## 6. Der Nachweis für A3 steht als Doctest

T11 verlangt den Nachweis „aus Sicht eines **anderen** Crates" und „ein Test
im Gate". Ein Doctest erfüllt beides ohne neue Abhängigkeit: `rustdoc` baut
ihn als eigene Kiste gegen `app_logic`, und `cargo test --workspace` führt ihn
mit. Fünf verbotene Fälle, je mit kompilierendem Zwilling.

Gemessen und festgehalten: die Codeangabe an ```` ```compile_fail,E0616 ````
wird von `rustdoc` **nicht** erzwungen (ein absichtlich falscher Code läuft
durch). Deshalb steht sie nicht im Code — der Zwilling ist die Absicherung
dagegen, dass ein Fall nur an einem Tippfehler scheitert.

Verworfen: `trybuild` (neue Abhängigkeit) und ein Test, der `cargo check` auf
eine eigens angelegte Kiste absetzt (verschachtelter Cargo-Aufruf im
Testlauf, langsam, und `app-shell` zu prüfen kostet Minuten).

## 7. Zwei test-only Haltepunkte in einem sicherheitskritischen Pfad

`ElevatedSftpSlot::before_operation_hook` (`#[cfg(test)]`) gibt es, weil zwei
Verschränkungen sonst nicht deterministisch zu treffen sind: `sftp_exists`
führt genau **eine** SFTP-Operation aus (T6c), und das Fenster „Zugang
angefordert, Sperre noch nicht bekommen" bräuchte sonst einen zweiten
gleichzeitigen Befehl. Vorbild ist `ElevatedSftpRegistry::interleave_hook` aus
Spec 0084 (T8b), dieselbe Begründung. Im Produktivbau existiert das Feld
nicht; `run_before_operation_hook` bleibt dort als leerer Rumpf.

## 8. Audit-Mitschnitt in T7 ohne neue Abhängigkeit

T7 verlangt „T1 und T2 mit erfasster Log-Ausgabe" und erlaubt als Rückfall,
stattdessen die Eingaben der Audit-Funktion zu prüfen, falls ein
Test-Subscriber nur für einen Thread gilt. Der Rückfall war nicht nötig:
`#[tokio::test]` fährt eine Ein-Thread-Laufzeit, die auch die per
`tokio::spawn` gestarteten Tasks auf demselben Thread abarbeitet — ein über
`tracing::subscriber::set_default` gesetzter Thread-Default erfasst sie also
mit. Der Subscriber ist von Hand geschrieben (rund 25 Zeilen, nur
Ereignisfelder): `app-shell` hat `tracing-subscriber` nicht als Abhängigkeit,
und für „welche Felder hatte das Ereignis" braucht es nichts weiter. Kein
Produktivcode wurde für den Mitschnitt angefasst.

## 9. Was Spec 0084 §9 jetzt genau heißt

Spec 0084 §9 sprach davon, dass Deaktivieren wartet, „bis ein laufender
Vorgang fertig ist". Ab hier ist mit „Vorgang" eine **einzelne
SFTP-Operation** gemeint, nicht ein ganzer Befehl (Spec 0085, A1.3). Der Test
`test_a_command_already_waiting_on_the_channel_fails_after_the_user_switched`
aus 0084 bleibt wörtlich erhalten und weiter wirksam, prüft aber seit dieser
Umstellung einen kürzeren Weg: `lock_browser_sftp` nimmt die Kanal-Sperre
nicht mehr, das Warten passiert je Operation. Der Fall „Zugriff wartet, dann
wird widerrufen" ist seither vom neuen Regressionstest
`test_a_revocation_while_an_access_waits_for_the_lock_still_stops_it`
abgedeckt.
