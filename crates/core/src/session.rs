//! Session-Modell — Zielbild für eine künftige, vereinheitlichte
//! Sitzungs-Persistenz (Spec 0037, Abschnitt 7). **Reine Typdefinitionen**,
//! bewusst ohne jede Implementierung: die bestehende Chat-Session-
//! Persistenz (Spec 0034, ggf. Spec 0036) wird im Rahmen dieser Spec
//! **nicht** zwangsweise auf dieses Modell migriert — das ist Gegenstand
//! eines separaten Sitzungs-Abgleichs (Spec 0037, Abschnitt 8) und einer
//! möglichen eigenen Folge-Spec. Diese Typen bleiben deshalb bewusst
//! (noch) nirgends im Code verwendet — das ist beabsichtigt, kein
//! übersehener Anschluss.
//!
//! Ergänzt um [`SyncBackend`] (Spec 0037, Abschnitt 6) — ebenfalls reines
//! Vokabular, keine Git-/Cloud-Sync-Implementierung in dieser Spec.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::ai::Role;
use crate::filter::{Decision, RuleId, RuleOrigin};
use crate::shared::ServerId;

/// Eindeutige Kennung einer [`Session`] — eigener Typ statt Wiederverwendung
/// einer der bestehenden `app-tauri`-internen `SessionId`-Alias (Spec 0007),
/// da `core` keine Abhängigkeit auf `app-tauri` haben darf (umgekehrt schon,
/// s. Moduldoc `lib.rs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(pub Uuid);

impl SessionId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for SessionId {
    fn default() -> Self {
        Self::new()
    }
}

/// Wer eine Sitzung ausgelöst hat — Spec 0037, Abschnitt 7 nennt
/// `Human | McpAgent { agent_id }` (dasselbe Unterscheidungsprinzip wie
/// das bestehende `app-tauri::dto::ActionOrigin`, hier aber als Teil des
/// Ziel-Session-Modells in `core`, nicht als Tauri-DTO).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionOrigin {
    Human,
    McpAgent { agent_id: String },
}

/// Eine Nachricht im kanonischen, providerneutralen Format (Spec 0037,
/// Abschnitt 7: "kanonisches Format, redigiert") — bewusst schlanker als
/// das bestehende `ai::ChatMessage`/`MessageContent` (Spec 0006/0034), das
/// noch an den aktuellen Chat-Turn-Ablauf gebunden ist (`CommandResult`/
/// `ActionRejected`-Varianten). Ob/wie beide Modelle zusammengeführt
/// werden, ist Teil des in Abschnitt 8 beschriebenen Sitzungs-Abgleichs,
/// nicht dieser Spec.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    /// Bereits redigierter Inhalt (Spec 0037, Abschnitt 7: "redigiert") —
    /// derselbe Grundsatz wie beim bestehenden `OutputRedactor` (Spec
    /// 0006 Abschnitt 5): Redaction ist hier bereits geschehen, nicht
    /// Aufgabe eines Konsumenten dieses Typs.
    pub content: String,
    pub at: DateTime<Utc>,
}

/// Digest (nicht der volle Inhalt) einer Kommando-Ausgabe für den
/// [`LedgerEntry`] — Spec 0037, Abschnitt 7 nennt `output_digest`, ohne den
/// genauen Aufbau festzulegen. Als reiner Hex-Digest-String gehalten
/// (Hash-Algorithmus wird bei tatsächlicher Erzeugung mitgeführt, hier noch
/// nicht festgelegt) statt der vollen Ausgabe: das Ledger ist laut
/// Architektur-Brief ein wörtliches, nie kompaktiertes Audit-Protokoll,
/// soll aber nicht zwangsläufig komplette (potenziell große, potenziell
/// sensible) Kommando-Ausgaben dauerhaft verdoppeln.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutputDigest(pub String);

/// Verweis auf die Regel, die eine [`LedgerEntry`]s `decision` bestimmt hat
/// (falls eine gegriffen hat) — kombiniert `RuleId` und `RuleOrigin` (Spec
/// 0037, Abschnitt 5), damit ein Ledger-Eintrag auch nach einer Auswertung
/// noch erkennen lässt, ob eine `User`- oder eine `Organization`-Regel
/// ausschlaggebend war.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuleRef {
    pub id: RuleId,
    pub origin: RuleOrigin,
}

/// Ein einzelner, wörtlicher Eintrag im Session-Ledger (Spec 0037,
/// Abschnitt 7) — im Unterschied zu `messages`/`summary` **nie**
/// kompaktiert oder zusammengefasst: das vollständige Audit-Protokoll
/// jeder ausgewerteten Aktion einer Sitzung.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LedgerEntry {
    pub at: DateTime<Utc>,
    pub command: String,
    pub decision: Decision,
    pub rule: Option<RuleRef>,
    pub exit_code: Option<i32>,
    pub output_digest: OutputDigest,
}

/// Zusammenfassung einer (teilweise) kompaktierten Sitzung (Spec 0037,
/// Abschnitt 7: `pub summary: Option<Summary>`) — Aufbau von der Spec nicht
/// weiter festgelegt; minimal gehalten (Text plus Zeitpunkt der
/// Erzeugung), da eine konkrete Kompaktierungs-Strategie nicht Teil dieser
/// Spec ist.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Summary {
    pub text: String,
    pub generated_at: DateTime<Utc>,
}

/// Kompaktierungs-Zustand einer Sitzung (Spec 0037, Abschnitt 7: `pub
/// compaction: CompactionState`) — ob/wann `messages` bereits durch
/// [`Summary`] verdichtet wurden. Kein `messages`-Löschen/-Kürzen in
/// dieser Spec, nur das Vokabular für den Zustand selbst.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CompactionState {
    NotCompacted,
    Compacted { at: DateTime<Utc> },
}

/// Zielbild einer Sitzung (Spec 0037, Abschnitt 7) — vereint, was die
/// bestehende Chat-Session-Persistenz (Spec 0034) aktuell auf mehrere
/// Konzepte verteilt (Nachrichtenverlauf, Kommando-Ergebnisse, Redaction),
/// um zusätzlich ein wörtliches, nie kompaktiertes Audit-Ledger
/// (`ledger`) und eine optionale Kompaktierungs-Zusammenfassung
/// (`summary`/`compaction`) vorzusehen. **Keine erzwungene Migration** der
/// bestehenden Implementierung auf dieses Modell im Rahmen dieser Spec —
/// s. Moduldoc.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Session {
    pub id: SessionId,
    pub server_id: ServerId,
    pub origin: SessionOrigin,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    /// Kanonisches Format, redigiert (Spec 0037, Abschnitt 7).
    pub messages: Vec<Message>,
    /// Wörtlich, nie kompaktiert (Spec 0037, Abschnitt 7).
    pub ledger: Vec<LedgerEntry>,
    pub summary: Option<Summary>,
    pub compaction: CompactionState,
}

/// Ein Ende-zu-Ende-verschlüsseltes Datenpaket für [`SyncBackend`] (Spec
/// 0037, Abschnitt 6) — Aufbau von der Spec nicht weiter festgelegt;
/// bewusst nur Ciphertext + Nonce (wie
/// [`crate::crypto::EncryptedContent`], hier aber als eigener,
/// bündel-weiter Typ statt einer einzelnen Nachricht, da ein Sync-Bündel
/// mehrere Sitzungen/Notizen auf einmal transportieren können soll).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EncryptedBundle {
    pub ciphertext: Vec<u8>,
    pub nonce: Vec<u8>,
}

/// Bestätigung eines erfolgreichen [`SyncBackend::push`] (Spec 0037,
/// Abschnitt 6) — Aufbau von der Spec nicht weiter festgelegt; minimal
/// gehalten (Zeitpunkt der Bestätigung).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncReceipt {
    pub synced_at: DateTime<Utc>,
}

/// Fehler aus [`SyncBackend`]. Analog zu `filter::PolicySourceError`/
/// `profiles::ProfileError::Backend`: `core` selbst darf keine
/// Netzwerk-/Backend-Abhängigkeit bekommen, daher nur eine
/// `String`-Nachricht statt eines strukturierten Zugriffs auf den
/// jeweiligen Original-Fehlertyp.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize, Deserialize)]
#[error("sync backend error: {0}")]
pub struct SyncError(pub String);

pub type SyncResult<T> = Result<T, SyncError>;

/// Vokabular für ein Ende-zu-Ende-verschlüsseltes Sync-Backend (Spec 0037,
/// Abschnitt 6) — **keine** Implementierung in dieser Spec (kein
/// Git-/Cloud-Sync). Eine konkrete Implementierung (privates Repo) würde
/// `push`/`pull` gegen ein tatsächliches Backend (z. B. ein Git-Repository
/// oder einen Cloud-Speicher) umsetzen.
#[async_trait]
pub trait SyncBackend: Send + Sync {
    async fn push(&self, bundle: EncryptedBundle) -> SyncResult<SyncReceipt>;
    async fn pull(&self) -> SyncResult<Option<EncryptedBundle>>;
}
