use serde::{Deserialize, Serialize};

/// Warum eine Aktion nicht ausgeführt wurde (Spec 0021, Abschnitt 3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum RejectionReason {
    /// Der Nutzer hat im Bestätigungsdialog auf "Ablehnen" geklickt.
    User,
    /// Die Filter-Engine hat die Aktion automatisch blockiert, ohne dass
    /// überhaupt ein Bestätigungsdialog gezeigt wurde — der `String` ist
    /// der `Decision::Deny { reason }`-Grund (Spec 0002).
    Blocked(String),
    /// Spec 0046, Fund 4: eine wartende Bestätigung wurde durch das
    /// Backend-seitige Sicherheitsnetz aufgelöst, OHNE dass je eine
    /// Nutzerentscheidung eintraf (z. B. ein Frontend-Reload, der den
    /// wartenden Dialog aus seinem State verlor). Bewusst nicht `User` —
    /// die KI (und ein Mensch, der die Historie später liest) soll nicht
    /// fälschlich annehmen, ein Mensch habe sich aktiv gegen den
    /// Vorschlag entschieden.
    Timeout,
    /// Issue #1019: the connection was re-established while the
    /// confirmation was open and the pending action was discarded. The
    /// user did not decide.
    Reconnect,
}

impl RejectionReason {
    /// Short stable label for log lines (never the free-text reason).
    pub fn kind(&self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Blocked(_) => "blocked",
            Self::Timeout => "timeout",
            Self::Reconnect => "reconnect",
        }
    }
}
