//! Typen, die von mehreren `core`-Modulen gemeinsam gebraucht werden.
//!
//! Aktuell nur [`ServerId`]: sowohl `profiles` (wo ein [`Server`] seine
//! Identität besitzt, siehe `docs/specs/0003-server-profile-datenmodell.md`)
//! als auch `filter` (das über `EvalContext`/`Scope::Server` auf Server
//! verweist, siehe `docs/specs/0002-filter-engine-spec.md`) brauchen exakt
//! denselben Typ, nicht bloß zwei zufällig gleich benannte. `filter` hatte
//! vorher eine eigene, String-basierte Platzhalter-`ServerId`; die wurde
//! entfernt und durch diesen gemeinsamen Typ ersetzt (Uuid-basiert, wie in
//! Spec 0003 Abschnitt 3 vorgegeben — `profiles` ist der Ort, an dem ein
//! Server seine Identität tatsächlich bekommt, daher ist dessen Definition
//! kanonisch). Siehe `docs/adr/0003-shared-server-id.md`.
//!
//! `Tag`-Werte (Spec 0002 `Scope::Tag`, Spec 0003 `Server.tags`) brauchen
//! dagegen **keinen** gemeinsamen Typ: beide Specs definieren sie bereits
//! identisch als reinen `String` — es gibt keine zweite Repräsentation, die
//! zusammengeführt werden müsste, `Vec<String>`/`String` sind implizit schon
//! "derselbe Typ". Ein eigener `Tag`-Newtype würde hier nur Umwandlungscode
//! ohne zusätzlichen Nutzen erzeugen.
//! [`Server`]: crate::profiles::Server

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Eindeutige Kennung eines Servers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ServerId(pub Uuid);

impl ServerId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for ServerId {
    fn default() -> Self {
        Self::new()
    }
}
