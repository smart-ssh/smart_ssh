//! Fallback-Modus für Provider/Modelle ohne natives Tool-Calling (Spec
//! 0006, Abschnitt 4, `supports_native_tool_calling: false`) — geteilt
//! zwischen `OpenAiCompatibleProvider` und `AnthropicProvider`, da das
//! Markerformat providerunabhängig ist.
//!
//! Aktionsvorschläge werden als JSON-Block zwischen `<!--ACTION-->` und
//! `<!--/ACTION-->` erwartet (Markerwahl gemäß Aufgabenstellung Teil 2,
//! Punkt 2). Der Block wird **erst nach Abschluss des Streams** geparst
//! (Spec 0006, Abschnitt 4) — deshalb werden Text-Deltas im Fallback-Modus
//! serverseitig gepuffert, statt roh weiterzustreamen: andernfalls würde
//! der Marker-Block selbst kurzzeitig als sichtbarer Chat-Text
//! durchrutschen, bevor er erkannt und herausgeschnitten werden kann. Das
//! kostet in diesem Modus die inkrementelle Streaming-UX, ist aber die
//! einzige Möglichkeit, den Marker-Block zuverlässig vor der Anzeige zu
//! entfernen.

use serde_json::Value;
use ssh_manager_core::ai::ActionSchema;
use ssh_manager_core::profiles::AiAction;

const ACTION_START: &str = "<!--ACTION-->";
const ACTION_END: &str = "<!--/ACTION-->";

/// Ergänzung für den System-Prompt im Fallback-Modus: beschreibt das
/// erwartete Marker-/JSON-Format sowie die verfügbaren Aktionen.
pub(crate) fn fallback_system_prompt_addition(actions: &[ActionSchema]) -> String {
    let mut addition = String::from(
        "\n\nWenn du eine der folgenden Aktionen vorschlagen möchtest, gib \
         genau einen JSON-Block zwischen den Markern ",
    );
    addition.push_str(ACTION_START);
    addition.push_str(" und ");
    addition.push_str(ACTION_END);
    addition.push_str(
        " aus, im Format {\"action\": \"<name>\", \"parameters\": {...}}. \
         Der Block darf an beliebiger Stelle in deiner Antwort stehen. \
         Gib niemals mehr als einen solchen Block aus. Verfügbare Aktionen:\n",
    );
    for action in actions {
        addition.push_str(&format!("- {}: {}\n", action.name, action.description));
    }
    addition
}

/// Ergebnis des Parsens einer vollständigen Fallback-Antwort.
pub(crate) struct FallbackParseResult {
    /// Sichtbarer Text — bei erfolgreichem Parsen ohne den Marker-Block,
    /// sonst unverändert der komplette Originaltext.
    pub text: String,
    pub action: Option<AiAction>,
}

/// Sucht den `<!--ACTION-->...<!--/ACTION-->`-Block in `full_text` und
/// versucht ihn zu parsen. Bei jedem Fehler (kein Block gefunden,
/// ungültiges JSON, unbekannte Aktion, fehlende Felder) wird `full_text`
/// unverändert als reiner Text zurückgegeben — nie ein `AiError` (Spec
/// 0006, Abschnitt 4).
pub(crate) fn parse_fallback_response(full_text: &str) -> FallbackParseResult {
    try_parse_action_block(full_text).unwrap_or(FallbackParseResult {
        text: full_text.to_string(),
        action: None,
    })
}

fn try_parse_action_block(full_text: &str) -> Option<FallbackParseResult> {
    let start = full_text.find(ACTION_START)?;
    let search_from = start + ACTION_START.len();
    let end_rel = full_text[search_from..].find(ACTION_END)?;
    let json_part = &full_text[search_from..search_from + end_rel];
    let marker_end = search_from + end_rel + ACTION_END.len();

    let value: Value = serde_json::from_str(json_part.trim()).ok()?;
    let name = value.get("action")?.as_str()?;
    let parameters = value.get("parameters")?;
    let action = super::action::action_from_tool_arguments(name, parameters).ok()?;

    let mut remaining = String::with_capacity(full_text.len());
    remaining.push_str(&full_text[..start]);
    remaining.push_str(&full_text[marker_end..]);

    Some(FallbackParseResult {
        text: remaining,
        action: Some(action),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ssh_manager_core::profiles::AiAction;

    #[test]
    fn test_parse_fallback_response_extracts_action_and_strips_marker() {
        let full_text =
            "Klar, das mache ich.\n<!--ACTION-->{\"action\": \"suggest_command\", \"parameters\": {\"command\": \"ls -la\"}}<!--/ACTION-->\nFertig.";

        let result = parse_fallback_response(full_text);

        assert_eq!(
            result.action,
            Some(AiAction::SuggestCommand {
                command: "ls -la".to_string()
            })
        );
        assert!(!result.text.contains(ACTION_START));
        assert!(result.text.contains("Klar, das mache ich."));
        assert!(result.text.contains("Fertig."));
    }

    /// `generate_document` läuft im Fallback-Modus über denselben
    /// `action::action_from_tool_arguments`-Pfad wie jede andere Aktion
    /// (`fallback_system_prompt_addition` iteriert bereits generisch über
    /// alle übergebenen `ActionSchema`s) — dieser Test deckt konkret ab,
    /// dass mehrzeiliger Markdown-Inhalt im `content_markdown`-Feld den
    /// Marker-Block selbst nicht durcheinanderbringt.
    #[test]
    fn test_parse_fallback_response_extracts_generate_document_action() {
        let full_text = "Hier ist die Analyse:\n<!--ACTION-->{\"action\": \"generate_document\", \
             \"parameters\": {\"title\": \"Analyse\", \"content_markdown\": \"# Analyse\\n\\nText.\"}}<!--/ACTION-->";

        let result = parse_fallback_response(full_text);

        assert_eq!(
            result.action,
            Some(AiAction::GenerateDocument {
                title: "Analyse".to_string(),
                content_markdown: "# Analyse\n\nText.".to_string(),
            })
        );
        assert!(!result.text.contains(ACTION_START));
    }

    #[test]
    fn test_parse_fallback_response_returns_full_text_on_malformed_json() {
        let full_text = "Text davor <!--ACTION-->{invalid json<!--/ACTION--> Text danach";

        let result = parse_fallback_response(full_text);

        assert_eq!(result.text, full_text);
        assert_eq!(result.action, None);
    }

    #[test]
    fn test_parse_fallback_response_returns_full_text_when_no_marker_present() {
        let full_text = "Ganz normale Antwort ohne Aktion.";

        let result = parse_fallback_response(full_text);

        assert_eq!(result.text, full_text);
        assert_eq!(result.action, None);
    }

    #[test]
    fn test_parse_fallback_response_returns_full_text_on_unknown_action_name() {
        let full_text = "<!--ACTION-->{\"action\": \"nuke\", \"parameters\": {}}<!--/ACTION-->";

        let result = parse_fallback_response(full_text);

        assert_eq!(result.text, full_text);
        assert_eq!(result.action, None);
    }
}
