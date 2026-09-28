# Spec 0082 — Anmeldeart wechseln: kein Credential-Verlust bei gescheitertem Speichern

Status: freigegeben (Stefan, 2026-09-28) · Backlog: BL-0252 · Gate: release-1.0/D
Zweck: Scheitert das Speichern eines bearbeiteten Servers, bleiben die
Zugangsdaten der bisherigen Anmeldeart vollständig erhalten.
Review-Priorität: ERHÖHT (Credential-Handling)

## 1. Ist-Stand (Stand `4d6516b`)

**Ablauf beim Bearbeiten.** `app_shell::commands::servers::update_server`
(Tauri-Command, `State<'_, AppState>`) ruft nacheinander die Ablehnung des
lokalen Servers (`is_local(id)`), `reject_local_jump_host` (private
Funktion in `app-shell`), `normalize_sftp_server_path`, `get_server`,
`app_logic::server_credentials::resolve_auth_method`,
`resolve_sudo_password` und `ProfileStore::update_server` auf, jeweils mit
`?` und ohne Rückweg. Eine Tauri-freie Fassung gibt es nicht; der Ablauf
ist in keinem Unit-Test aufrufbar (gelesen, nicht ausgeführt).

**Aufräumen zuerst.** `resolve_auth_method` ruft als Erstes
`cleanup_abandoned_slots` auf. Bei einem Wechsel der Anmeldeart löscht das
die Schlüsselbund-Einträge der bisherigen Art, **bevor** die neue Art
geprüft oder geschrieben ist. Löschfehler verwirft es still
(`let _ = credential_store.delete(r)`).

**Erreichbar über die Oberfläche.** Beim Bearbeiten sind die Secret-Felder
keine Pflichtfelder (`required={isCreate}` in `ServerForm.tsx`); ein leeres
Feld geht als `null` ans Backend.

**Gemessen** mit Sonden gegen `InMemoryCredentialStore` als Testmodul in
`app_logic::server_credentials` einer Wegwerf-Kopie des Stands oben
(`cargo test -p app-logic --lib probe_bl0252`, Exit 0; M1–M4 und M5
gleichlautend zur ersten Messung vom 2026-09-24). Ausgangszustand jeweils: Server mit `AuthMethod::Password`, Passwort hinterlegt.

| Nr. | Bearbeitung | Ergebnis | Passwort danach |
|---|---|---|---|
| M1 | → Zertifikat, beide Felder leer | `SERVER_CERTIFICATE_REQUIRED` | **gelöscht** |
| M2 | → Private Key, Schreiben in den Schlüsselbund scheitert | Fehler | **gelöscht** |
| M3 | → Agent, danach scheitert das Sudo-Passwort | Fehler | **gelöscht** |
| M4 | → Zertifikat, nur Zertifikat angegeben | `SERVER_CERTIFICATE_KEY_REQUIRED` | **gelöscht**, dazu ein verwaister Eintrag `certificate` |

Die Datenbank trägt in allen vier Fällen weiter `AuthMethod::Password`; der
Server lässt sich nicht mehr verbinden, die Oberfläche zeigt nur die
Fehlermeldung des Speicherns. „Datenbank-Schreiben scheitert" folgt
demselben Ablauf (hergeleitet, nicht gemessen).

**Gemeinsamer Slot (M5).** `PrivateKey` und `IdentityFile` legen ihre
Passphrase unter demselben Ref ab (`credential_ref(server_id,
"passphrase")` in beiden Zweigen von `resolve_auth_method`). Wechselt ein
Server mit Passphrase zwischen beiden Arten, in beide Richtungen, **mit**
neuer Passphrase, trägt die neue `AuthMethod` genau den Ref, den die alte
als „aufzuräumen" führt. Aufräumen einfach ans Ende zu verschieben löscht
die gerade gespeicherte Passphrase.

**Vorbild Anlegen.** `app_logic::servers::create_server` räumt bei jedem
Fehler (`resolve_auth_method`, `resolve_sudo_password`,
`store.create_server`) mit `delete_all_possible_server_secrets` auf und ist
ohne `tauri::State` testbar.

**Doppelte Zuordnung.** „Welche Refs gehören zu einer `AuthMethod`" steht
zweimal: als Paarliste in `cleanup_abandoned_slots` und in
`delete_auth_method_secrets`.

**Bestehende Tests**, die nur `resolve_auth_method` aufrufen und das
Löschen des alten Slots dort erwarten:
`test_update_kind_change_cleans_up_abandoned_slot`,
`test_switching_away_from_an_identity_file_cleans_up_the_passphrase_slot`.

**Testhilfen vorhanden** (`app_logic::test_support`):
`InMemoryProfileStore::with_failing_update_server`,
`InMemoryCredentialStore::with_failing_set_for_slot`, `::with_failing_delete`,
`log_capture::{start_recording, recorded_text}` (thread-lokal, Test im
current_thread-Runtime). `InMemoryCredentialStore` zählt nur Lesezugriffe
(`get_calls`), keine `set`-/`delete`-Aufrufe.

**Vorbild bei den Ablehnungen:** `create_server` lässt `is_local` und
`reject_local_jump_host` im Tauri-Command; die `app-logic`-Funktion prüft
sie nicht selbst.

**Weitere Wege, die die Anmeldeart ändern:** nur
`convert_identity_file_to_keychain` (`IdentityFile` → `PrivateKey`). MCP
kann Server nicht bearbeiten (`mcp-server` bietet nur `list_servers`,
`server_notes`, `propose_action`).

## 2. Teil 0

Teil 0: entfällt — der Fehler ist auf dem Stand oben gemessen (M1–M5);
alles Weitere ist Code im Repo.

## 3. Ziel und Nicht-Ziele

Ziel: Schlägt das Bearbeiten eines Servers an irgendeiner Stelle fehl,
sind Schlüsselbund und Datenbank für die **bisherige** Anmeldeart genau so
verbindbar wie vorher. Einzige Ausnahme ist R1 (§8). Gelingt das
Bearbeiten, ist das Ergebnis dasselbe wie heute.

Nicht-Ziele:
- `create_server`, `delete_server` und `convert_identity_file_to_keychain`
  werden nicht umgebaut; das Sudo-Passwort bleibt ein eigener Slot.
- Kein Zurücksetzen eines im selben Vorgang **überschriebenen** Secrets
  auf den alten Wert (R1).
- Keine Änderung an Formular, Meldungstexten oder Fehlercodes.

## 4. Anforderungen

- **A1 MUSS — nichts von der bisherigen Art geht bei einem Fehler
  verloren.** Scheitert das Bearbeiten an der Pflichtfeld-Prüfung, an
  einem Schreibfehler im Schlüsselbund, am Sudo-Passwort oder am Schreiben
  der Datenbank, ist jeder Schlüsselbund-Eintrag, auf den die bisher
  gespeicherte `AuthMethod` verweist, danach noch vorhanden, und die
  Datenbank ist unverändert. Der Fehler erreicht die Oberfläche mit
  demselben Code wie heute.
- **A2 MUSS — aufgeräumt wird erst nach erfolgreichem Speichern.** Einträge
  der bisherigen Art werden erst entfernt, wenn die Datenbank die neue
  `AuthMethod` trägt. Ein Ref, auf den die **neue** `AuthMethod` verweist,
  wird dabei nie gelöscht (M5).
- **A3 MUSS — kein verwaister Eintrag nach einem Fehler.** Hat der Aufruf
  vor dem Fehler Einträge der neuen Art geschrieben, die zu keinem Ref der
  bisherigen `AuthMethod` gehören (M4), werden sie wieder entfernt.
  Einträge der bisherigen Art und der Slot `sudo_password` werden dabei nie
  angefasst.
- **A4 MUSS — Aufräumfehler sind im Log sichtbar.** Scheitert das Entfernen
  eines Eintrags (A2 oder A3), bleibt das Speichern erfolgreich bzw. der
  ursprüngliche Fehler der gemeldete Fehler; es entsteht eine Warnung mit
  dem Ref und ohne Secret-Inhalt.
- **A5 MUSS — Erfolgsweg unverändert.** Bei gleicher Art bleibt „leeres
  Feld = unverändert". Bei einem Wechsel sind nach dem Speichern alle
  Einträge der bisherigen Art entfernt, außer denen, die die neue Art
  weiterverwendet.
- **A6 MUSS — Prüfungen vor dem Schlüsselbund.** Die Ablehnung eines
  lokalen Servers bzw. eines lokalen Jump-Hosts geschieht weiterhin vor
  jedem Lesen, Schreiben oder Löschen im Schlüsselbund.
- **A7 MUSS — testbar ohne Tauri-Laufzeit.** Der gesamte Bearbeiten-Ablauf
  ist in einem Unit-Test mit In-Memory-Speichern aufrufbar; der
  Tauri-Command reicht nur noch durch.

## 5. Design

- Nach erfolgreichem Speichern entfernt wird genau: die Refs der
  bisherigen `AuthMethod` abzüglich der Refs der neuen. Diese
  Mengenregel ersetzt die Paarliste; bei gleicher Art ist die Menge leer,
  M5 ist in beiden Richtungen abgedeckt.
- Die Zuordnung „Refs einer `AuthMethod`" gibt es danach nur noch einmal;
  Aufräumen nach Erfolg und `delete_auth_method_secrets` nutzen sie beide.
- Schreiben bleibt in der Reihenfolge Schlüsselbund → Datenbank; umgedreht
  wird nur das Löschen.
- Anders als bei `create_server` ziehen die Ablehnung des lokalen Servers
  und des lokalen Jump-Hosts mit in die Tauri-freie Funktion, vor jedes
  Lesen aus Profil- oder Credential-Store (A6/A7, T15).

## 6. Sicherheits-Invarianten

- **Credential-Handling:** kein Secret-Inhalt in Log, Fehlermeldung oder
  Event; Refs dürfen geloggt werden (wie heute in
  `delete_user_requested_secret`). T10/T14 prüfen das am aufgezeichneten Log.
- **Lokaler Server / Jump-Host:** A6, geprüft durch T15, T15b.
- `create_server` behält seinen vollständigen Rückweg (Nicht-Ziel, keine
  Änderung).
- Filter, Risiko, Ausführungspfad, MCP: nicht berührt.

## 7. Tests

Jeder Test fährt den **Bearbeiten-Ablauf als Ganzes** (Auflösen der
Anmeldeart, Sudo-Passwort, Schreiben der Datenbank). Ausgangszustand, wo
nicht anders genannt: Server mit `Password`, Passwort hinterlegt,
In-Memory-Profilspeicher.

- **T1** (M1): → Zertifikat, beide Felder leer →
  `SERVER_CERTIFICATE_REQUIRED`, Passwort vorhanden, DB trägt `Password`.
  Scheitert heute.
- **T2** (M2): → Private Key, `set` für `private_key` scheitert → Fehler,
  Passwort vorhanden. Scheitert heute.
- **T3** (M3): → Agent, `set` für `sudo_password` scheitert → Fehler,
  Passwort vorhanden, DB trägt `Password`. Scheitert heute.
- **T4**: → Agent, Schreiben der Datenbank scheitert → Fehler, Passwort
  vorhanden. Scheitert heute.
- **T5** (M4): → Zertifikat, nur Zertifikat →
  `SERVER_CERTIFICATE_KEY_REQUIRED`, Passwort vorhanden, **kein** Eintrag
  `certificate`. Scheitert heute an beiden Prüfungen.
- **T6** (M5): `PrivateKey` mit Passphrase → Schlüsseldatei mit **neuer**
  Passphrase, Erfolg → `passphrase` mit neuem Wert vorhanden,
  `private_key` entfernt, DB trägt `IdentityFile` mit diesem Ref. Scheitert,
  sobald A2 den gemeinsamen Ref löscht.
- **T7**: wie T6 ohne neue Passphrase → Erfolg, `private_key` **und**
  `passphrase` entfernt, DB ohne Passphrase-Ref.
- **T8**: → Agent, Erfolg → Passwort-Eintrag entfernt (A5). Scheitert, wenn
  das Aufräumen ganz entfällt. Nachfolger von
  `test_update_kind_change_cleans_up_abandoned_slot`.
- **T9**: gleiche Art, Feld leer, Erfolg → Passwort unverändert vorhanden.
- **T10** (A4): → Agent, Erfolg, `delete` scheitert → Erfolg, DB trägt
  `Agent`, Warnung mit dem Ref im Log, der Passwortwert **nicht**.
  Scheitert heute an der fehlenden Warnung.
- **T11** (M5, Gegenrichtung): `IdentityFile` mit Passphrase → Private Key
  mit neuem Schlüssel und **neuer** Passphrase, Erfolg → `passphrase` mit
  neuem Wert, DB trägt `PrivateKey` mit diesem Ref.
- **T12**: `IdentityFile` mit Passphrase → Agent, Erfolg → `passphrase`
  entfernt. Nachfolger von
  `test_switching_away_from_an_identity_file_cleans_up_the_passphrase_slot`.
- **T13** (A3, Sudo): `Password` und Sudo-Passwort hinterlegt → Agent mit
  neuem Sudo-Passwort, DB scheitert → Fehler, Passwort vorhanden,
  Sudo-Eintrag vorhanden (neuer Wert, R1). Fängt einen Rückweg, der wie
  beim Anlegen alle Slots abräumt.
- **T14** (A4 auf dem Rückweg): wie T5, zusätzlich scheitert jedes
  `delete` → Code bleibt `SERVER_CERTIFICATE_KEY_REQUIRED`, Passwort
  vorhanden, Warnung mit Ref `…:certificate`. Fängt einen Rückweg, der den
  Löschfehler per `?` weiterreicht.
- **T15** (A6): Bearbeiten mit lokalem Jump-Host, Wechsel auf Private Key
  mit Schlüssel **und** neuem Sudo-Passwort → Ablehnung wie heute;
  `password` unverändert, kein Eintrag `private_key`, kein `sudo_password`,
  `get_calls() == 0` (abgefragt vor jeder eigenen Prüfung, die selbst
  liest). Der Store läuft mit `with_failing_delete()`, damit ein zu früh
  geschriebener Eintrag nicht vom Rückweg (A3) verwischt wird. T15b:
  dasselbe für die ID des lokalen Servers. Scheitert, wenn eine Ablehnung
  hinter die Credential-Auflösung rutscht.
- **T16** (R1 festhalten): gleiche Art `Password` mit **neuem** Passwort,
  DB scheitert → Fehler, Eintrag trägt den neuen Wert, nichts gelöscht.
  Scheitert, wenn ein Rückweg den gerade überschriebenen gemeinsamen Ref
  als „neu geschrieben" wieder entfernt.
- **T17** (A3, Fehler nach dem Schreiben): → Private Key mit Schlüssel,
  Datenbank scheitert → Fehler, Passwort vorhanden, **kein** `private_key`.
  T17b: dasselbe, aber `set` für `sudo_password` scheitert (neues
  Sudo-Passwort angegeben). Fängt einen Rückweg, der nur innerhalb der
  Credential-Auflösung aufräumt.
- **T18** (M5 auf dem Fehlerweg): `PrivateKey` mit Passphrase →
  Schlüsseldatei mit **neuer** Passphrase, Datenbank scheitert → Fehler,
  `private_key` vorhanden, `passphrase` vorhanden (neuer Wert, R1), DB
  unverändert. Fängt einen Rückweg, der „in diesem Aufruf geschrieben"
  ohne Abgleich mit den alten Refs entfernt.

## 8. Offene Punkte

Keine.

**R1 (bewusst nicht behoben):** Nutzen alte und neue Art denselben Ref
(gleiche Art mit neuem Wert, oder die Passphrase in M5), überschreibt der
Aufruf den alten Wert vor dem Datenbank-Schreiben; scheitert danach etwas,
bleibt der neue Wert stehen. Bei gleicher Art ist das der gewollte Wert.
Bei M5 lässt sich die bisherige Anmeldung danach nicht mehr entsperren —
nur wenn Wechsel Private Key ↔ Schlüsseldatei, neue Passphrase und ein
Fehler danach zusammenkommen. Dasselbe gilt für ein neues Sudo-Passwort.
Zurücksetzen hieße, bei jedem Speichern den alten Wert vorher auszulesen.

**R2 (bewusst nicht behoben):** Zwei gleichzeitige Speichervorgänge
desselben Servers rechnen mit ihrem Anfangsstand; der spätere könnte einen
Ref löschen, den der frühere gerade geschrieben hat. Die Anmeldeart ändern
nur Speichern und `convert_identity_file_to_keychain`, beide aus demselben
Formular; MCP kann es nicht.

## 9. Klarstellungen

**K1 — `sudo_password` ist auch beim Aufräumen nach Erfolg ausgenommen**
(Coder, 2026-09-28, auf einen Fund des spec-reviewers). A3 nennt den Slot
ausdrücklich für den Rückweg; A2/A5 nennen ihn nicht, weil sie ihn
stillschweigend als nicht zur `AuthMethod` gehörig voraussetzen — unter
dieser Annahme sind die Mengen ohnehin disjunkt und die Ausnahme ein
No-op. Der Code führt sie trotzdem auf beiden Wegen, damit die Annahme
nicht bloß gilt, sondern geprüft wird: Verweist eine gespeicherte
`AuthMethod` doch einmal auf `server:{id}:sudo_password` (von Hand
veränderte Zeile, künftige Variante mit demselben Slot-Namen), löschte der
Erfolgsweg sonst das Sudo-Passwort, das derselbe Aufruf gerade geschrieben
hat. Die Kehrseite — in eben diesem Zustand bleibt ohne neues
Sudo-Passwort ein verwaister Eintrag stehen — ist der gewollte Tausch:
ein Eintrag zu viel statt ein Credential zu wenig. Produktiv unerreichbar,
`resolve_auth_method` vergibt nur `password`, `private_key`, `passphrase`,
`certificate`, `certificate_key`. S. `docs/adr/0081`.

## Umsetzung

**Teil 0:** entfällt.

**Reihenfolge:**
1. `refactor(app-logic): move the server edit flow out of the Tauri command (spec 0082) [BL-0252]` — A7; Ablauf und Reihenfolge der Prüfungen unverändert, bestehende Tests grün.
2. `test(app-logic): pin down credential loss when saving an edited server fails (spec 0082) [BL-0252]` — T1–T18, rot bzw. als Wächter; im Bericht, welche heute rot sind.
3. `fix(app-logic): keep the previous credentials when saving an edited server fails (spec 0082) [BL-0252]` — A1–A5; die beiden bestehenden Tests ziehen in T8/T12 um, keiner fällt vor seinem Nachfolger.
4. `docs(app-logic): describe the new cleanup order at the credential resolution (spec 0082) [BL-0252]` — Doc-Kommentare an Auflösung und Aufräumen.
5. `docs(changelog): add fragment for keeping credentials on a failed edit [BL-0252]` — „Beim Wechsel der Anmeldeart gingen die bisherigen Zugangsdaten verloren, wenn das Speichern scheiterte."

**Priorität:** ERHÖHT. Angriffsrichtungen für den Review:
- Ein Fehlerweg nach dem ersten Schlüsselbund-Schreiben, der den Rückweg
  aus A3 umgeht (frühes `return`, `?`) — T5, T14, T17, T17b.
- A3 trifft einen Ref der bisherigen Art, weil alte und neue Art denselben
  Slot-Namen nutzen (M5, `Password → Password`) — T16, T18.
- A2 löscht etwas, auf das die neue `AuthMethod` verweist — T6, T11.
- Eine neue Logzeile trägt Secret-Inhalt statt nur des Refs — T10, T14.
- Der Umbau zieht einen Schlüsselbund-Zugriff vor die Ablehnung des
  lokalen Servers bzw. Jump-Hosts — T15, T15b.
- Der Rückweg räumt wie beim Anlegen alle Slots des Servers ab,
  `sudo_password` eingeschlossen — T13.

**Aufteilung:** ein Lauf, **Opus** (Credential-Handling), spec-reviewer
ERHÖHT. Die Schritte hängen aneinander (Test braucht Schritt 1, Fix
braucht die Tests); Schritt 4–5 sind zu klein für einen eigenen Lauf.

**Berührte Module:** `crates/app-logic` (`server_credentials.rs`,
`servers.rs`, Tests, ggf. `test_support.rs`),
`crates/app-shell/src/commands/servers.rs`,
`changelog.d/`.

**Melde zurück:** welche Tests heute rot waren (Gegenbeweis nach
Repo-`CLAUDE.md`), wie der Rückweg die in diesem Aufruf geschriebenen Refs
kennt, manueller Testablauf: Server mit Passwort bearbeiten → „Zertifikat",
Felder leer, speichern → Fehlermeldung; Dialog schließen, verbinden →
gelingt mit dem alten Passwort.
