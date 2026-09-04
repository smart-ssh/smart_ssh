# 0026-local-pseudo-server-design

## Status
Vorgeschlagen

## Kontext

`docs/specs/0032-local-pseudo-server.md` verlangt einen immer vorhandenen
"Localhost"-Eintrag, der über dieselben `SshTransport`/`InteractiveShell`/
`SftpSession`-Traits (Spec 0005/0020) läuft wie ein echter Server, aber
ohne SSH-Verbindung, ohne `servers`-Zeile und ohne Host-Key-Behandlung.
Die Spec lässt offen, wie genau Notizen/Tags (Abschnitt 3: "bleiben
editierbar") und die Windows-PTY-Unterstützung konkret umgesetzt werden.

## Entscheidungen

**1. `LocalTransport`/`LocalShell`/`LocalFileSession` implementieren die
bestehenden Traits über lokale Prozessausführung/`portable-pty`/
`tokio::fs`, ohne jede Änderung an Kernschleife, Filter-Engine oder
Risiko-Klassifizierung.** Das ist der in Spec 0032 Abschnitt 2 explizit
genannte Architektur-Vorteil: weil sämtliche sicherheitsrelevante Logik
ausschließlich gegen die Trait-Grenze programmiert ist, braucht der lokale
Pseudo-Server keine Sonderbehandlung an genau der Stelle, an der ein
Bypass am gefährlichsten wäre. Verifiziert über einen dedizierten Test
(`crate::local_server::tests::test_filter_engine_evaluates_local_pseudo_
server_like_any_real_server`), der dieselbe `Scope::Server`-Regel gegen
die lokale und eine echte `ServerId` auswertet und identisches Verhalten
erwartet.

**2. Notizen/Tags des lokalen Pseudo-Servers laufen über
`tauri-plugin-store` (`settings.json`), nicht über
`record_note_revision`/`server_tags`.** Beide bestehenden Mechanismen
setzen eine existierende `servers`-Zeile voraus:
`server_tags.server_id` trägt einen `FOREIGN KEY ... ON DELETE CASCADE`
auf `servers(id)` bei aktivem `PRAGMA foreign_keys = ON`
(`crates/persistence-sqlite/src/store.rs`), und
`SqliteProfileStore::record_note_revision` prüft nach dem
`UPDATE servers SET notes = ... WHERE id = ?` explizit
`rows_affected() == 0` und rollt die gesamte Transaktion zurück
(`ProfileError::ServerNotFound`). Für den lokalen Pseudo-Server (keine
`servers`-Zeile, Abschnitt 3) schlägt dieser Pfad also grundsätzlich fehl.
Statt das Schema (z. B. eine nullable FK oder eine Ausnahme-Zeile für die
Nil-UUID) oder die Transaktionslogik anzupassen — beides würde eine
Invariante aufweichen, die für alle **echten** Server weiterhin gelten
soll —, speichert `crate::local_server` Notizen/Tags als einfache
Aktuell-Wert-Felder im selben Settings-Store wie andere reine
App-Einstellungen (Spec-0024-Muster). **Bewusster Funktionsverzicht:**
der lokale Pseudo-Server hat dadurch **keine Notiz-Revisions-Historie**
und kein Rollback — nur einen editierbaren Freitext-Stand. Die Spec
verlangt ausdrücklich nur "editierbar", nicht "mit Historie"; eigene
Tauri-Commands (`update_local_server_notes`/`update_local_server_tags`)
kapseln das, das Frontend zeigt für `isLocal: true` einen einfachen
Notiz-Editor statt der `NotesPanel`-Historie-Komponente.

**3. `connect_session` verzweigt früh auf `LOCAL_SERVER_ID`, noch vor
`profile_store.get_server`/`resolve_connection_target`.** Der lokale
Pseudo-Server hat keine DB-Zeile, keinen Host-Key und keine Credentials —
ein Aufruf von `ssh_transport::connect()` wäre hier fachlich falsch
(nichts davon existiert). Der Rest des Verbindungsaufbaus (AI-Provider,
`system_context`, `Session`-Konstruktion) bleibt unverändert und läuft
mit einem zur Laufzeit synthetisierten `Server` (`AuthMethod::Agent` als
bedeutungsloser Platzhalter, `group_id`/`jump_host: None`).

**4. `LOCAL_SERVER_ID` ist die Nil-UUID (`Uuid::nil()`), als `const fn`
zur Compile-Zeit reserviert.** `ServerId::new()` erzeugt ausschließlich
UUIDv4-Werte; die Nil-UUID kann dadurch nie mit einer echten Server-ID
kollidieren, ganz ohne zusätzliches Registry/Sentinel-Konzept.

**5. Windows-ConPTY-Unterstützung von `portable-pty` 0.9: als bekannte
Einschränkung dokumentiert, nicht selbst gefixt.** `portable-pty` 0.9
unterstützt ConPTY, setzt aber (Stand dieser Implementierung, laut
öffentlicher Diskussion rund um das Crate, u. a. im Fork `psmux`) einige
modernere ConPTY-Erstellungs-Flags (`PSEUDOCONSOLE_RESIZE_QUIRK`,
`PSEUDOCONSOLE_WIN32_INPUT_MODE`, `PSEUDOCONSOLE_PASSTHROUGH_MODE`) nicht.
Dieses Verhalten liegt vollständig in der Abhängigkeit selbst; ein Fix
außerhalb dieses Projekts wäre nötig. Aus macOS-Entwicklungsumgebung
heraus ohnehin nicht verifizierbar — dokumentiert statt (ungetestet)
"behoben" behauptet.

**6. Der lokale Pseudo-Server hat unveränderlich `PostIngestPolicy::
Balanced` (`crate::local_server::synthetic_server`, s. dortiger
Kommentar) statt einer über die App einstellbaren Stufe (Spec 0039,
Abschnitt 5).** Wie bei Notizen/Tags (Entscheidung 2) hat der lokale
Pseudo-Server keine `servers`-Zeile — `PostIngestPolicy` lebt aber als
Spalte auf genau dieser Zeile (`persistence-sqlite`, `servers.post_ingest_
policy`), es gibt also strukturell keinen Speicherort für einen
nutzerdefinierten Wert, ohne entweder (a) eine dritte Instanz des
Settings-Store-Musters aus Entscheidung 2 nur für dieses eine Feld
einzuführen, oder (b) doch eine echte, leere `servers`-Zeile für den
lokalen Pseudo-Server anzulegen — Letzteres würde genau die in
Entscheidung 2 bewusst vermiedene Komplikation zurückholen (FK-Ziel für
`server_tags`, `record_note_revision`-Zielzeile), nur für ein einzelnes
Enum-Feld. **Bewusst nicht (a) gebaut:** anders als Notizen/Tags (die der
Spec nach explizit "editierbar" sein müssen) verlangt Spec 0032 an
keiner Stelle eine einstellbare Eskalationsstufe für den lokalen
Pseudo-Server — `Balanced` (der App-weite Default für jeden neuen Server,
s. `PostIngestPolicy::default()`) ist eine sachlich vertretbare, in der
Spec nicht widersprochene Wahl, kein Behelfsfix.

**Warum gerade `Balanced` und nicht `Strict`:** die Bedrohung, gegen die
Spec 0039 Abschnitt 5 primär gerichtet ist — über den KI-Kontext
eingeschleuste Anweisungen aus **fremdem** Serverinhalt, der den Nutzer zu
riskanten Folgeaktionen bewegt —, ist beim lokalen Pseudo-Server strukturell
abgeschwächt: "Serverinhalt" ist hier der eigene Rechner des Nutzers, nicht
ein potenziell kompromittierter Drittserver. `Strict` als unveränderlicher
Zwang hätte lediglich jede Folgeaktion nach dem ersten lokalen Lesebefehl
generisch verlangsamt, ohne dass die Spec das für diesen Fall verlangt.

## Konsequenzen

**Positiv:**
- Kernschleife/Filter-Engine/Risiko-Klassifizierung bleiben exakt so
  vertrauenswürdig für den lokalen Pseudo-Server wie für jeden echten
  Server — durch Trait-Wiederverwendung strukturell erzwungen, nicht nur
  durch Konvention.
- Keine Schema-Änderung, keine Aufweichung der
  `record_note_revision`-Invariante ("jede Notiz-Änderung hat eine
  existierende Ziel-Zeile") für echte Server/Gruppen.

**Negativ / Trade-off:**
- Der lokale Pseudo-Server hat keine Notiz-Historie/kein Rollback —
  inkonsistent zu Servern/Gruppen, aber spec-konform und über einen
  dedizierten, klar benannten Command-Pfad sichtbar gemacht statt still
  im bestehenden Pfad mitzulaufen.
- Windows-ConPTY-Verhalten für den interaktiven lokalen Shell-Modus ist
  auf Basis der aktuellen `portable-pty`-Version möglicherweise
  eingeschränkt (fehlende moderne Flags) — nicht in dieser
  Entwicklungsumgebung nachstellbar, daher nur dokumentiert.
- Ein Nutzer, der für den lokalen Pseudo-Server bewusst `Strict` oder
  `Standard` statt `Balanced` möchte, kann das aktuell nicht einstellen —
  dieselbe strukturelle Grenze wie bei der fehlenden Notiz-Historie
  (Entscheidung 2), hier aber ohne eigenen Command-Pfad, der die
  Einschränkung im UI sichtbar macht (kein Formularfeld existiert
  überhaupt, das man als deaktiviert anzeigen könnte).
