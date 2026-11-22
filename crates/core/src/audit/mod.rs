//! Das Session-Ledger (Spec 0057, §1) — ein append-only, dauerhaftes
//! Protokoll aller sicherheitsrelevanten Vorgänge einer Sitzung: was
//! vorgeschlagen, wie entschieden und was tatsächlich ausgeführt wurde,
//! plus die KI-Nachrichten selbst. Die dauerhafte Wahrheit, aus der später
//! Audit-Log und Report schöpfen (Spec 0057, Einleitung).
//!
//! **Reine Typdefinitionen** — kein I/O, keine Persistenz-Logik hier (s.
//! `crate`-Moduldoc zur `core`-Grenze). Die konkrete Speicherung lebt in
//! `persistence-sqlite::SqliteLedgerStore` (additive Migration, Spec 0057
//! §5), das Einhängen ins Geschehen in `app-shell::orchestration` (an
//! denselben Stellen, an denen die Redaction für den KI-Kontext bereits
//! läuft, s. dortige Doc-Kommentare zu `write_ledger_entry`).
//!
//! **Abgrenzung zu `crate::session::LedgerEntry`** (Spec 0037, Abschnitt
//! 7): jenes Modul ist explizit unbenutztes Zielbild-Vokabular für eine
//! künftige, vereinheitlichte Sitzungs-Persistenz und hält bewusst nur
//! einen `OutputDigest` (Hash), nicht die volle Ausgabe — "das Ledger ...
//! soll aber nicht zwangsläufig komplette ... Kommando-Ausgaben dauerhaft
//! verdoppeln" (dortiger Kommentar). Spec 0057 §1.1 verlangt explizit das
//! Gegenteil: "**Alles** rein (Kommandos, Ergebnisse, ...)" — die volle,
//! redigierte `stdout`/`stderr`, nicht nur ein Digest, weil das Ledger
//! später die Grundlage für Report/Audit-Log liefern soll, die einen
//! bloßen Hash nicht sinnvoll anzeigen könnten. Diese beiden Typen sind
//! deshalb bewusst getrennt gehalten (unterschiedliche Module,
//! unterschiedlicher Name `LedgerEntryContent` statt `LedgerEntry`) statt
//! den Namenskonflikt durch Wiederverwendung/Anpassung des unbenutzten
//! Spec-0037-Typs aufzulösen — Letzterer bleibt unverändert, `core::session`
//! ist laut seinem eigenen Moduldoc ohnehin noch an keiner Stelle verankert.

use serde::{Deserialize, Serialize};

use crate::filter::{Pattern, RuleAction, RuleId, RuleOrigin, Scope};
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
/// JSON-`content`), hier von `persistence-sqlite::ledger_store`
/// gespiegelt (`entry_type`-Spalte + verschlüsselter `content`-Blob).
///
/// **Redaction ist NICHT Aufgabe dieses Typs** — er transportiert nur die
/// Daten, redigiert werden sie zentral in `app-shell::orchestration::
/// write_ledger_entry`, unmittelbar bevor ein Eintrag den Store erreicht
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
    /// wird von `write_ledger_entry` redigiert, s. Typ-Doc-Kommentar
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
    /// An AI file write (Spec 0057): written when the write succeeded, or with
    /// status `Failed` when it failed after it had started. Neither content nor diff is stored; `None` in a before/after
    /// field means "not known" (e.g. a new file has no "before"; the old
    /// content of a file only readable with elevated rights is not read).
    FileWritten {
        path: String,
        /// `Failed`: the write started and then failed, so the target may be
        /// partly overwritten; the size/hash fields of such an entry are `None`
        /// (nothing is known to have been written).
        #[serde(default)]
        status: LedgerFileWriteStatus,
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

/// Outcome of a [`LedgerEntryContent::FileWritten`] entry.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LedgerFileWriteStatus {
    /// The whole write succeeded.
    #[default]
    Completed,
    /// The write reached the target and then failed; it may be partly changed.
    Failed,
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

impl LedgerEntryContent {
    /// Value of the `entry_type` column for this content.
    pub fn entry_type(&self) -> &'static str {
        match self {
            LedgerEntryContent::CommandProposed { .. } => "command_proposed",
            LedgerEntryContent::Decision { .. } => "decision",
            LedgerEntryContent::CommandExecuted { .. } => "command_executed",
            LedgerEntryContent::AiMessage { .. } => "ai_message",
            LedgerEntryContent::FileRead { .. } => "file_read",
            LedgerEntryContent::FileWritten { .. } => "file_written",
            LedgerEntryContent::Connection { .. } => "connection",
            LedgerEntryContent::ActivityTick => "activity_tick",
        }
    }
}

/// The rule that decided, as it was at decision time (stored with `decision`
/// entries in the `rule_snapshot` column).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LedgerRuleSnapshot {
    pub pattern: Pattern,
    pub action: RuleAction,
    pub scope: Scope,
}

/// Fields every ledger entry carries besides its content. All defaults apply
/// to rows written before the activity record existed.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LedgerEntryMeta {
    /// Links proposal, decision and execution of one action.
    #[serde(default)]
    pub action_id: Option<String>,
    /// Set when redaction could not be completed for this entry.
    #[serde(default)]
    pub redaction_incomplete: bool,
    /// Only for `decision` entries.
    #[serde(default)]
    pub rule_snapshot: Option<LedgerRuleSnapshot>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `file_written` row stored before the `status` field existed (#1483)
    /// still reads back as `Completed`, other fields unchanged.
    #[test]
    fn test_legacy_file_written_without_status_deserializes_as_completed() {
        let json = r#"{"FileWritten":{"path":"/etc/a","backup_path":"/etc/a.bak","bytes_before":3,"bytes_after":5,"sha256_before":"aa","sha256_after":"bb","sudo":true}}"#;
        let content: LedgerEntryContent = serde_json::from_str(json).unwrap();
        let LedgerEntryContent::FileWritten {
            path,
            status,
            backup_path,
            bytes_before,
            bytes_after,
            sha256_before,
            sha256_after,
            sudo,
        } = content
        else {
            panic!("expected file_written");
        };
        assert_eq!(status, LedgerFileWriteStatus::Completed);
        assert_eq!(path, "/etc/a");
        assert_eq!(backup_path.as_deref(), Some("/etc/a.bak"));
        assert_eq!((bytes_before, bytes_after), (Some(3), Some(5)));
        assert_eq!(sha256_before.as_deref(), Some("aa"));
        assert_eq!(sha256_after.as_deref(), Some("bb"));
        assert!(sudo);
    }
}
