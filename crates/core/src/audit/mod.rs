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
}
