//! Central redaction of ledger entry contents (Spec 0057, §1.2).

use ssh_manager_core::ai::OutputRedactor;
use ssh_manager_core::audit::LedgerEntryContent;

/// Redigiert alle Inhalts-tragenden Varianten eines [`LedgerEntryContent`]
/// (Spec 0057, §1.2 "PFLICHT") — zentral hier statt an jeder Aufrufstelle
/// von [`emit_activity`] einzeln, damit ein vergessener Aufrufer
/// keinen unredigierten Eintrag durchlassen kann. `Decision` trägt keine
/// Ausgabedaten (nur `reason`/`code`, von der Filter-Engine selbst
/// erzeugte Texte, kein Serverinhalt) und bleibt deshalb unverändert.
pub(super) fn redact_activity_content(
    redactor: &dyn OutputRedactor,
    content: LedgerEntryContent,
) -> LedgerEntryContent {
    match content {
        LedgerEntryContent::CommandProposed { command } => LedgerEntryContent::CommandProposed {
            command: redactor.redact_text(&command),
        },
        LedgerEntryContent::CommandExecuted {
            command,
            output,
            cancelled,
        } => LedgerEntryContent::CommandExecuted {
            command: redactor.redact_text(&command),
            output: redactor.redact(&output),
            cancelled,
        },
        LedgerEntryContent::AiMessage { text } => LedgerEntryContent::AiMessage {
            text: redactor.redact_text(&text),
        },
        LedgerEntryContent::FileRead {
            path,
            bytes,
            sha256,
        } => LedgerEntryContent::FileRead {
            path: redactor.redact_text(&path),
            bytes,
            sha256,
        },
        // Only the paths can carry a secret; sizes and hashes are not content.
        LedgerEntryContent::FileWritten {
            path,
            backup_path,
            bytes_before,
            bytes_after,
            sha256_before,
            sha256_after,
            sudo,
        } => LedgerEntryContent::FileWritten {
            path: redactor.redact_text(&path),
            backup_path: backup_path.map(|b| redactor.redact_text(&b)),
            bytes_before,
            bytes_after,
            sha256_before,
            sha256_after,
            sudo,
        },
        LedgerEntryContent::Connection { event, detail } => LedgerEntryContent::Connection {
            event,
            detail: detail.map(|d| redactor.redact_text(&d)),
        },
        // No content at all.
        tick @ LedgerEntryContent::ActivityTick => tick,
        decision @ LedgerEntryContent::Decision { .. } => decision,
    }
}
