//! `russh`-gestützte Implementierung von `ssh_manager_core::ssh` (Spec
//! 0005). Konkrete, austauschbare SSH-Bibliotheks-Anbindung — `core` selbst
//! kennt nur die Traits (`SshTransport`, `HostKeyStore`, ...).

mod auth;
mod connect;
mod error;
mod exec;
mod handler;
mod host_key;
mod local;
mod local_sftp;
mod sftp;
mod shell;
mod transport;

#[cfg(test)]
mod tests;

pub use connect::{connect, ConnectOutcome};
pub use local::LocalTransport;
pub use local_sftp::LocalFileSession;
pub use transport::RusshTransport;
