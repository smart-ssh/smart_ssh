//! Von Tauri verwalteter Shared State (Spec 0007, Abschnitt 3).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use persistence_sqlite::SqliteAiProviderStore;
use ssh_manager_core::profiles::{CredentialStore, ProfileStore};
use uuid::Uuid;

/// Platzhalter für eine laufende Terminal-/Chat-Session (`transport`/
/// `ai_provider`/`context`/`filter_engine`, Spec 0007 Abschnitt 3). Kommt
/// erst mit dem Terminal-/Chat-Teil — für Teil 1 reicht ein leerer Marker,
/// damit `AppState.sessions` schon den richtigen Grundriss
/// (`HashMap<SessionId, Session>`) hat, ohne den `SshTransport`/
/// `AiProvider`/`FilterEngine`-Zoo vorzeitig zu verdrahten.
pub struct PlaceholderSession;

/// Eigener Typalias statt direkt `Uuid`: macht Signaturen lesbarer und ist
/// der Ort, an dem eine spätere echte `SessionId`-Newtype (falls gewünscht)
/// eingesetzt würde, ohne Aufrufer-Code anzufassen.
pub type SessionId = Uuid;

pub struct AppState {
    // Wird von keinem Teil-1-Befehl gelesen (kein `connect`/`open_terminal`
    // in diesem Schritt) — deshalb `#[allow(dead_code)]` statt das Feld
    // wegzulassen: der Platz im Grundriss ist bewusst schon da (s.
    // Modul-Kommentar), nur die lesenden Befehle kommen erst mit dem
    // Terminal-/Chat-Teil.
    #[allow(dead_code)]
    pub sessions: Mutex<HashMap<SessionId, PlaceholderSession>>,
    pub profile_store: Arc<dyn ProfileStore>,
    // `CredentialStore` deklariert (anders als `ProfileStore`) keinen
    // `Send + Sync`-Bound im Trait selbst (s. `core::profiles::credentials`)
    // — hier explizit im Objekttyp ergänzt, damit `Arc<dyn CredentialStore
    // + Send + Sync>` als Tauri-`State` (verlangt `Send + Sync + 'static`)
    // taugt, ohne den Trait selbst (der auch synchron/nicht-App-spezifisch
    // bleiben soll) anzufassen.
    pub credential_store: Arc<dyn CredentialStore + Send + Sync>,
    pub ai_provider_store: Arc<SqliteAiProviderStore>,
}
