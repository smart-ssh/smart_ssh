//! SSH-Verbindungsmanagement.
//!
//! Geplanter Inhalt:
//! - Verbindungsaufbau/-verwaltung (z. B. via `russh` oder `ssh2`)
//! - Server-/Host-Modelle (Adresse, Port, User, bevorzugte Auth-Methode)
//! - Kommando-Ausführung über eine offene Session (interaktiv & non-interaktiv)
//! - Terminal-/PTY-Handling für interaktive Shells
//! - Verbindungsstatus & Reconnect-Logik
