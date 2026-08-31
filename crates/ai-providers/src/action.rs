//! Geteilte Übersetzung zwischen der providerunabhängigen [`ActionSchema`]
//! und JSON — sowohl das Werkzeug-/Function-Definitionsformat (das OpenAI-
//! und Anthropic-Tool-Schema sind bis auf die äußere Hülle identisches
//! JSON-Schema) als auch das Zurückparsen konkreter Tool-Argumente in eine
//! [`AiAction`] (Spec 0003, Abschnitt 5.2).

use serde_json::{Map, Value};
use ssh_manager_core::ai::{ActionParameterKind, ActionSchema, AiError};
use ssh_manager_core::profiles::{AiAction, NoteTargetSelector};

/// Erzeugt das `{"type":"object","properties":{...},"required":[...]}`-
/// JSON-Schema-Objekt für eine [`ActionSchema`]. Von OpenAI (`function.parameters`)
/// und Anthropic (`input_schema`) unverändert wiederverwendbar.
pub(crate) fn parameters_json_schema(schema: &ActionSchema) -> Value {
    let mut properties = Map::new();
    let mut required = Vec::new();
    for param in &schema.parameters {
        let mut property = Map::new();
        match &param.kind {
            ActionParameterKind::String => {
                property.insert("type".to_string(), Value::String("string".to_string()));
            }
            ActionParameterKind::Enum(values) => {
                property.insert("type".to_string(), Value::String("string".to_string()));
                property.insert(
                    "enum".to_string(),
                    Value::Array(values.iter().cloned().map(Value::String).collect()),
                );
            }
        }
        property.insert(
            "description".to_string(),
            Value::String(param.description.clone()),
        );
        properties.insert(param.name.clone(), Value::Object(property));
        if param.required {
            required.push(Value::String(param.name.clone()));
        }
    }
    Value::Object(Map::from_iter([
        ("type".to_string(), Value::String("object".to_string())),
        ("properties".to_string(), Value::Object(properties)),
        ("required".to_string(), Value::Array(required)),
    ]))
}

/// Parst Tool-/Function-Argumente (Name + JSON-Argumentobjekt, wie sie
/// sowohl aus einem nativen Tool-Call als auch aus dem Fallback-JSON-Block
/// kommen) in eine konkrete [`AiAction`] (Spec 0003, Abschnitt 5.2).
///
/// Gibt `Err` bei unbekanntem Aktionsnamen, fehlenden Pflichtfeldern oder
/// einem unbekannten `target`-Wert zurück — die Aufrufer entscheiden je
/// nach Modus (nativ vs. Fallback), wie damit umzugehen ist (s.
/// `openai_compatible.rs`/`anthropic.rs`).
pub(crate) fn action_from_tool_arguments(
    name: &str,
    arguments: &Value,
) -> Result<AiAction, AiError> {
    let get_str_aliases = |keys: &[&str]| -> Result<String, AiError> {
        for &key in keys {
            if let Some(val) = arguments.get(key) {
                if let Some(s) = val.as_str() {
                    return Ok(s.to_string());
                }
                if !val.is_null() && !val.is_object() && !val.is_array() {
                    return Ok(val.to_string());
                }
            }
        }
        Err(AiError::InvalidResponse(format!(
            "Feld '{}' fehlt oder ist kein String",
            keys[0]
        )))
    };

    match name {
        "suggest_command" => Ok(AiAction::SuggestCommand {
            command: get_str_aliases(&["command", "cmd", "script", "code"])?,
        }),
        "propose_note_update" => {
            let new_content = get_str_aliases(&["new_content", "content", "notes", "note", "text"])?;
            // Spec 0016, Abschnitt 6: kein Freitext-`target_id`-Feld mehr —
            // die KI wählt nur noch zwischen zwei relativen Optionen, nie
            // eine ID. Fehlt `target`, gilt `current_server` als Default
            // (Spec-Vorgabe), nicht als Fehler.
            let target = match arguments.get("target").and_then(Value::as_str) {
                None | Some("current_server") => NoteTargetSelector::CurrentServer,
                Some("current_server_group") => NoteTargetSelector::CurrentServerGroup,
                Some(other) => {
                    return Err(AiError::InvalidResponse(format!(
                        "unbekannter target-Wert '{other}'"
                    )))
                }
            };
            Ok(AiAction::ProposeNoteUpdate {
                target,
                new_content,
            })
        }
        "generate_document" => {
            let title = get_str_aliases(&["title", "name", "document_title", "topic", "filename"])
                .unwrap_or_else(|_| "Dokument".to_string());
            let content_markdown = get_str_aliases(&[
                "content_markdown",
                "content",
                "markdown",
                "text",
                "body",
                "document",
                "markdown_content",
            ])?;
            Ok(AiAction::GenerateDocument {
                title,
                content_markdown,
            })
        }
        other => Err(AiError::InvalidResponse(format!(
            "unbekannte Aktion '{other}'"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_parameters_json_schema_marks_required_fields() {
        let schema = ActionSchema::suggest_command();

        let json = parameters_json_schema(&schema);

        assert_eq!(json["type"], "object");
        assert_eq!(json["properties"]["command"]["type"], "string");
        assert_eq!(json["required"], json!(["command"]));
    }

    #[test]
    fn test_parameters_json_schema_encodes_enum_kind() {
        let schema = ActionSchema::propose_note_update();

        let json = parameters_json_schema(&schema);

        assert_eq!(
            json["properties"]["target"]["enum"],
            json!(["current_server", "current_server_group"])
        );
        assert_eq!(json["required"], json!(["new_content"]));
    }

    #[test]
    fn test_action_from_tool_arguments_parses_suggest_command() {
        let action =
            action_from_tool_arguments("suggest_command", &json!({"command": "ls -la"})).unwrap();

        assert_eq!(
            action,
            AiAction::SuggestCommand {
                command: "ls -la".to_string()
            }
        );
    }

    #[test]
    fn test_action_from_tool_arguments_parses_propose_note_update_for_current_server() {
        let action = action_from_tool_arguments(
            "propose_note_update",
            &json!({"target": "current_server", "new_content": "neu"}),
        )
        .unwrap();

        assert_eq!(
            action,
            AiAction::ProposeNoteUpdate {
                target: NoteTargetSelector::CurrentServer,
                new_content: "neu".to_string(),
            }
        );
    }

    #[test]
    fn test_action_from_tool_arguments_parses_propose_note_update_for_current_server_group() {
        let action = action_from_tool_arguments(
            "propose_note_update",
            &json!({"target": "current_server_group", "new_content": "neu"}),
        )
        .unwrap();

        assert_eq!(
            action,
            AiAction::ProposeNoteUpdate {
                target: NoteTargetSelector::CurrentServerGroup,
                new_content: "neu".to_string(),
            }
        );
    }

    /// Spec 0016, Abschnitt 6: fehlt `target` ganz, gilt `current_server`
    /// als Default — kein Fehler, und die KI muss das Feld nicht einmal
    /// kennen, um den häufigsten Fall (aktueller Server) auszulösen.
    #[test]
    fn test_action_from_tool_arguments_defaults_missing_target_to_current_server() {
        let action = action_from_tool_arguments(
            "propose_note_update",
            &json!({"new_content": "neu"}),
        )
        .unwrap();

        assert_eq!(
            action,
            AiAction::ProposeNoteUpdate {
                target: NoteTargetSelector::CurrentServer,
                new_content: "neu".to_string(),
            }
        );
    }

    #[test]
    fn test_action_from_tool_arguments_parses_generate_document() {
        let action = action_from_tool_arguments(
            "generate_document",
            &json!({"title": "Analyse", "content_markdown": "# Analyse\n\nInhalt."}),
        )
        .unwrap();

        assert_eq!(
            action,
            AiAction::GenerateDocument {
                title: "Analyse".to_string(),
                content_markdown: "# Analyse\n\nInhalt.".to_string(),
            }
        );
    }

    #[test]
    fn test_action_from_tool_arguments_rejects_unknown_action_name() {
        let result = action_from_tool_arguments("delete_everything", &json!({}));

        assert!(matches!(result, Err(AiError::InvalidResponse(_))));
    }

    #[test]
    fn test_action_from_tool_arguments_rejects_unknown_target_value() {
        let result = action_from_tool_arguments(
            "propose_note_update",
            &json!({"target": "some_other_server", "new_content": "x"}),
        );

        assert!(matches!(result, Err(AiError::InvalidResponse(_))));
    }

    #[test]
    fn test_action_from_tool_arguments_rejects_missing_new_content() {
        let result = action_from_tool_arguments(
            "propose_note_update",
            &json!({"target": "current_server"}),
        );

        assert!(matches!(result, Err(AiError::InvalidResponse(_))));
    }
}
