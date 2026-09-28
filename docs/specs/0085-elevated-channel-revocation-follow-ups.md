# Spec 0085 — Erhöhter Kanal: Widerruf innerhalb laufender Befehle, Kanal-Ende, Schutz des normalen Kanals

Status: **freigegeben** (Stefan, 2026-09-28) · Backlog: BL-0270, BL-0271, BL-0272, BL-0273 · Gate: —
Repo: **öffentlich** `smart-ssh` — `crates/app-shell/`, `crates/app-logic/`,
`crates/ssh-transport/` (nur Tests)
Review-Priorität: **ERHÖHT** (Teil 1: Schreibzugriffe über den erhöhten
Dateibrowser-Kanal, Spec 0067 A, Spec 0084 §9)
Zweck: Ein Widerruf des erhöhten Modus beendet auch Befehle, die schon
laufen. Dass der Kanal danach wirklich zu ist, wird durch einen Test
gesichert. Und außerhalb von `app-logic` lässt sich kein Kanal in den
normalen SFTP-Kanal einer Sitzung schieben.

## 1. Ausgangslage (gemessen, Stand `28ca814`)

- **Rekursive Befehle prüfen den Widerruf nur einmal.** `sftp_delete`,
  `sftp_chmod` und `sftp_delete_preview` holen den Kanal einmal über
  `lock_browser_sftp` und `BrowserSftpGuard::sftp` und laufen dann durch
  `delete_recursive`, `chmod_recursive` bzw. `walk_dirs_and_count_files`.
  Der Widerrufs-Merker (`ElevatedSftpSlot::is_revoked`, Spec 0084 §9) wird
  nur in `BrowserSftpGuard::sftp` geprüft, also vor dem ersten Zugriff.
  Wird der Modus während der Rekursion ausgeschaltet, die Sitzung
  entfernt oder für einen anderen Nutzer neu aktiviert, laufen die übrigen
  Zugriffe weiter mit den alten Rechten. `ElevatedSftpSlot::revoke` wartet
  dabei auf die Kanal-Sperre, also auf das Ende der ganzen Rekursion.
  `download_recursive` holt den Kanal je Verzeichnis neu und ist nicht
  betroffen. Mehrere Operationen unter einem Zugriff führen auch
  `sftp_read_text` und `sftp_open_for_editing` aus (`stat`, danach
  `read_file`).
- **Kanal-Ende gemessen** (russh 0.63.1, russh-sftp 2.4.0, OpenSSH im
  Test-Container aus `dev/ssh-test-servers/`, sudo ohne Passwort). Der
  Kanal wird wie in `open_sftp_via_exec_inner` geöffnet
  (`sudo -n /usr/lib/ssh/sftp-server`). Solange der Client ihn hält, laufen
  `sudo` und `sftp-server` als `root`. Wird der Client-Wert verworfen,
  sind beide Prozesse sofort beendet, und zwar bei **stehender**
  Verbindung. Nach dem Ende der Verbindung läuft ebenfalls keiner mehr.
  Ein explizites `close()` ändert daran nichts. Gesichert ist das nur durch
  das `Drop` der beiden Bibliotheken. Kein Test im Repo würde es merken,
  wenn ein Versions-Update es ändert.
- `ElevatedSftpSlot::revoke` (Deaktivieren) nimmt den Kanal unter der Sperre
  heraus und verwirft ihn. `remove_session` (Trennen) setzt nur den
  Merker. Der Kanal endet dann mit dem letzten Halter des Slots, spätestens
  aber mit dem Trennen des Transports in `disconnect`.
- **`Session::sftp` ist `pub`** (`app_logic::session::Session`). `app-shell`
  nutzt den normalen Kanal über `lock_browser_sftp`
  (`BrowserSftpGuard::Normal` hält den `MutexGuard<Option<Box<dyn
  SftpSession>>>`), und `BrowserSftpGuard::sftp` liefert `&mut Box<dyn
  SftpSession>`. Über beide Wege ließe sich der Kanal **ersetzen**, auch
  durch einen erhöhten. KI (`orchestration::remote_files`) und MCP liefen
  dann erhöht. Heute tut das kein Produktivcode. Tests in `app-shell`
  schreiben das Feld direkt (`commands/elevation.rs`, Modul
  `browser_channel_tests`). Außerdem baut `app-shell` `Session` per
  Struct-Literal: im Produktivcode beim Verbinden (`commands/connect.rs`)
  und in Tests (`commands/chat.rs`).
- **Doku-Verweise:** In `crates/app-logic` zeigen 42 Kommentar-Verweise auf
  Elemente, die in `app-shell` liegen: `crate::commands` (24),
  `crate::run` (5), `crate::mcp_backend` (4), `crate::mcp_settings` (4),
  je einmal `crate::elevated_sftp`, `crate::event_emitter`,
  `crate::local_server`, `crate::risk_second_opinion`,
  `crate::startup_dialog`. Gezählt mit dem
  Befehl in T12. Alle stehen in Kommentaren, keiner ist ein Intra-Doc-Link.
- Der Test `test_ai_and_mcp_file_actions_never_use_the_elevated_channel`
  begrenzt den MCP-Zweig mit einem Timeout (5 s), den KI-Zweig
  (`run_chat_turn`) nicht. Wartet dieser Zweig auf eine Bestätigung, greift
  erst `PENDING_ACTION_CONFIRM_TIMEOUT` (3600 s).

## 2. Ziel und Nicht-Ziele

Ziel: A1–A5.

Nicht-Ziele: kein Zähler „x von y erledigt“ bei Abbruch (Entscheidung
Stefan, §8). Kein neuer Nutzertext. Kein Rückgängigmachen schon geänderter
Einträge. Keine Änderung an `download_recursive`, am Aufbau des erhöhten
Kanals, an russh/russh-sftp-Versionen oder an KI- und MCP-Pfaden.
Der Bestätigungs-Timeout bleibt unverändert.

## 3. Anforderungen

**A1 — Widerruf wirkt auch innerhalb eines laufenden Befehls** (BL-0270).
1. Nach einem Widerruf, also Deaktivieren, Entfernen der Sitzung (Spec 0084
   A2.1) oder Neu-Aktivieren für einen anderen Nutzer, führt **kein**
   Browser-Befehl mehr eine SFTP-Operation über den widerrufenen Kanal aus.
   Das gilt auch für Befehle, die vor dem Widerruf begonnen haben. Eine
   schon laufende einzelne SFTP-Operation darf zu Ende laufen.
2. Ein so abgebrochener Befehl endet mit dem bestehenden Fehler
   `ELEVATED_CHANNEL_INACTIVE`, nicht mit `Ok` und nicht mit einem
   SFTP-Fehler. Schon geänderte Einträge bleiben geändert.
3. Deaktivieren wartet höchstens auf die gerade laufende einzelne
   SFTP-Operation, nicht auf den Rest des Befehls. Das präzisiert Spec 0084
   §9 („wartet, bis ein laufender Vorgang fertig ist“): Mit „Vorgang“ ist
   von hier an eine einzelne SFTP-Operation gemeint.
4. Ein abgebrochener erhöhter `delete`/`chmod`, bei dem mindestens eine
   Operation lief, schreibt die Audit-Zeile wie bei jedem Fehlschlag:
   `ok = false`, **mit** dem Ziel-Nutzer, unter dem die schon ausgeführten
   Operationen liefen. Scheitert schon der Zugriff auf den Kanal
   (`BrowserSftpGuard::sftp`), gibt es wie bisher keine Zeile. Eine Zeile
   `ok = false` ohne ausgeführte Operation ist zulässig. Sie behauptet
   keinen Erfolg.
5. Über den normalen Kanal ändert sich nichts.
6. Kein Rückfall: Nach dem Abbruch läuft keine Operation dieses Befehls
   über den normalen Kanal weiter.

**A2 — Kanal-Ende gesichert** (BL-0271).
1. Ein Test auf Transport-Ebene scheitert, wenn das Verwerfen einer über
   `open_sftp_via_exec` geöffneten SFTP-Sitzung den SSH-Kanal auf der
   Server-Seite nicht innerhalb einer festen Frist schließt, während die
   Verbindung weiter steht.
2. Ein Test scheitert, wenn nach dem Deaktivieren noch irgendein Zustand
   den Kanal-Wert hält, wenn er also nur als widerrufen markiert statt
   verworfen wird.

**A3 — Normaler Kanal außerhalb von `app-logic` nicht ersetzbar**
(BL-0272).
1. Code außerhalb von `app-logic` kann den normalen SFTP-Kanal einer
   `Session` weder setzen noch ersetzen noch herausnehmen. Er erhält nur
   Zugriff, um ihn zu **benutzen**, unter derselben Sperre wie heute.
   Keine Referenz, die er erhält, erlaubt ein Ersetzen des Kanals, auch
   nicht über `std::mem::replace`/`swap`/`take`.
2. Befüllt wird der Kanal nur in `app-logic`, aus dem Transport der
   Sitzung selbst (heute `ensure_sftp_open`).
3. Tests, die einen Test-Kanal einsetzen, tun das über eine Testhilfe
   hinter dem Feature `test-support` (Spec 0084 A5). Produktivbauten
   aktivieren das Feature weiterhin nicht.
4. Ein automatischer Test im Gate scheitert, wenn A3.1 verletzt wird.

**A4 — Doku-Verweise** (BL-0273). Kommentare in `crates/app-logic`
verweisen auf Elemente in `app-shell` als `app_shell::…`, nicht als
`crate::…`. Die Zählung aus T12 ergibt 0.

**A5 — Test hängt nicht** (BL-0273). Der KI-Zweig von
`test_ai_and_mcp_file_actions_never_use_the_elevated_channel` ist wie der
MCP-Zweig zeitlich begrenzt. Wartet er, scheitert der Test mit einer
Meldung, statt zu hängen.

**A6 — Gate grün** (Repo-`CLAUDE.md`), keine neuen `#[allow(…)]`, kein
bestehender Test entfällt oder wird `#[ignore]`. Changelog-Fragment für A1
nach `changelog.d/README.md`: nennt das Verhalten beim Ausschalten, keine
Einzelheiten zur früheren Lücke.

## 4. Design

- A1 ist eine Verhaltensanforderung. Wie der Merker geprüft wird,
  entscheidet der Coder. Die Prüfung muss **jede einzelne** Operation über
  den erhöhten Kanal abdecken, auch in künftigen Befehlen. Eine Lösung, die
  nur einzelne Schleifen anfasst, genügt A1.1 nicht, denn auch
  `sftp_read_text` und `sftp_open_for_editing` führen zwei Operationen aus.
- Der Abbruch muss beim Aufrufer als `ELEVATED_CHANNEL_INACTIVE` ankommen,
  und zwar wörtlich. Ein `SshError`, der über die allgemeine Umwandlung
  (`impl From<E: Display> for CommandError`) zu „Channel-Fehler: …“ wird,
  verletzt A1.2. **Festlegung:** Die Prüfung sitzt zentral am Zugang zum
  erhöhten Kanal in `app-shell`. Dort wird auch vermerkt, dass ein Abbruch
  wegen Widerrufs stattfand. Ebenso zentral wird das Ergebnis des Befehls
  in `ELEVATED_CHANNEL_INACTIVE` übersetzt, sobald dieser Vermerk gesetzt
  ist. Die Übersetzung gilt auch dann, wenn der Befehl den SFTP-Fehler
  selbst abfängt. Ein einheitlicher Abschluss-Aufruf je Befehl ist
  erlaubt, eine eigene Regel je Befehl nicht. Heute schlucken
  `sftp_exists` (`stat(..).is_ok()` → `Ok(false)`), `sftp_download`
  (`.ok()`, die Gesamtgröße vor dem Speichern-Dialog; der Transfer danach
  greift erneut zu und scheitert dort) und `sftp_read_text` (`if let Ok`)
  einen Fehler bei `stat`.
  Ohne die Übersetzung würde ein Widerruf dort zu `Ok` und verletzte A1.2.
  `crates/core` wird nicht geändert.
- A3: Eine Referenz auf das Trait-Objekt (`&mut dyn SftpSession`) lässt
  sich nicht ersetzen, eine Referenz auf `Box` oder `Option` schon. Die
  Grenze verläuft also am Typ der herausgegebenen Referenz, nicht nur an
  der Sichtbarkeit des Felds.
- A3: Wie `app-shell` danach eine `Session` erzeugt (heute Struct-Literal
  in `commands/connect.rs`), wählt der Coder. Er braucht dafür kein
  `#[allow(clippy::too_many_arguments)]` (A6), z. B. mit einem
  Parameter-Struct. Ein neu erzeugter normaler Kanal ist immer leer. Der
  Konstruktor nimmt keinen Kanal an.
- Verworfen: Abbruch mit Zähler (Entscheidung Stefan).
  Aufräumen/Rückgängigmachen bei Abbruch: unvollständig und selbst ein
  erhöhter Schreibvorgang nach dem Widerruf.

## 5. Sicherheits-Invarianten

- **Spec 0067 A / Spec 0084 A1 (erhöhter Kanal nur für Browser-Befehle):**
  wird strenger (A3). Eine Abweichung, die KI oder MCP einen Weg zum
  erhöhten Kanal öffnet, ist ein Blocker.
- **Spec 0084 §9 (Widerruf wirkt beim Zugriff):** wird vollständig (A1).
  Die Tests 0084-T10 bis 0084-T10c (`test_t10_…`) und die beiden
  Widerrufs-Regressionstests aus 0084 bleiben
  unverändert grün.
- **Keine stillen Rückfälle:** A1.6.
- **Audit:** Keine Audit-Zeile behauptet einen Erfolg, der nicht
  stattfand. Keine unterschlägt eine erhöhte Änderung, die stattfand (A1.4).
- Filter-Engine, Risiko-Klassifizierung, Redaktion, Bestätigung,
  Credentials: nicht berührt.

## 6. Tests

Für jeden mit ⚑ markierten Test im Bericht belegen, dass er gegen die
genannte kaputte Variante scheitert (lokal umgestellt, nicht committet).

Teil 1 — adversarial (A1):
- **T1 ⚑** Rekursives Löschen über den erhöhten Kanal, Mock-Kanal, der nach
  der k-ten Operation auf ein Signal wartet. Währenddessen Deaktivieren.
  Erwartet: nach dem Widerruf keine weitere Operation am Mock,
  Befehlsergebnis ist **gleich** `ELEVATED_CHANNEL_INACTIVE`, nicht nur
  „enthält“ (gilt ebenso für T2–T4, T6, T6b und T6c). Kaputte Variante: Merker
  nur bei Befehlsbeginn geprüft (Stand heute). Zweite kaputte Variante für
  den Gleichheits-Vergleich: Abbruch kommt als SFTP-Fehler zurück.
- **T2 ⚑** Wie T1 für rekursives chmod.
- **T3 ⚑** Wie T1, aber Entfernen der Sitzung (A2.1 aus 0084) statt
  Deaktivieren. Zusätzlich kehrt das Entfernen zurück, ohne auf den Befehl
  zu warten.
- **T4 ⚑** Wie T1, aber Neu-Aktivieren für einen anderen Nutzer. Keine
  weitere Operation des laufenden Befehls läuft über den alten **oder** den
  neuen Kanal.
- **T5 ⚑** Deaktivieren während T1: Das Deaktivieren kehrt zurück, sobald
  die gerade laufende Operation fertig ist, und nicht erst nach allen
  restlichen Elementen (Mock mit vielen Elementen, Zähler der danach
  ausgeführten Operationen = 0). Kaputte Variante: Merker nur bei
  Befehlsbeginn geprüft, Sperre bis zum Ende gehalten.
- **T6 ⚑** Wie T1 für die Lösch-Vorschau (`sftp_delete_preview`): kein
  weiterer Lesezugriff nach dem Widerruf, Fehler statt Zählergebnis.
- **T6b ⚑** `sftp_read_text` und `sftp_open_for_editing`: Widerruf zwischen
  `stat` und `read_file`. Die Datei wird nicht gelesen, das Ergebnis ist
  `ELEVATED_CHANNEL_INACTIVE`.
- **T7 ⚑** Audit: T1 und T2 mit erfasster Log-Ausgabe. Je genau eine Zeile
  `ok = false` mit dem ursprünglichen Ziel-Nutzer. Kaputte Variante:
  Ziel-Nutzer erst nach dem Widerruf abgefragt (dann fehlt die Zeile).
  Dazu Widerruf vor `BrowserSftpGuard::sftp`: keine Zeile. Lassen sich Logs im
  Test nicht zuverlässig einfangen, weil der Test-Subscriber nur für einen
  Thread gilt, dann meldet der Coder das und prüft stattdessen die
  Eingaben der Audit-Funktion.
- **T6c ⚑** `sftp_exists`: Widerruf vor dem `stat`
  über den erhöhten Kanal. Ergebnis `ELEVATED_CHANNEL_INACTIVE`, nicht
  `Ok(false)`. Kaputte Variante:
  Prüfung je Operation ohne zentrale Übersetzung.
- **T8** Kein Rückfall: In T1 zählt ein Mock des **normalen** Kanals der
  Sitzung 0 Operationen.
- **T9** Regression: rekursives Löschen und chmod über den normalen Kanal
  und über einen nicht widerrufenen erhöhten Kanal verhalten sich wie
  bisher (bestehende Tests grün, dazu ein Durchlauf mit erhöhtem Kanal
  ohne Widerruf bis `Ok`).

Teil 1 — A2:
- **T14 ⚑** Transport-Ebene (Test-Server im Prozess, wie
  `test_sftp_via_exec_runs_sftp_over_the_exec_channel`): Sitzung über
  `open_sftp_via_exec` öffnen, benutzen, verwerfen. Der Server sieht das
  Schließen dieses Kanals innerhalb einer festen Frist, die Verbindung
  steht weiter (danach ist ein weiterer Befehl auf ihr möglich). Kaputte
  Variante: Sitzung gehalten statt verworfen. Dann darf das Schließen
  nicht beobachtet werden.
- **T15 ⚑** App-Ebene: Mock-Kanal, der sein Verwerfen meldet. Der Test
  hält während des Deaktivierens selbst eine Kopie des Slots, so wie ein
  wartender Befehl (etwa über `BrowserChannel::from_request`). Nach dem
  Deaktivieren ist der Kanal verworfen, **obwohl diese Kopie noch lebt**.
  Kaputte Variante: Deaktivieren setzt nur den Merker.

Teil 1 — adversarial (A3):
- **T11 ⚑** Ein Test im Gate scheitert, wenn Code außerhalb von
  `app-logic` den normalen Kanal einer `Session` setzen kann, ob durch
  Zuweisung ans Feld oder über eine erhaltene Referenz (`mem::replace`,
  `swap`, `take`). Der Nachweis muss aus Sicht eines **anderen** Crates
  geführt werden. Kaputte Variante: Feld wieder `pub` bzw. Zugriff liefert
  `&mut Box<dyn SftpSession>`. Belegt im Bericht für beide. Wird der
  Nachweis mit Code geführt, der nicht kompilieren darf, dann gibt es zu
  jedem Fall einen **kompilierenden Zwilling**, der sich nur in der
  verbotenen Zeile unterscheidet und kein Feature `test-support` braucht
  (z. B. `&Session` als Parameter statt einer Konstruktion). So scheitert
  der Fall nachweislich an der verbotenen Zeile und nicht an einem
  Tippfehler.
- **T11b** Bestehende Tests in `app-shell`, die heute das Feld schreiben,
  laufen über die Testhilfe aus A3.3 und sind unter gleichem Namen grün.
  `cargo tree -p smart-ssh-community -e features` zeigt `test-support`
  weiterhin nicht.

Teil 2 (A4/A5):
- **T12** `grep -rnE 'crate::(commands|elevated_sftp|mcp_backend|mcp_settings|local_server|wiring|risk_second_opinion|ssh_config_apply|ssh_config_export|startup_dialog|first_run_notice|chat_retention|event_emitter|run|Wiring|Edition)\b' crates/app-logic | wc -l`
  ergibt 0 (heute 42). Die Liste deckt alle Module von `crates/app-shell/src`
  ab, die es in `app-logic` nicht gibt, dazu die öffentlichen Elemente aus
  `app-shell/src/lib.rs`. Der Coder gleicht sie vorher mit `ls` ab
  und nennt eine Abweichung im Bericht.
- **T13 ⚑** KI-Zweig mit Timeout: Wird der Test lokal so verändert, dass
  der KI-Zweig auf eine Bestätigung wartet, scheitert er innerhalb der
  Frist mit Meldung. Kaputte Variante: ohne Timeout (hängt).

## 7. Umsetzungsreihenfolge

1. **Teil 1 (Opus):** A1 mit T1–T9, Gate, Commit
   `fix(app-shell): stop elevated browser commands on revocation [BL-0270]`.
   A2 mit T14/T15, Gate, Commit `test: … [BL-0271]`. A3 mit T11/T11b,
   Gate, Commit `refactor(app-logic): … [BL-0272]`.
2. **Teil 2 (Sonnet):** A4, A5 mit T12/T13, Changelog-Fragment (A6),
   Gate, Commit(s) `[BL-0273]`.

## 8. Offene Punkte

Keine. Entschieden (Stefan, 2026-09-28): Abbruch bei Widerruf ohne
Zähler und ohne neuen Nutzertext (A1.2).

## 9. Klarstellungen

(wird während der Umsetzung nachgetragen: Datum · Frage-ID · Antwort)
