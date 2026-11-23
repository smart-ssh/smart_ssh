//! The content model of session activity: what was proposed, how it was
//! decided and what was executed, plus the AI messages themselves.
//!
//! The core keeps no record of it (issue #1506, ADR 0161); these types only
//! describe the content that is redacted once and handed to the activity
//! observers (`app-logic::activity`). The `Ledger*` names are kept from the
//! time when this content was stored, to avoid a workspace-wide rename.
//!
//! Pure type definitions — no I/O. Not to be confused with
//! `crate::session::LedgerEntry` (Spec 0037), which keeps only an output
//! digest and is unused target vocabulary.

use serde::{Deserialize, Serialize};

use crate::filter::{RuleId, RuleOrigin};
use crate::ssh::CommandOutput;

/// Wer/was einen Ledger-Eintrag ausgelöst hat (Spec 0057, §1.1: "Quelle
/// (`user`/`ai`/`mcp-agent`) — für spätere Audit-„wer"-Unterscheidung").
///
/// Bewusst ein eigenes Enum, nicht identisch mit `app-shell::dto::
/// ActionOrigin` (kennt nur `Internal`/`Mcp`, lebt außerdem in `app-shell`,
/// nicht in `core`) oder `crate::session::SessionOrigin` (kennt nur
/// `Human`/`McpAgent`, beschreibt wer die ganze *Sitzung* ausgelöst hat,
/// nicht einen einzelnen Ledger-Eintrag): ein `Confirm`-Klick im
/// Bestätigungsdialog ist weder "die KI" noch "ein MCP-Agent" — braucht
/// also den eigenen dritten Wert `User`, den keiner der beiden anderen
/// Typen kennt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LedgerSource {
    User,
    Ai,
    McpAgent,
}

/// Ausgang einer Freigabe-Entscheidung (Spec 0057, §1.1: "bestätigt /
/// abgelehnt / per Regel automatisch").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LedgerDecisionOutcome {
    /// Der Nutzer hat im Bestätigungsdialog zugestimmt (inkl. `Edit-then-
    /// approve`, s. `app-shell::dto::ActionUserDecision`).
    Confirmed,
    /// Der Nutzer hat abgelehnt, ODER die Filter-Engine hat automatisch
    /// blockiert (`Decision::Deny`) — beides ein "Nein", unterschieden
    /// durch `LedgerEntryContent::Decision::reason`/`code` (bei einer
    /// automatischen Blockade gefüllt) und `source` (bei einer echten
    /// Nutzer-Ablehnung `LedgerSource::User`).
    Rejected,
    /// `Decision::AutoExec` — durch eine Allow-Regel automatisch
    /// freigegeben, ohne Bestätigungsdialog.
    AutoApproved,
}

/// Inhalt eines einzelnen Ledger-Eintrags (Spec 0057, §1.1) — genau die
/// vier für diese Etappe verlangten Ereignistypen. Bewusst als ein
/// getaggtes Enum statt vier getrennter Tabellen/Typen: dieselbe, bereits
/// etablierte Konvention wie `ai::MessageContent`
/// (`persistence-sqlite::chat_session_store`s `content_type`-Spalte +
/// JSON-`content`).
///
/// **Redaction ist NICHT Aufgabe dieses Typs** — er transportiert nur die
/// Daten, redigiert werden sie zentral in `app-shell::orchestration::
/// emit_activity`, unmittelbar bevor ein Eintrag den Store erreicht
/// (Spec 0057, §1.2 "PFLICHT"). Wer diesen Typ direkt konstruiert (z. B. in
/// Tests), muss also selbst redigieren, falls das Ergebnis in einen
/// echten Store geschrieben werden soll.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum LedgerEntryContent {
    /// Ein Kommando wurde vorgeschlagen (Spec 0057, §1.1, erster Punkt) —
    /// "von wem" ist der begleitende [`LedgerSource`] des Eintrags, nicht
    /// Teil dieser Variante.
    CommandProposed { command: String },
    /// Freigabe-Entscheidung für ein zuvor vorgeschlagenes Kommando (Spec
    /// 0057, §1.1, zweiter Punkt). `matched_rule`/`matched_rule_origin`
    /// sind `Some`, wenn eine Nutzerregel ausschlaggebend war (aus
    /// `FilterEngine::evaluate_explained`s `EvaluationTrace`) — `None` bei
    /// einer Eskalation ohne Regelbezug (z. B. MCP-Herkunft, Sudo-Passwort,
    /// s. `app-shell::orchestration::handle_action_proposed`) oder wenn
    /// die Hard-Blacklist/der Default (kein `AutoExec` ohne Regel) den
    /// Ausschlag gab; `reason`/`code` decken diese Fälle stattdessen ab.
    Decision {
        outcome: LedgerDecisionOutcome,
        reason: Option<String>,
        code: Option<String>,
        matched_rule: Option<RuleId>,
        matched_rule_origin: Option<RuleOrigin>,
    },
    /// Ein Kommando wurde ausgeführt, mit Ergebnis (Spec 0057, §1.1,
    /// dritter Punkt — "stdout/stderr/exit — redigiert"). `output` MUSS
    /// bereits redigiert sein, bevor dieser Wert konstruiert wird (bzw.
    /// wird von `emit_activity` redigiert, s. Typ-Doc-Kommentar
    /// oben).
    CommandExecuted {
        command: String,
        output: CommandOutput,
        cancelled: bool,
    },
    /// Eine Nachricht der KI (Spec 0057, §1.1, vierter Punkt) — sowohl
    /// normaler Chat-Text als auch ein per `GenerateDocument` erzeugtes
    /// Dokument (Spec 0012, Abschnitt 5: "wird als Teil der
    /// Assistant-Nachricht ... übernommen, wie ein normaler Chat-Text" —
    /// dieselbe Gleichsetzung gilt hier für den Ledger-Eintrag).
    AiMessage { text: String },
    /// An AI file read (Spec 0057). The file content is never stored here,
    /// only the path, the size and a SHA-256 (lowercase hex) of the bytes read.
    FileRead {
        path: String,
        bytes: Option<u64>,
        sha256: Option<String>,
    },
    /// An AI file write (Spec 0057): written only when bytes were actually
    /// written. Neither content nor diff is stored; `None` in a before/after
    /// field means "not known" (e.g. a new file has no "before"; the old
    /// content of a file only readable with elevated rights is not read).
    FileWritten {
        path: String,
        /// Server-side backup of the previous content, if one was made.
        backup_path: Option<String>,
        bytes_before: Option<u64>,
        bytes_after: Option<u64>,
        sha256_before: Option<String>,
        sha256_after: Option<String>,
        /// Whether the sudo fallback was used for this write.
        sudo: bool,
    },
    /// A connection event of the recorded session (issue #1447).
    Connection {
        event: LedgerConnectionEvent,
        detail: Option<String>,
    },
    /// Proof of life of a running session. Carries no content at all.
    ActivityTick,
}

/// Kind of a [`LedgerEntryContent::Connection`] entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LedgerConnectionEvent {
    Connected,
    Disconnected,
    Reconnected,
    Failed,
}
