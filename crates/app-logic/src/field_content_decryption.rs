//! Issue #113 (ADR 0112): der Startschritt, der die früher feldweise
//! verschlüsselten Spalten (Chat, Ledger, Prompt-Historie,
//! Zusammenfassungen) einmalig auf Klartext in der verschlüsselten
//! Datenbank umstellt.
//!
//! Läuft direkt nach dem Öffnen der Datenbank und **vor** allem, was einen
//! dieser Stores benutzt. Die Arbeit selbst liegt in
//! `persistence_sqlite::SqliteProfileStore::decrypt_field_encrypted_content`
//! (eine Transaktion, alles oder nichts); hier entscheidet sich nur, was der
//! Nutzer davon sieht:
//!
//! - Waren Einträge mit dem aktuellen K nicht lesbar und sind entfernt,
//!   kommt **einmal** ein Hinweis mit ihrer Anzahl. „Einmal", weil die
//!   Umstellung danach auf `done` steht und nie wieder läuft.
//! - Scheitert die Umstellung, endet der Start sichtbar
//!   ([`ConnectFailureKind::FieldContentDecryptionFailed`]) — nie ein
//!   Weiterlauf mit Spalten, die kein Store mehr lesen kann.

use persistence_sqlite::{ConnectFailureKind, FieldContentDecryption, SqliteProfileStore};

use crate::database_startup::{StartupAbort, StartupPrompt};

/// S. Moduldoc. `root_key` ist K, mit dem die Datenbank gerade geöffnet
/// wurde.
pub async fn decrypt_field_encrypted_content(
    store: &SqliteProfileStore,
    root_key: &[u8; 32],
    prompt: &dyn StartupPrompt,
) -> Result<(), StartupAbort> {
    let outcome = store
        .decrypt_field_encrypted_content(root_key)
        .await
        .map_err(|err| {
            tracing::error!(
                error = %err,
                "converting the field-encrypted columns failed; nothing was changed (Issue #113)"
            );
            StartupAbort::Fatal {
                kind: ConnectFailureKind::FieldContentDecryptionFailed,
                detail: format!("Umstellung der feldweise verschlüsselten Spalten: {err}"),
            }
        })?;
    match outcome {
        FieldContentDecryption::AlreadyDone => {}
        FieldContentDecryption::Completed(report) => {
            tracing::info!(
                decrypted = report.decrypted_total(),
                removed = report.removed_total(),
                removed_chat_messages = report.chat_messages.removed,
                removed_ledger_entries = report.ledger_entries.removed,
                removed_prompt_history = report.prompt_history.removed,
                removed_summaries = report.summaries.removed,
                "field-encrypted columns converted to plaintext inside the encrypted database"
            );
            if report.removed_total() > 0 {
                prompt.notify_unreadable_history_removed(report.removed_total());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
