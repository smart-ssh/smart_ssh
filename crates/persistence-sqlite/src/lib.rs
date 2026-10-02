//! SQLite-Persistenzschicht für `ssh-manager-core::profiles`.
//!
//! Setzt `docs/specs/0004-sqlite-persistence.md` um: implementiert den
//! `ProfileStore`-Trait aus `core::profiles` (Spec 0003) gegen eine lokale
//! SQLite-Datenbank via `sqlx`. Bewusst eine eigene Crate statt Teil von
//! `ssh-manager-core` — siehe Spec 0004 Abschnitt 1: `core` bleibt frei von
//! I/O-Abhängigkeiten, austauschbare Storage-Details gehören hierher.

mod ai_provider_store;
mod chat_session_store;
mod error;
mod ledger_store;
mod mapping;
mod paths;
mod policy_store;
mod prompt_history_store;
mod store;

#[cfg(test)]
mod tests;
/// Spec 0101, T0 — eigene Datei: erzeugt (einmalig, von Hand) und prüft die
/// eingecheckte Datenbank-Fixture `tests/fixtures/t0-pre-sqlcipher.sqlite3`,
/// Grundlage für T4–T6 (Commit 4).
#[cfg(test)]
mod tests_fixture_t0;
/// Spec 0096, A3/A4 — eigene Datei statt in `tests`: der Nachweis liest die
/// Datenbankdateien roh, an SQLite vorbei, und teilt mit der
/// `SqliteProfileStore`-Testsuite dort weder Helfer noch Aufbaumuster.
#[cfg(test)]
mod tests_raw_file;

pub use ai_provider_store::{
    AiProviderConfig, AiProviderConfigUpdate, AiProviderStoreError, SqliteAiProviderStore,
};
pub use chat_session_store::{ChatSessionStoreError, ChatSessionSummary, SqliteChatSessionStore};
pub use error::{ConnectFailureKind, PersistenceError, PersistenceResult};
pub use ledger_store::{LedgerEntry, LedgerStoreError, SqliteLedgerStore};
pub use paths::default_db_path;
pub use policy_store::{PolicyStoreError, SqlitePolicyStore, StoredRule};
pub use prompt_history_store::{PromptHistoryStoreError, SqlitePromptHistoryStore};
pub use store::SqliteProfileStore;
