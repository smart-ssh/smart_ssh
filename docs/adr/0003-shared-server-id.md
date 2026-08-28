# 0003-shared-server-id

## Status
Accepted

## Kontext

Beim Implementieren von `crates/core/src/profiles/` (Spec 0003) sollte
`ServerId` — analog zur bereits geklärten Tag/`Scope`-Kompatibilität mit
Spec 0002 — nicht dupliziert, sondern zwischen `filter` und `profiles`
geteilt werden.

Beim Nachsehen stellte sich heraus, dass es keinen bereits existierenden
`ServerId`-Typ zum Wiederverwenden gab, sondern zwei **inkompatible**
Definitionen:

- `crate::filter` hatte aus der vorherigen Implementierung (Spec 0002) einen
  eigenen, String-basierten Platzhalter:
  `pub struct ServerId(pub String);`, mit dem Kommentar, dass er ersetzt
  werden sollte, sobald ein kanonischer Typ existiert.
- `docs/specs/0003-server-profile-datenmodell.md`, Abschnitt 3, definiert
  `ServerId` dagegen als `pub struct ServerId(Uuid);` — Uuid-basiert, weil
  ein `Server`-Datensatz seine Identität dort tatsächlich bekommt (Primary
  Key eines künftigen DB-Schemas).

Beide Module referenzieren dasselbe reale Konzept (die Identität eines
Servers, auf den sowohl eine Filter-Regel als auch ein Server-Profil
verweisen), brauchten also zwingend denselben Rust-Typ, nicht bloß zwei
gleich benannte.

## Entscheidung

`ServerId` lebt jetzt in einem neuen Modul `crate::shared`
(`crates/core/src/shared/mod.rs`), Uuid-basiert (wie in Spec 0003
Abschnitt 3 vorgegeben — `profiles` gilt als kanonischer Ort für die
Server-Identität, da dort das eigentliche `Server`-Profil liegt). Sowohl
`profiles::Server::id`/`jump_host` als auch `filter::EvalContext::server_id`
und `filter::Scope::Server(..)` nutzen jetzt exakt diesen einen Typ.

Der bisherige String-basierte Platzhalter in `filter::types` wurde entfernt;
`filter::mod` re-exportiert `ServerId` stattdessen direkt aus
`crate::shared`, sodass sich für externe Aufrufer der öffentliche Pfad
`ssh_manager_core::filter::ServerId` nicht ändert.

Als Folge musste `filter`s Testsuite angepasst werden: der `ctx()`-Test-Helper
konstruierte vorher `ServerId(server.to_string())` aus einem Label wie
`"srv1"`; das funktioniert mit einem Uuid-Wrapper nicht mehr. Da kein
bestehender Filter-Test tatsächlich auf einen konkreten `ServerId`-Wert prüft
(keiner nutzt `Scope::Server(..)`-Regeln), wurde der Helper auf eine frische,
zufällige `ServerId::new()` umgestellt; der Label-Parameter bleibt aus
Lesbarkeitsgründen an den Call-Sites erhalten, wird intern aber ignoriert.

`GroupId` (nur in `profiles` gebraucht, `filter` kennt das Konzept "Gruppe"
nicht) bleibt bewusst lokal in `profiles` und wandert nicht nach
`crate::shared`.

## Konsequenzen

**Positiv:**
- Nur eine Definition von "was ist ein Server" — kein Risiko, dass sich
  `filter::ServerId` und `profiles::ServerId` künftig durch unabhängige
  Änderungen auseinanderentwickeln.
- `EvalContext::server_id` kann direkt mit `Server::id` aus `profiles`
  verglichen/befüllt werden, ohne Konvertierung — wichtig für den späteren
  Aufrufer (z. B. `app-tauri` oder eine Session-Orchestrierung), der beim
  Start einer SSH-Session sowohl das Server-Profil lädt als auch die
  Filter-Engine mit demselben `server_id` füttert.
- `Uuid` ist `Copy`, was an mehreren Stellen (`EffectiveScope::from`,
  Vergleiche in `ProfileStore`) unnötige `.clone()`-Aufrufe erspart
  (von Clippy im Zuge dieser Änderung auch aktiv angemahnt).

**Negativ / Trade-off:**
- Cross-Modul-Kopplung: `filter` hat jetzt eine Kompilierzeit-Abhängigkeit
  auf `crate::shared` (vorher war `filter` bzgl. `ServerId` komplett
  eigenständig). Das ist beabsichtigt und unproblematisch, solange
  `core::shared` klein und stabil bleibt — sollte dort aber nicht zu einer
  Sammelstelle für beliebige "irgendwie geteilte" Typen ausufern.
- Die Testfall-Anpassung in `filter::tests` (`ctx()`-Helper) war eine
  reine Mechanik ohne Verhaltensänderung, aber ein Beispiel dafür, dass eine
  scheinbar lokale Typänderung in `profiles` bereits bestehenden Code in
  `filter` berührt — bei künftigen Änderungen an `crate::shared::ServerId`
  ist das erneut zu erwarten.
