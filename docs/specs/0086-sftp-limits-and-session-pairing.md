# Spec 0086 — Dateibrowser: Größengrenzen nach dem Lesen, Transfer-Meldung beim Widerruf, Transport-Kanal-Paarung, Doku-Verweise

Status: **freigegeben** (Stefan, 2026-09-28) · Backlog: BL-0274, BL-0275, BL-0276, BL-0277 · Gate: —
Repo: **öffentlich** `smart-ssh` — `crates/app-shell/`, `crates/app-logic/`,
Kommentare in weiteren Crates (A4)
Review-Priorität: **ERHÖHT** für A3 (Session-Aufbau, normaler SFTP-Kanal,
Spec 0067 A, Spec 0085 A3), **normal** für A1, A2, A4
Zweck: Die Größengrenze der Vorschau hält auch, wenn die Datei zwischen
Prüfung und Lesen wächst, und „Lokal öffnen" bekommt eine eigene Grenze.
Die Transferliste zeigt beim Widerruf denselben Text wie der Befehl. Der
Transport einer Sitzung lässt sich von außen nicht mehr getrennt von ihrem
normalen SFTP-Kanal austauschen. Kommentar-Verweise auf `app_shell::…`
zeigen wieder auf existierende Stellen.

## 1. Ausgangslage (gemessen, Stand `f62c8eb`)

- **Vorschau (A1).** `read_text_impl` (hinter `sftp_read_text`) prüft die
  Größe per `stat` gegen `MAX_TEXT_PREVIEW_BYTES` (256 KB) und liest danach
  mit `read_file`. Seit Spec 0085 (A1.3) wird die Kanal-Sperre je Operation
  genommen, dazwischen kann ein anderer Befehl laufen (ADR 0078 §1). Wächst
  die Datei zwischen `stat` und `read_file`, wird sie ganz gelesen und
  zurückgegeben. Scheitert `stat`, wird ohne jede Prüfung gelesen (so
  gewollt, s. Doc-Kommentar von `sftp_read_text`).
- **„Lokal öffnen" (A1).** `open_for_editing_impl` (hinter
  `sftp_open_for_editing`) liest mit `stat` und `read_file` ohne jede
  Größengrenze und schreibt die Datei ins Editier-Temp-Verzeichnis.
  `read_file` liefert die ganze Datei als `Vec<u8>`.
- **Transfer-Meldung (A2).** `download_one_file` und der Upload-Kern
  senden `sftp-transfer-finished` mit `e.message` des Fehlers aus ihrem
  Rumpf. Ein Widerruf, der erst bei der einzelnen Operation greift (nach
  dem Erstzugang), kommt dort als `SshError::ChannelError(<Text>)` an,
  dessen `Display` das Präfix `Channel-Fehler: ` vorsetzt
  (`crates/core/src/ssh/error.rs`). Das Befehlsergebnis wird danach in
  `with_browser_channel` zentral auf `ELEVATED_CHANNEL_INACTIVE` gesetzt,
  das Ereignis ist dann schon gesendet. Folge: Ereignis „Channel-Fehler:
  Der erhöhte Modus ist nicht mehr aktiv …", Befehl „Der erhöhte Modus ist
  nicht mehr aktiv …".
- **Paarung Transport/Kanal (A3).** `Session` besteht aus `SessionParts`
  (alle Felder `pub`, darunter `transport: AsyncMutex<Box<dyn
  SshTransport>>`, tokio) und dem privaten normalen SFTP-Kanal. `Session`
  implementiert `Deref` **und** `DerefMut` auf `SessionParts`. Der
  Transport lässt sich deshalb auf zwei Wegen unabhängig vom Kanal
  austauschen: (1) mit `&mut Session` über das Feld oder über die ganzen
  `SessionParts`; (2) schon mit `&Session`, also auch hinter dem `Arc` in
  der Registry, über den Guard von `transport.lock()`, der `&mut Box<dyn
  SshTransport>` herausgibt. Danach gehört der schon geöffnete Kanal zu
  einem anderen Transport als die Sitzung. Alle heutigen Aufrufer rufen
  über den Guard nur Methoden auf. Gemessen:
  Entfernt man `DerefMut`, scheitern 168 Stellen beim Übersetzen, alle in
  Tests (`#[cfg(test)]`-Module und `orchestration::test_support`), keine im
  Produktivcode. `\.transport\b` trifft in `crates/` 11 Stellen: 8 Sperr-Aufrufe,
  alle nur Methoden (7 Produktiv, 1 Test), 1 Zuweisung in einem Test
  (`commands/chat.rs`) und 2 Kommentare.
- **Doku-Verweise (A4).** 14 Kommentare in `crates/` nennen
  `app_shell::<modul>` für ein Modul, das es in `crates/app-shell/src`
  nicht gibt: `app_shell::orchestration` (9), `app_shell::compaction` (3,
  davon einer über einen Zeilenumbruch: `app_shell::` am Zeilenende in
  `crates/ai-providers/src/anthropic.rs`, `compaction::…` in der nächsten
  Zeile), `app_shell::key_files` (1), `app_shell::lib` (1). Die genannten
  Elemente liegen heute in `app-logic` (`orchestration`, `compaction`,
  `key_files`) bzw. sind die Kistenwurzel von `app-shell` (`lib`). Ein
  Verweis nennt eine Funktion, die nur in `ai-providers` existiert
  (`install_test_subscriber_once`). Gezählt mit dem Befehl in T10.

## 2. Ziel und Nicht-Ziele

Ziel: A1–A4 wie in §3.

Nicht-Ziele:
- Kein begrenztes Lesen: `read_file` und das Trait `SftpSession` bleiben
  unverändert. Die Datei darf kurz ganz im Speicher liegen, sie wird nur
  nicht weitergegeben (Entscheidung Stefan, §9).
- Keine Grenze für Download, Upload oder die KI-Lesepfade.
- `SshError`s `Display` bleibt unverändert, auch das Präfix
  `Channel-Fehler: ` an anderen Stellen.
- Die Meldung bei `UserChanged` (erhöhter Modus läuft als anderer Nutzer)
  bleibt, wie sie ist.
- Die Rennen in `delete_recursive` und `chmod_recursive` (ADR 0078 §1)
  bleiben unberührt.
- Keine dauerhafte Prüfung der `app_shell::`-Verweise in CI.

## 3. Anforderungen

### A1 — Größengrenzen (BL-0274)

- **A1.1** „Dateiinhalt kopieren" (`sftp_read_text`) MUSS eine Datei
  ablehnen, deren gelesener Inhalt größer als 256 KB ist, auch wenn die
  Prüfung per `stat` vorher bestanden hat oder `stat` gescheitert ist. Die
  Meldung ist wörtlich dieselbe wie bei der Ablehnung per `stat`. Dateien
  bis einschließlich 256 KB verhalten sich wie bisher.
- **A1.2** „Lokal öffnen" (`sftp_open_for_editing`) MUSS eine Datei über
  50 MB (50 × 1024 × 1024 Bytes) ablehnen, und zwar vor dem Lesen, wenn
  `stat` die Größe schon zeigt, sonst (Datei ist gewachsen) nach dem Lesen.
  Ein gescheitertes `stat` bricht den Befehl weiter ab wie heute. Meldung, wörtlich:
  `Datei ist größer als 50 MB — zu groß zum lokalen Öffnen. Bitte stattdessen herunterladen.`
- **A1.3** Bei einer Ablehnung nach A1.2 entsteht **keine** lokale Datei
  und kein Verzeichnis im Editier-Temp-Verzeichnis. Eine dort schon
  liegende Kopie derselben Datei aus einem früheren „Lokal öffnen" bleibt
  unverändert.
- **A1.4** Beide Grenzen gelten für den normalen und den erhöhten Kanal
  gleich. Die Grenze aus A1.2 ist eine eigene Konstante, ohne gemeinsamen
  Code mit `MAX_TEXT_PREVIEW_BYTES` oder dem KI-Lesepfad (Begründung am
  Doc-Kommentar von `MAX_TEXT_PREVIEW_BYTES`, gilt hier genauso).

### A2 — Transfer-Meldung beim Widerruf (BL-0275)

- **A2.1** Scheitert ein Download oder Upload über den erhöhten Kanal,
  weil der erhöhte Modus widerrufen wurde, MUSS das Feld `error` von
  `sftp-transfer-finished` wörtlich denselben Text tragen wie das
  Befehlsergebnis (`ELEVATED_CHANNEL_INACTIVE`), ohne Präfix.
- **A2.2** Für alle anderen Fehler bleibt der Text im Ereignis, wie er ist.
  Ein Transfer, der vor dem Widerruf fertig war, meldet weiter Erfolg
  (`error: None`).

### A3 — Transport und Kanal bleiben ein Paar (BL-0276)

- **A3.1** Code außerhalb von `app-logic` DARF den Transport einer
  bestehenden `Session` weder ersetzen noch herausnehmen noch mit dem einer
  anderen `Session` tauschen. Das gilt für das Feld selbst und für jeden
  Weg, der es mitnimmt (etwa ein Tausch oder Ersetzen aller mitgegebenen
  Bestandteile einer Sitzung), und auch nicht über die Sperre des
  Transports, die schon mit `&Session` erreichbar ist. Nachweis beim
  Übersetzen, wie T11 in Spec 0085.
- **A3.2** Weiter erlaubt bleiben: der Tausch zweier **ganzer**
  `Session`-Werte (Transport und Kanal wandern zusammen), das Sperren
  des Transports **zum Benutzen**, also alle Methoden von `SshTransport`
  über die Sperre, und das Bauen einer Sitzung aus allen Bestandteilen.
  Die Sperre gibt dabei keine Referenz heraus, über die sich der
  Transport-Wert selbst ersetzen ließe (Muster: `NormalSftpGuard::sftp`,
  Spec 0085 A3.1).
- **A3.3** Tests MÜSSEN weiterhin Sitzungen mit gezielt gesetzten Feldern
  bauen können. Der Weg ist frei, darf aber keine Hintertür für
  Produktivcode außerhalb von `app-logic` sein (z. B. nur unter `cfg(test)`
  bzw. Feature `test-support`, wie `set_sftp_for_tests`).
- **A3.4** Kein Verhaltensunterschied im Produktivbetrieb. Die Zusagen aus
  Spec 0085 A3 (Kanal von außen nicht setzbar, ersetzbar, entnehmbar)
  gelten unverändert, ihre verbotenen Fälle bleiben `compile_fail`. Ihre
  kompilierenden Zwillinge dürfen angepasst werden, falls sie nach dem
  Umbau nicht mehr übersetzen (etwa weil sie über `DerefMut` ein anderes
  Feld schreiben). Der Zwilling unterscheidet sich weiter nur in der
  verbotenen Zeile. Jede Anpassung steht im ADR.

### A4 — Doku-Verweise (BL-0277)

- **A4.1** Jeder Kommentar in `crates/`, der `app_shell::<modul>` nennt,
  nennt ein Modul, das es in `crates/app-shell/src` gibt. Jeder in diesem
  Schritt geänderte Verweis zeigt auf ein Element, das es am genannten Ort
  gibt. Verweise auf Elemente in `app-logic` heißen
  `app_logic::…` (bzw. `crate::…` innerhalb von `app-logic`). `app_shell::lib`
  wird zu `app_shell` bzw. `app_shell::run`.
- **A4.2** Ein Verweis, dessen Ziel es nirgends mehr gibt, wird auf das
  heutige Ziel umgeschrieben oder, wenn es keins gibt, entfernt. Nur
  Kommentare ändern sich, kein Code.

## 4. Design

- A1: Nachprüfung der Länge des gelesenen Puffers, danach dieselbe
  Fehlerbildung wie beim `stat`. Für A1.3 prüft „Lokal öffnen" vor jedem
  Schreibzugriff ins Temp-Verzeichnis.
- A2: Frei, solange A2.1/A2.2 halten und die Übersetzung in
  `ELEVATED_CHANNEL_INACTIVE` nicht zu einer eigenen Regel je Befehl wird
  (ADR 0078 §4). Naheliegend: denselben Widerrufs-Vermerk nutzen, den
  `with_browser_channel` liest.
- A3: Frei. Die Messung in §1 zeigt: Ohne `DerefMut` betrifft Weg 1 nur
  Tests. Weg 2 ändert die Sperrstellen im Produktivcode (7 Stellen), deren
  Verhalten bleibt gleich (A3.4). Ein Guard mit
  `DerefMut<Target = dyn SshTransport>` hält diese Änderungen klein. Den gewählten Weg und den verworfenen im ADR festhalten.
- ADR: Ein ADR `docs/adr/0080-…` mit den Entscheidungen zu A1–A3.
  Changelog-Fragment in `changelog.d/` für A1 (sichtbare
  Verhaltensänderung). A2–A4 brauchen keins.

## 5. Sicherheits-Invarianten

- **Spec 0067 A (KI und MCP nie über den erhöhten Kanal):** A3 verstärkt
  sie, weil der normale Kanal nur noch zum Transport seiner eigenen Sitzung
  gehört. Keine Anforderung lockert Spec 0085 A3.
- **Widerruf (Spec 0085 A1):** A2 ändert nur den Text im Ereignis. Die
  maßgebliche Widerrufsprüfung unter der Sperre und der Schnellabbruch
  (ADR 0078 §2) bleiben unberührt.
- **Temp-Dateien (Spec 0054/0067 A5):** A1.3 schreibt bei Ablehnung nichts.
  Rechte `0700`/`0600` im Erfolgsfall unverändert.
- **Keine neue Datensenke.**

## 6. Tests

Jeder Test muss mit der heutigen Implementierung scheitern (Ausnahme:
ausdrücklich als Absicherung markiert). Bitte je Test im Bericht belegen,
dass er ohne den Fix rot war.

- **T1 (A1.1, wachsende Datei):** Mock-Kanal, bei dem `stat` 100 Bytes
  meldet und `read_file` 256 KB + 1 Byte liefert. Ergebnis ist ein Fehler
  mit dem Text der `stat`-Ablehnung. Scheitert heute, weil der Inhalt
  zurückkommt.
- **T2 (A1.1, `stat` scheitert):** `stat` liefert einen Fehler,
  `read_file` 256 KB + 1 Byte. Erwartet: Ablehnung, gleicher Text.
- **T3 (A1.1, Grenzwert):** genau 256 KB gültiges UTF-8 wird
  zurückgegeben. Absicherung gegen ein `>=` statt `>`.
- **T4 (A1.2, vorher):** `stat` meldet 50 MB + 1. Erwartet: Ablehnung mit
  dem Text aus A1.2, **ohne** dass `read_file` aufgerufen wurde (Mock
  zählt). Kein Eintrag im Editier-Temp-Verzeichnis.
- **T5 (A1.2/A1.3, nachher):** `stat` meldet 10 Bytes, `read_file`
  liefert 50 MB + 1. Erwartet: Ablehnung, keine lokale Datei. Liegt vorher
  eine Kopie mit bekanntem Inhalt dort, ist sie danach byte-gleich.
  Testaufbau darf die Grenze nicht über einen echten 50-MB-Puffer prüfen
  müssen, wenn der Test dadurch spürbar langsamer wird; dann den Weg im
  ADR begründen (z. B. Grenze als Parameter der inneren Funktion).
- **T6 (A1.2, Grenzwert):** genau 50 MB wird geöffnet. Absicherung.
- **T7 (A1.4):** T1 und T4 auch über den erhöhten Kanal (ein Fall genügt
  je Grenze).
- **T8 (A2.1):** Download über den erhöhten Kanal, Widerruf vor dem
  Lesen bzw. Schreiben, aber nach dem Erstzugang (Haltepunkt
  `set_before_operation_hook` aus Spec 0085). Heute nimmt der Download-
  und der Upload-Kern ein `AppHandle` und baut den Emitter selbst, kein
  Test erreicht sie. Beide dürfen dafür den Emitter
  (`app_logic::events::EventEmitter`) als Parameter bekommen; der Test
  nutzt ein aufzeichnendes Emitter-Double. Das aufgezeichnete
  Ereignis `sftp-transfer-finished` hat `error` == Befehlsergebnis ==
  `ELEVATED_CHANNEL_INACTIVE`. Dasselbe für Upload. Scheitert heute am
  Präfix.
- **T9 (A2.2):** Ein Download, der mit einem gewöhnlichen
  `ChannelError` scheitert (kein Widerruf), meldet im Ereignis weiter den
  Text mit Präfix. Absicherung gegen eine zu breite Umstellung.
- **T10 (A4):** Befehl, Ergebnis 0 Zeilen:
  ```bash
  mods=$(ls crates/app-shell/src | sed -E 's/\.rs$//' | grep -vx lib | sort -u | paste -sd'|' -)
  grep -rnoE "app_shell::[a-z_]+" crates --include='*.rs' \
    | grep -vE "app_shell::(${mods}|run)\b"
  ```
  Heute 13 Zeilen. Der Befehl sieht keine Verweise über einen
  Zeilenumbruch; den einen aus §1 prüft der Bericht von Hand, dazu
  `grep -rn "app_shell::$" crates --include='*.rs'` (heute 1 Treffer,
  danach 0 oder nur Treffer mit gültigem Modul in der Folgezeile). Zusätzlich für jeden geänderten Verweis im Bericht
  belegen, dass das Ziel existiert (`grep -rn` mit Treffer).
- **T11 (A3.1, Doctests aus Sicht eines anderen Crates):** je ein
  `compile_fail`-Fall mit kompilierendem Zwilling, der sich nur in der
  verbotenen Zeile unterscheidet (Muster wie Spec 0085 T11):
  (a) `mem::swap` der Transporte zweier Sitzungen,
  (b) `mem::replace` des Transports durch einen neuen Wert,
  (c) Tausch aller mitgegebenen Bestandteile zweier Sitzungen
  (heute `mem::swap(&mut **a, &mut **b)` bei `a, b: &mut Session`, bzw.
  was nach dem Umbau dem entspricht),
  (d) Zuweisung eines neuen Transports,
  (e) mit nur `&Session`: Zuweisung eines neuen Transports über die
  Sperre und `mem::swap` der Inhalte zweier gesperrter Transporte.
  Kompilieren MUSS: Tausch zweier ganzer `Session`-Werte
  (`mem::swap(a, b)` bzw. `mem::swap(&mut *a, &mut *b)`) und ein
  Methodenaufruf über die Sperre (z. B. `execute`).
  Für jeden `compile_fail`-Fall im Bericht zeigen, dass er heute
  übersetzt (also wirklich ein neues Verbot ist).
- **T12 (A3.3):** Die bestehenden Tests, die Felder nach dem Bau setzen,
  laufen nach dem Umbau unverändert in ihrer Aussage. Kein Test wird
  entfernt oder abgeschwächt, um A3 durchzubekommen.

## 7. Umsetzungsreihenfolge

Ein Lauf, Commits je Schritt:

1. A3 mit T11/T12 und ADR-Abschnitt (Kern, ERHÖHT).
2. A1 mit T1–T7, Changelog-Fragment, ADR-Abschnitt.
3. A2 mit T8/T9.
4. A4 mit T10.
5. Gate nach `CLAUDE.md`, spec-reviewer (Priorität ERHÖHT wegen A3).

## 8. Offene Punkte

Keine. Die K3-Frage zu A1 ist entschieden (§9).

## 9. Klarstellungen

- 2026-09-28 · Tor 1 · Stefan: Vorschau nach dem Lesen erneut prüfen, kein
  begrenztes Lesen. „Lokal öffnen" bekommt eine Grenze von 50 MB, geprüft
  vor und nach dem Lesen.
