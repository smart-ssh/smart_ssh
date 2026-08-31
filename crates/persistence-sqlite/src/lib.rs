//! SQLite-Persistenzschicht für `ssh-manager-core::profiles`.
//!
//! Setzt `docs/specs/0004-sqlite-persistence.md` um: implementiert den
//! `ProfileStore`-Trait aus `core::profiles` (Spec 0003) gegen eine lokale
//! SQLite-Datenbank via `sqlx`. Bewusst eine eigene Crate statt Teil von
//! `ssh-manager-core` — siehe Spec 0004 Abschnitt 1: `core` bleibt frei von
//! I/O-Abhängigkeiten, austauschbare Storage-Details gehören hierher.

mod ai_provider_store;
mod error;
mod mapping;
mod paths;
mod policy_store;
mod prompt_history_store;
mod store;

#[cfg(test)]
mod tests;

pub use ai_provider_store::{
    AiProviderConfig, AiProviderConfigUpdate, AiProviderStoreError, SqliteAiProviderStore,
};
pub use error::{PersistenceError, PersistenceResult};
pub use paths::default_db_path;
pub use policy_store::{PolicyStoreError, SqlitePolicyStore, StoredRule};
pub use prompt_history_store::{PromptHistoryStoreError, SqlitePromptHistoryStore};
pub use store::SqliteProfileStore;
