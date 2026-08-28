//! Server-Profile, Gruppen, Credentials & LLM-Kontextnotizen.
//!
//! Setzt `docs/specs/0003-server-profile-datenmodell.md` um. Reine
//! Datenmodell-Logik, keine konkrete Persistenz (DB/Keychain) in diesem
//! Schritt — dafür sorgen `CredentialStore`/`ProfileStore` als Traits,
//! analog zu `PolicyStore` in `crate::filter` (Spec 0002).

mod credentials;
mod notes;
mod store;
mod types;

#[cfg(test)]
mod tests;

pub use credentials::{CredentialError, CredentialResult, CredentialStore};
pub use notes::{effective_notes, record_revision};
pub use store::{ProfileError, ProfileResult, ProfileStore};
pub use types::{
    AiAction, AuthMethod, CredentialRef, Group, GroupId, NoteEditor, NoteRevision, NoteTarget,
    Server,
};
