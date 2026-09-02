use serde::{Deserialize, Serialize};

// `ServerId` lebt seit Spec 0003 in `crate::shared` (gemeinsam mit
// `profiles`), s. dortiger Modul-Kommentar für die Begründung. Der frühere
// String-basierte Platzhalter hier wurde entfernt.
use crate::shared::ServerId;

/// Ergebnis einer Filter-Auswertung (Spec 0002, Abschnitt 2).
///
/// `code` (Spec 0024, Abschnitt 5): stabiler, sprachunabhängiger Bezeichner
/// der Grund-Art, fürs Frontend-Mapping auf Übersetzungs-Keys — `reason`
/// bleibt der bestehende (deutsche) Anzeigetext, unverändert als Fallback,
/// falls das Frontend einen `code` nicht kennt. Bei mehreren gleichzeitig
/// zutreffenden Gründen (z. B. Hard-Blacklist UND eine passende Regel, s.
/// `engine::combine`) trägt `reason` weiterhin alle zusammengeführten Texte,
/// `code` dagegen nur den nach Präzedenz wichtigsten einzelnen Grund (s.
/// `engine::merge_codes`) — für die Anzeige repräsentativ genug, ohne den
/// Typ auf eine Liste umstellen zu müssen. `String` statt `&'static str`:
/// `Decision` leitet `Deserialize` ab (Rundtrip über Tauri-Events/Tests),
/// ein geliehenes `&'static str`-Feld ist damit nicht kombinierbar (serde
/// kann `'static` nicht aus der Deserializer-Lifetime `'de` herleiten) —
/// die Werte selbst bleiben trotzdem aus einer festen Konstanten-Menge
/// (s. `engine`-Modul), nur die Repräsentation ist `String`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Decision {
    AutoExec,
    Confirm { reason: String, code: String },
    Deny { reason: String, code: String },
}

/// Wirkung einer Nutzerregel (Spec 0002, Abschnitt 2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum RuleAction {
    /// macht Kommando AutoExec-fähig
    Allow,
    /// erzwingt Bestätigung, auch wenn andere Regel Allow sagt
    Confirm,
    /// blockt komplett, keine Ausführung möglich
    Deny,
}

/// Eindeutige Kennung einer Nutzerregel (Spec 0009, Abschnitt 3 referenziert
/// `RuleId` in den Tauri-Command-Signaturen, ohne den Typ selbst zu
/// definieren).
///
/// Bewusst ein `String`-Newtype, nicht Uuid-basiert wie `ServerId`/
/// `GroupId`/`ProviderId`: Spec 0002 definiert `Rule.id` von Anfang an als
/// freien `String` (u. a. für sprechende IDs in Tests/Beispielen wie
/// `"allow-ls"`), und Spec 0009s SQL-Schema legt `id TEXT PRIMARY KEY` fest,
/// ohne UUID-Formatierung zu verlangen — `persistence-sqlite` befüllt ihn in
/// der Praxis mit einer frisch generierten UUID (`Uuid::new_v4().to_string()`,
/// analog zu den anderen IDs), aber der Typ selbst bleibt so flexibel wie
/// von Spec 0002 vorgesehen.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RuleId(pub String);

impl std::fmt::Display for RuleId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Nutzerdefinierte Regel (Spec 0002, Abschnitt 2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rule {
    pub id: RuleId,
    pub pattern: Pattern,
    pub action: RuleAction,
    pub scope: Scope,
    /// höher = wird innerhalb seines Scope-/Action-Buckets zuerst geprüft
    pub priority: i32,
}

/// Muster, gegen das ein (bereits in Teilkommandos zerlegtes) Kommando
/// geprüft wird (Spec 0002, Abschnitt 2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Pattern {
    /// z. B. "ls *", "cat /var/log/*"
    Glob(String),
    /// für komplexere Fälle
    Regex(String),
    Exact(String),
}

/// Geltungsbereich einer Regel (Spec 0002, Abschnitt 2). Spezifität für die
/// Präzedenz-Sortierung (Abschnitt 3): `Server` > `Tag` > `Global`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Scope {
    Global,
    Server(ServerId),
    /// z. B. "production", "dev"
    Tag(String),
}

/// Eingabe-Kontext einer Auswertung (Spec 0002, Abschnitt 5).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvalContext {
    pub server_id: ServerId,
    pub tags: Vec<String>,
}

/// Von [`EvalContext`] abgeleiteter Scope, den ein `PolicyStore` verwendet,
/// um die für eine Auswertung relevanten Regeln zu bestimmen.
///
/// Die Spec referenziert `EffectiveScope` in der `PolicyStore`-Signatur
/// (Abschnitt 5), definiert den Typ dort aber nicht konkret. Hier als simple
/// Bündelung aus Server-ID und Tags umgesetzt, analog zu `EvalContext` — ein
/// `PolicyStore` kann so unabhängig vom vollen Auswertungs-Kontext
/// implementiert werden.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EffectiveScope {
    pub server_id: ServerId,
    pub tags: Vec<String>,
}

impl From<&EvalContext> for EffectiveScope {
    fn from(ctx: &EvalContext) -> Self {
        Self {
            server_id: ctx.server_id,
            tags: ctx.tags.clone(),
        }
    }
}

/// Nachvollziehbare Spur einer Auswertung (Spec 0009, Abschnitt 4) — löst
/// den in Spec 0002 Abschnitt 7 offen gelassenen "Simulationsansicht"-Punkt.
/// Ausschließlich für die Testen-Funktion im UI gedacht, nicht für die
/// eigentliche KI-Kommandoschleife (dort reicht `Decision` aus `evaluate()`).
///
/// `matched_rule`/`matched_hard_blacklist_entry` sind bewusst unabhängig
/// voneinander gesetzt (beide können gleichzeitig `Some` sein, wenn z. B.
/// sowohl eine passende Nutzerregel als auch die Hard-Blacklist gegriffen
/// haben) — das spiegelt ehrlich wider, dass `combine()` mehrere
/// gleichzeitig zutreffende Faktoren zur strengsten Gesamt-Decision
/// zusammenführt, statt nur einen einzelnen "Gewinner" zu benennen.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvaluationTrace {
    pub decision: Decision,
    /// `None`, falls keine Nutzerregel gegriffen hat (Hard-Blacklist/Default
    /// war ausschlaggebend) — bei mehreren Teilkommandos (Chaining) auf der
    /// obersten Ebene ebenfalls `None`, da dort kein einzelnes `matched_rule`
    /// mehr sinnvoll ist (s. `sub_command_traces`).
    pub matched_rule: Option<RuleId>,
    /// Anzeigetext des gegriffenen Hard-Blacklist-Musters, falls eines
    /// gegriffen hat (s. `crate::filter::hard_blacklist_patterns`).
    pub matched_hard_blacklist_entry: Option<String>,
    /// Ein Trace pro Teilkommando bei Chaining (Spec 0002, Abschnitt 4) UND
    /// pro `$(...)`/Backtick-Command-Substitution — beide Fälle nutzen
    /// dasselbe Feld, da für die Erklärbarkeit derselbe Gedanke gilt: "das
    /// hier ist ein eigenständig ausgewerteter Teil, der zur
    /// Gesamt-Entscheidung beigetragen hat".
    pub sub_command_traces: Vec<EvaluationTrace>,
}
