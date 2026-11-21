//! Central redaction of ledger entry contents (Spec 0057, §1.2).

use ssh_manager_core::ai::OutputRedactor;
use ssh_manager_core::audit::LedgerEntryContent;

/// Redigiert alle Inhalts-tragenden Varianten eines [`LedgerEntryContent`]
/// (Spec 0057, §1.2 "PFLICHT") — zentral hier statt an jeder Aufrufstelle
/// von [`write_ledger_entry`] einzeln, damit ein vergessener Aufrufer
/// keinen unredigierten Eintrag durchlassen kann. `Decision` trägt keine
/// Ausgabedaten (nur `reason`/`code`, von der Filter-Engine selbst
/// erzeugte Texte, kein Serverinhalt) und bleibt deshalb unverändert.
pub(super) fn redact_ledger_entry_content(
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
        LedgerEntryContent::FileRead { path, bytes } => LedgerEntryContent::FileRead {
            path: redactor.redact_text(&path),
            bytes,
        },
        LedgerEntryContent::FileWritten { path, bytes } => LedgerEntryContent::FileWritten {
            path: redactor.redact_text(&path),
            bytes,
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
