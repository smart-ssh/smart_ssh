//! Geteilte Übersetzung zwischen der providerunabhängigen [`ActionSchema`]
//! und JSON — sowohl das Werkzeug-/Function-Definitionsformat (das OpenAI-
//! und Anthropic-Tool-Schema sind bis auf die äußere Hülle identisches
//! JSON-Schema) als auch das Zurückparsen konkreter Tool-Argumente in eine
//! [`AiAction`] (Spec 0003, Abschnitt 5.2).

use serde_json::{Map, Value};
use ssh_manager_core::ai::{ActionParameterKind, ActionSchema, AiError};
use ssh_manager_core::profiles::{AiAction, GroupId, NoteTarget};
use ssh_manager_core::shared::ServerId;
use uuid::Uuid;

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
/// einer ungültigen `target_id` (keine gültige UUID) zurück — die
/// Aufrufer entscheiden je nach Modus (nativ vs. Fallback), wie damit
/// umzugehen ist (s. `openai_compatible.rs`/`anthropic.rs`).
pub(crate) fn action_from_tool_arguments(
    name: &str,
    arguments: &Value,
) -> Result<AiAction, AiError> {
    let get_str = |key: &str| -> Result<String, AiError> {
        arguments
            .get(key)
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| {
                AiError::InvalidResponse(format!("Feld '{key}' fehlt oder ist kein String"))
            })
    };

    match name {
        "suggest_command" => Ok(AiAction::SuggestCommand {
            command: get_str("command")?,
        }),
        "propose_note_update" => {
            let target_type = get_str("target_type")?;
            let target_id = get_str("target_id")?;
            let new_content = get_str("new_content")?;
            let uuid = Uuid::parse_str(&target_id).map_err(|err| {
                AiError::InvalidResponse(format!("target_id ist keine gültige UUID: {err}"))
            })?;
            let target = match target_type.as_str() {
                "server" => NoteTarget::Server(ServerId(uuid)),
                "group" => NoteTarget::Group(GroupId(uuid)),
                other => {
                    return Err(AiError::InvalidResponse(format!(
                        "unbekannter target_type '{other}'"
                    )))
                }
            };
            Ok(AiAction::ProposeNoteUpdate {
                target,
                new_content,
            })
        }
        "generate_document" => Ok(AiAction::GenerateDocument {
            title: get_str("title")?,
            content_markdown: get_str("content_markdown")?,
        }),
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
            json["properties"]["target_type"]["enum"],
            json!(["server", "group"])
        );
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
    fn test_action_from_tool_arguments_parses_propose_note_update_for_server() {
        let id = Uuid::new_v4();
        let action = action_from_tool_arguments(
            "propose_note_update",
            &json!({"target_type": "server", "target_id": id.to_string(), "new_content": "neu"}),
        )
        .unwrap();

        assert_eq!(
            action,
            AiAction::ProposeNoteUpdate {
                target: NoteTarget::Server(ServerId(id)),
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
    fn test_action_from_tool_arguments_rejects_invalid_target_id() {
        let result = action_from_tool_arguments(
            "propose_note_update",
            &json!({"target_type": "server", "target_id": "not-a-uuid", "new_content": "x"}),
        );

        assert!(matches!(result, Err(AiError::InvalidResponse(_))));
    }
}
