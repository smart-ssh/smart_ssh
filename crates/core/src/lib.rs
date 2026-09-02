//! ssh-manager core library.
//!
//! UI-unabhängige Geschäftslogik für ssh-manager. Dieses Crate darf keine
//! Abhängigkeit auf ein UI-/App-Framework (z. B. Tauri) haben – es wird von
//! `app-tauri` und potenziell weiteren Frontends (CLI, TUI, ...) genutzt.

pub mod ai;
pub mod audit;
pub mod credentials;
pub mod filter;
pub mod profiles;
pub mod risk;
pub mod shared;
pub mod ssh;
