//! Credential-Verwaltung (SSH-Keys, Passwörter, Passphrasen).
//!
//! Geplanter Inhalt:
//! - Sichere Ablage von Zugangsdaten (OS-Keychain / verschlüsselter Store)
//! - SSH-Key-Import/-Generierung, Passphrase-Handling
//! - Zuordnung Credential <-> Server/Projekt
//! - Verschlüsselung at-rest, Zugriffsschutz
//!
//! Hinweis: `CredentialRef` und der `CredentialStore`-Trait (Abschnitt 4 von
//! `docs/specs/0003-server-profile-datenmodell.md`) sind vorerst in
//! [`crate::profiles`] definiert, nicht hier — dort werden sie als Teil des
//! Server-Profil-Datenmodells gebraucht. Sobald eine konkrete, OS-Keychain-
//! gestützte `CredentialStore`-Implementierung entsteht (`keyring`-Crate,
//! s. Spec 0001 Abschnitt 2), gehört sie hierher; ob die Trait-/Typ-
//! Definitionen dann mitwandern oder in `profiles` bleiben und nur
//! re-exportiert werden, ist noch offen. Siehe
//! `docs/adr/0005-credential-store-in-profiles.md`.
