//! Issue #113 (ADR 0112): einmalige Umstellung der feldweise verschlüsselten
//! Spalten auf Klartext innerhalb der verschlüsselten Datenbankdatei.
//!
//! Betroffen sind genau vier Spalten (s. [`COLUMNS`]). Eine Altzeile ist
//! eine Zeile, deren Wert die SQLite-Speicherklasse `BLOB` hat: Die Stores
//! haben bis Issue #113 ausschließlich `nonce || ciphertext` als Blob
//! geschrieben und schreiben seitdem ausschließlich `TEXT`. Ein Wert, der
//! schon `TEXT` ist — neu geschrieben oder eine Prompt-Historie-Zeile aus der
//! Zeit vor Spec 0040, die nie verschlüsselt wurde —, bleibt unverändert.
//!
//! **Ablauf, in einer einzigen Transaktion:**
//! 1. Steht `field_content_decryption_state` auf `done`, passiert nichts.
//! 2. Jede Altzeile wird mit dem Wurzelschlüssel K entschlüsselt und als
//!    Klartext zurückgeschrieben.
//! 3. Eine Altzeile, die sich mit dem aktuellen K **nicht** entschlüsseln
//!    lässt (z. B. nach „Neuen Schlüssel erzeugen", Spec 0101 D4), wird
//!    entfernt — Nachricht, Ledger-Eintrag und Historie-Eintrag als Zeile,
//!    die Zusammenfassung als Feld (die Sitzung selbst bleibt). Nichts
//!    bleibt halb lesbar liegen; die Anzahl geht an den Aufrufer, der den
//!    Nutzer einmal darauf hinweist.
//! 4. Der Zustand wird auf `done` gesetzt, dann wird festgeschrieben.
//!
//! Bricht der Lauf irgendwo ab (Absturz, Plattenfehler), rollt SQLite die
//! Transaktion zurück: Jede Zeile hat danach ihren alten Stand, der Zustand
//! steht noch auf `open`, und der nächste Start beginnt von vorn.

use sqlx::Row;

use ssh_manager_core::crypto::legacy_field_content::decrypt_legacy_field_content;

use crate::error::PersistenceResult;
use crate::SqliteProfileStore;

/// Wie viele Altzeilen je Abfrage gelesen werden — begrenzt den Speicher bei
/// langen Chat-Verläufen, ohne die Transaktion aufzuteilen.
const BATCH_SIZE: i64 = 256;

/// Was mit einer nicht entschlüsselbaren Altzeile geschieht.
#[derive(Debug, Clone, Copy)]
enum Unreadable {
    /// Die ganze Zeile entfernen.
    DeleteRow,
    /// Nur die Zusammenfassung entfernen (`summary_text` und die
    /// zugehörige Rundenzahl auf `NULL`) — die Sitzung mit ihren
    /// Nachrichten bleibt.
    ClearSummary,
}

/// Eine der vier Spalten mit früherer feldweiser Verschlüsselung.
#[derive(Debug, Clone, Copy)]
struct Column {
    table: &'static str,
    column: &'static str,
    unreadable: Unreadable,
}

/// Die vier Spalten aus Spec 0036/0040/0057. Feste Namen, nie aus
/// Eingaben — deshalb dürfen sie in die SQL-Texte unten eingesetzt werden.
const COLUMNS: [Column; 4] = [
    Column {
        table: "chat_messages",
        column: "content",
        unreadable: Unreadable::DeleteRow,
    },
    Column {
        table: "ledger_entries",
        column: "content",
        unreadable: Unreadable::DeleteRow,
    },
    Column {
        table: "prompt_history",
        column: "content",
        unreadable: Unreadable::DeleteRow,
    },
    Column {
        table: "chat_sessions",
        column: "summary_text",
        unreadable: Unreadable::ClearSummary,
    },
];

/// Zählung je Spalte.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ColumnCounts {
    /// Entschlüsselt und als Klartext zurückgeschrieben.
    pub decrypted: u64,
    /// Mit dem aktuellen K nicht lesbar und deshalb entfernt.
    pub removed: u64,
}

/// Ergebnis einer Umstellung, die tatsächlich gelaufen ist.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FieldContentDecryptionReport {
    pub chat_messages: ColumnCounts,
    pub ledger_entries: ColumnCounts,
    pub prompt_history: ColumnCounts,
    pub summaries: ColumnCounts,
}

impl FieldContentDecryptionReport {
    /// Alle entfernten Einträge zusammen — die Zahl im Hinweis an den
    /// Nutzer.
    pub fn removed_total(&self) -> u64 {
        self.chat_messages.removed
            + self.ledger_entries.removed
            + self.prompt_history.removed
            + self.summaries.removed
    }

    /// Alle entschlüsselten Einträge zusammen (fürs Log).
    pub fn decrypted_total(&self) -> u64 {
        self.chat_messages.decrypted
            + self.ledger_entries.decrypted
            + self.prompt_history.decrypted
            + self.summaries.decrypted
    }

    fn counts_mut(&mut self, table: &str) -> &mut ColumnCounts {
        match table {
            "chat_messages" => &mut self.chat_messages,
            "ledger_entries" => &mut self.ledger_entries,
            "prompt_history" => &mut self.prompt_history,
            _ => &mut self.summaries,
        }
    }
}

/// Ausgang von [`SqliteProfileStore::decrypt_field_encrypted_content`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldContentDecryption {
    /// Schon bei einem früheren Start abgeschlossen; nichts angefasst.
    AlreadyDone,
    /// In diesem Aufruf gelaufen und festgeschrieben.
    Completed(FieldContentDecryptionReport),
}

impl SqliteProfileStore {
    /// Issue #113: die einmalige Umstellung, s. Moduldoc. `root_key` ist K —
    /// derselbe Schlüssel, aus dem der Datenbankschlüssel abgeleitet ist.
    ///
    /// Ein Fehler lässt die Datenbank im Stand vor dem Aufruf (Rollback);
    /// der Aufrufer bricht dann den Start ab, statt mit unlesbaren Spalten
    /// weiterzulaufen.
    pub async fn decrypt_field_encrypted_content(
        &self,
        root_key: &[u8; 32],
    ) -> PersistenceResult<FieldContentDecryption> {
        self.decrypt_field_encrypted_content_inner(root_key, None)
            .await
    }

    /// Wie [`Self::decrypt_field_encrypted_content`], bricht aber nach
    /// `fail_after` bearbeiteten Altzeilen mit einem Fehler ab — für den
    /// Nachweis, dass ein unterbrochener Lauf nichts verliert.
    #[cfg(any(test, feature = "test-support"))]
    pub async fn decrypt_field_encrypted_content_failing_after(
        &self,
        root_key: &[u8; 32],
        fail_after: u64,
    ) -> PersistenceResult<FieldContentDecryption> {
        self.decrypt_field_encrypted_content_inner(root_key, Some(fail_after))
            .await
    }

    async fn decrypt_field_encrypted_content_inner(
        &self,
        root_key: &[u8; 32],
        fail_after: Option<u64>,
    ) -> PersistenceResult<FieldContentDecryption> {
        let mut tx = self.pool.begin().await?;

        let state: String =
            sqlx::query_scalar("SELECT state FROM field_content_decryption_state WHERE id = 1")
                .fetch_one(&mut *tx)
                .await?;
        if state == "done" {
            tx.rollback().await?;
            return Ok(FieldContentDecryption::AlreadyDone);
        }

        let mut report = FieldContentDecryptionReport::default();
        let mut processed: u64 = 0;
        for column in COLUMNS {
            let select = format!(
                "SELECT id, {col} AS value FROM {table} WHERE typeof({col}) = 'blob' LIMIT ?",
                col = column.column,
                table = column.table,
            );
            let update = format!(
                "UPDATE {table} SET {col} = ? WHERE id = ?",
                col = column.column,
                table = column.table,
            );
            let remove = match column.unreadable {
                Unreadable::DeleteRow => format!("DELETE FROM {} WHERE id = ?", column.table),
                Unreadable::ClearSummary => format!(
                    "UPDATE {} SET summary_text = NULL, summary_rounds_covered = NULL \
                     WHERE id = ?",
                    column.table
                ),
            };

            // Jede bearbeitete Zeile ist danach kein Blob mehr (Klartext
            // geschrieben, entfernt oder auf NULL gesetzt) — die nächste
            // Abfrage liefert also die nächsten, und die Schleife endet.
            loop {
                let rows = sqlx::query(sqlx::AssertSqlSafe(select.clone()))
                    .bind(BATCH_SIZE)
                    .fetch_all(&mut *tx)
                    .await?;
                if rows.is_empty() {
                    break;
                }
                for row in rows {
                    if fail_after == Some(processed) {
                        // Ohne Commit: `tx` rollt beim Verlassen zurück.
                        return Err(sqlx::Error::Protocol(
                            "injected failure during field content decryption".into(),
                        )
                        .into());
                    }
                    let id: String = row.try_get("id")?;
                    let blob: Vec<u8> = row.try_get("value")?;
                    let counts = report.counts_mut(column.table);
                    match decrypt_legacy_field_content(root_key, &blob) {
                        Ok(plaintext) => {
                            sqlx::query(sqlx::AssertSqlSafe(update.clone()))
                                .bind(plaintext)
                                .bind(&id)
                                .execute(&mut *tx)
                                .await?;
                            counts.decrypted += 1;
                        }
                        Err(_) => {
                            sqlx::query(sqlx::AssertSqlSafe(remove.clone()))
                                .bind(&id)
                                .execute(&mut *tx)
                                .await?;
                            counts.removed += 1;
                        }
                    }
                    processed += 1;
                }
            }
        }

        sqlx::query("UPDATE field_content_decryption_state SET state = 'done' WHERE id = 1")
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(FieldContentDecryption::Completed(report))
    }
}
