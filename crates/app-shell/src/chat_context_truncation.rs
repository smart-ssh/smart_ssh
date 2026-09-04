//! Kontext-Kürzung beim Laden einer Sitzung (Spec 0034, Abschnitt 9) — löst
//! den in Spec 0006, Abschnitt 8 offen gelassenen Punkt zur Kontext-Kürzung.
//!
//! Bewusst eine einfache, zeichenbasierte Näherung statt eines exakten,
//! providerspezifischen Tokenizers (Spec-Text: "einfache, zeichenbasierte
//! Näherung"). Kürzt nur die an [`ssh_manager_core::ai::AiProvider::send`]
//! übergebene *Kopie* der Historie — die gespeicherte Historie in
//! `Session::context`/der DB bleibt in jedem Fall vollständig erhalten,
//! s. Aufrufer in `crate::orchestration`/`crate::commands`.

use ssh_manager_core::ai::{ChatMessage, MessageContent, RejectionReason};

/// Spec 0034, Abschnitt 9: "konfigurierbares Zeichen-Budget (Default z. B.
/// 40.000 Zeichen)". Nicht Teil dieses Schritts, tatsächlich zur Laufzeit
/// konfigurierbar zu machen (keine entsprechende Einstellungs-UI vorgesehen
/// oder von der Spec verlangt) — dieselbe Konstante-statt-Einstellung-
/// Entscheidung wie z. B. `orchestration::MAX_LOGGED_OUTPUT_LEN`.
pub const DEFAULT_CHAR_BUDGET: usize = 40_000;

/// Grobe Zeichen-Näherung des tatsächlich an den Provider gesendeten Texts
/// einer einzelnen Nachricht — reicht für die in Abschnitt 9 verlangte
/// "einfache Näherung", ohne das exakte Fencing/Formatierung, das
/// `ai-providers` erst beim tatsächlichen Versand anwendet, hier
/// nachzubilden.
fn approximate_char_len(message: &ChatMessage) -> usize {
    match &message.content {
        MessageContent::Text(text) => text.chars().count(),
        MessageContent::CommandResult {
            command, output, ..
        } => {
            command.chars().count()
                + String::from_utf8_lossy(&output.stdout).chars().count()
                + String::from_utf8_lossy(&output.stderr).chars().count()
        }
        MessageContent::ActionRejected { command, reason } => {
            command.chars().count()
                + match reason {
                    RejectionReason::User => 0,
                    RejectionReason::Blocked(reason) => reason.chars().count(),
                }
        }
    }
}

/// Entfernt die **ältesten** Nachrichten zuerst, bis die verbleibende
/// Historie unter [`DEFAULT_CHAR_BUDGET`] passt (Spec 0034, Abschnitt 9:
/// "reines Kürzen, einfach und vorhersehbar" — kein Zusammenfassen/
/// Komprimieren).
pub fn truncate_to_budget(history: Vec<ChatMessage>) -> Vec<ChatMessage> {
    truncate_to_budget_with(history, DEFAULT_CHAR_BUDGET)
}

/// Testbare Variante mit explizitem Budget (s. [`truncate_to_budget`]).
pub(crate) fn truncate_to_budget_with(
    mut history: Vec<ChatMessage>,
    budget: usize,
) -> Vec<ChatMessage> {
    let mut total: usize = history.iter().map(approximate_char_len).sum();
    // `history.len() > 1` statt `!history.is_empty()`: die jeweils letzte
    // (jüngste) Nachricht bleibt immer erhalten, selbst wenn sie allein
    // schon das Budget sprengt (z. B. ein sehr langer Kommando-Output) —
    // sonst würde ein Kürzen bis auf null Nachrichten den gerade aktuellen
    // Gesprächsbeitrag mit wegwerfen, nicht nur älteren Kontext.
    while total > budget && history.len() > 1 {
        let removed = history.remove(0);
        total = total.saturating_sub(approximate_char_len(&removed));
    }
    history
}

#[cfg(test)]
mod tests {
    use super::*;
    use ssh_manager_core::ai::Role;

    fn text_message(text: &str) -> ChatMessage {
        ChatMessage {
            role: Role::User,
            content: MessageContent::Text(text.to_string()),
        }
    }

    #[test]
    fn test_history_under_budget_is_unchanged() {
        let history = vec![text_message("kurz"), text_message("auch kurz")];
        let result = truncate_to_budget_with(history.clone(), 1000);
        assert_eq!(result, history);
    }

    /// Spec 0034, Abschnitt 9: "die ältesten Nachrichten zuerst verworfen".
    #[test]
    fn test_oldest_messages_are_dropped_first() {
        let history = vec![
            text_message(&"a".repeat(50)),
            text_message(&"b".repeat(50)),
            text_message(&"c".repeat(50)),
        ];
        // Budget passt genau für die letzten zwei Nachrichten (100 Zeichen).
        let result = truncate_to_budget_with(history, 100);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].content, MessageContent::Text("b".repeat(50)));
        assert_eq!(result[1].content, MessageContent::Text("c".repeat(50)));
    }

    #[test]
    fn test_truncation_never_leaves_history_over_budget_when_possible() {
        let history = vec![
            text_message(&"a".repeat(30)),
            text_message(&"b".repeat(30)),
            text_message(&"c".repeat(30)),
        ];
        let result = truncate_to_budget_with(history, 45);
        let total: usize = result.iter().map(approximate_char_len).sum();
        assert!(
            total <= 45,
            "verbleibende Historie muss unters Budget passen: {total}"
        );
        assert_eq!(result.len(), 1);
    }

    /// Ein einzelner, für sich genommen bereits das Budget sprengender
    /// Eintrag darf nicht zu einer leeren Historie führen, wenn er der
    /// einzige verbleibende ist — die Schleife bricht ab, sobald die
    /// Historie leer ist, statt in einer Endlosschleife/Panik zu enden.
    #[test]
    fn test_single_oversized_message_is_kept_not_dropped_to_empty() {
        let history = vec![text_message(&"x".repeat(1000))];
        let result = truncate_to_budget_with(history.clone(), 10);
        assert_eq!(result, history);
    }

    #[test]
    fn test_empty_history_stays_empty() {
        assert_eq!(truncate_to_budget_with(Vec::new(), 100), Vec::new());
    }
}
