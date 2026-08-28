use serde::{Deserialize, Serialize};

/// Platzhalter-Identifier für einen Server.
///
/// Das `ssh`-Modul (siehe `crates/core/src/ssh/mod.rs`) ist aktuell nur ein
/// leeres Skelett und stellt noch keinen kanonischen Server-Typ bereit. Sobald
/// das der Fall ist, sollte dieser Typ durch einen Re-Export von dort ersetzt
/// werden, statt ihn hier zu duplizieren.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ServerId(pub String);

/// Ergebnis einer Filter-Auswertung (Spec 0002, Abschnitt 2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Decision {
    AutoExec,
    Confirm { reason: String },
    Deny { reason: String },
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

/// Nutzerdefinierte Regel (Spec 0002, Abschnitt 2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rule {
    pub id: String,
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
            server_id: ctx.server_id.clone(),
            tags: ctx.tags.clone(),
        }
    }
}
