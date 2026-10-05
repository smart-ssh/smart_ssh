# ADR 0096 — T17 und T18 belegen: was gemessen wird und was gelesen

Status: akzeptiert
Betrifft: Spec 0101 (§7, T17/T18), ADR 0095 §10

ADR 0095 §10 ließ zwei Tests offen: T17 (Schlüssel, Passwort und Secret
tauchen weder im Log noch im Diagnosepaket auf) und die zweite Hälfte von
T18 (vor der Entsperrung ist kein Kommando erreichbar und der MCP-Server
läuft nicht). Beide sind jetzt belegt. Dieses Dokument hält fest, **wie** —
weil zwei der Tests einen ungewöhnlichen Weg nehmen und das eine Entscheidung
ist, keine Nachlässigkeit.

## 1. T17 läuft gegen den echten Log-Strom, nicht gegen Fehlertexte

Der vorhandene `test_no_error_text_carries_secret_material` prüft, was der
Nutzer sieht. T17 fragt nach dem, was auf die Platte geht. Der neue Test
zeichnet deshalb den tatsächlichen `tracing`-Strom des ganzen Passwort-Modus
auf — im selben JSON-Format, das `logging::init_logging` schreibt — und
reicht **genau diesen Strom** in `build_diagnostics_bundle`, also in der
Form, in der `logging::read_last_log_lines` ihn dem Diagnose-Kommando
liefert. Gesucht wird nach K (Hex, Base64, Byte-Liste aus `{:?}`), dem
abgeleiteten Datenbankschlüssel (PRAGMA-Wert und Hex), beiden Passwörtern und
einem Secret, das nichts mit K zu tun hat.

Zwei Aussagen über das Paket, und beide sind nötig:

- Die Positivliste aus Spec 0063 kennt keine Zeile dieses Moduls, das Paket
  lässt sie also alle draußen (fail-closed).
- Käme je eine davon auf die Liste, bliebe die Suche nach den Begriffen als
  Wächter übrig.

**Drei Zeilen schreibt der Test selbst** und sagt das auch: Sie bilden die
Produktiv-Aufrufstellen in `app_shell::commands::master_password` nach
(`detail_for_log`, `?state`, `?health`), die aus `app-logic` nicht erreichbar
sind. Die `?state`-Zeile ist dabei die wichtigste: Sie ist die Stelle, an der
ein `#[derive(Debug)]` auf `RootKeyState` K ins Log schriebe — und der
Gegenbeweis dazu ist geführt.

**Nicht geprüft, weil die Zusage nicht besteht:** ein PRAGMA-Schlüsselwert,
der über einen SQL-Fehlertext in eine Zeile der Positivliste gerät, würde vom
`OutputRedactor` nicht erkannt (der sucht Secret-*Muster*, kein rohes Hex).
Geschlossen ist dieser Weg an der Quelle: `DatabaseKey::pragma_value`
liefert einen `SecretString` ohne `Display`, und `DatabaseKey`s eigenes
`Debug` zeigt nichts. Ein Test, der das Paket einen geplanten PRAGMA-Wert
redigieren ließe, prüfte eine Zusage, die niemand gegeben hat, und wäre
dauerhaft rot.

## 2. T18, erste drei Aussagen: gemessen über den echten IPC-Weg

Der neue `startup_gate_wiring` baut eine Mock-App, deren `invoke_handler`
**das echte `gated(...)`** ist, dazu ein Webview, und schickt Aufrufe mit
`tauri::test::get_ipc_response` hindurch. Damit stehen drei Aussagen, die der
reine Entscheidungstest nicht trifft: Der abgelehnte Aufruf kommt als Fehler
mit dem Code aus A20 an, der Verteiler wird dabei **gar nicht erst betreten**
(er zählt seine Aufrufe), und die drei Wahlmöglichkeiten aus A16 kommen
durch.

Zwei Dinge mussten dafür gemessen werden:

- **Die ACL der Mock-App ist leer und liegt vor dem Tor.** Ohne Freigabe
  antwortet Tauri mit „… not allowed. Plugin not found", und der Test prüfte
  nur noch die ACL. `RuntimeAuthority::__allow_command` ist der dafür
  vorgesehene Zugang — `#[doc(hidden)]`, aber öffentlich, und von Tauris
  eigenen Tests genutzt. Im Produktivcode wäre ein Aufruf davon ein Fehler;
  hier ist er die Voraussetzung dafür, dass das Tor überhaupt gefragt wird.
- **Nur `tauri://localhost` gilt als lokaler Ursprung.** Mit
  `http://tauri.localhost` greift die als `ExecutionContext::Local` erteilte
  Erlaubnis nicht, und die Antwort ist wieder eine ACL-Ablehnung.

`gated` ist dafür über die Laufzeit `R` abstrahiert statt auf `tauri::Wry`
festgelegt. An der Aufrufstelle in `run()` ändert sich nichts.

## 3. Zwei Tests lesen die Quelle — mit Ansage

Zwei Aussagen sind so nicht erreichbar:

- **Dass `run()` das Tor benutzt.** Die Tests aus §2 bauen ihre eigene App um
  `gated` und beweisen, was das Tor tut, nicht dass es verdrahtet ist. Nähme
  eine Änderung `gated` aus der Builder-Kette, blieben sie grün.
- **Dass der MCP-Server vor der Entsperrung nicht startet.** Der Startpfad
  (`autostart_if_enabled` → `start_server_if_not_running` → `AppMcpBackend`,
  das die Kommandos aufruft) ist auf `tauri::Wry` festgelegt. Mit der
  Test-Laufzeit ist er nicht aufrufbar, und eine echte `Wry`-App braucht ein
  Fenstersystem. Ein `AppState` lässt sich im Test ebenfalls nicht stellen —
  er hängt an einem offenen SQLite-Pool.

Beide Aussagen stehen deshalb als Prüfung auf den Quelltext: ein
`include_str!` auf das eigene Modul und eine Zusicherung über die Stelle, auf
die es ankommt (der Verteiler liegt hinter `gated(`; der Wächter auf den
verwalteten `AppState` steht **vor** dem Serverstart und kehrt zurück).

**Die Alternativen und warum nicht:**

- *Die Kette über `R` abstrahieren.* Das hieße, `AppMcpBackend` und die
  Kommandos dahinter umzubauen — ein Eingriff in den Produktivpfad, dessen
  Risiko größer ist als der Gewinn an Prüftiefe.
- *Den Punkt offen lassen.* Das war der Stand, den ADR 0095 §10 festhielt.
  Ein unbelegtes A16 ist bei einer Sicherheits-Anforderung das schlechtere
  Ergebnis.
- *Eine reine Prädikatsfunktion einziehen* (`fn darf_starten(bool) -> bool`).
  Die wäre prüfbar, aber der Test prüfte dann nur sich selbst: Die
  Entscheidung steckt in `try_state::<AppState>()`, nicht in der
  Verzweigung darüber.

**Der Preis ist bekannt:** Wird eines der gelesenen Symbole umbenannt oder
die Builder-Kette anders geschrieben, scheitert der Test, obwohl nichts
kaputt ist. Die Fehlermeldungen sagen deshalb, was zu tun ist. Jeder dieser
Tests ist gegen einen verfälschten Stand rot gesehen worden, einschließlich
einer Verfälschung, die weiter kompiliert (das Tor aus der Builder-Kette
genommen).

## 4. Kein Changelog-Fragment

Dieser Schritt fügt Tests hinzu und ändert kein Verhalten. Nach
`CLAUDE.md` („Versioning & changelog") bleibt Test-Infrastruktur aus dem
Changelog heraus.
