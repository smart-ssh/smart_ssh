//! Geteilte Übersetzung zwischen der providerunabhängigen [`ActionSchema`]
//! und JSON — sowohl das Werkzeug-/Function-Definitionsformat (das OpenAI-
//! und Anthropic-Tool-Schema sind bis auf die äußere Hülle identisches
//! JSON-Schema) als auch das Zurückparsen konkreter Tool-Argumente in eine
//! [`AiAction`] (Spec 0003, Abschnitt 5.2).

use serde_json::{Map, Value};
use ssh_manager_core::ai::{
    sanitize_tool_name, ActionParameterKind, ActionSchema, AiError, AiEvent, InvalidToolCallReason,
};
use ssh_manager_core::profiles::{AiAction, AskUserOption, NoteTargetSelector};

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
            ActionParameterKind::Boolean => {
                property.insert("type".to_string(), Value::String("boolean".to_string()));
            }
            ActionParameterKind::OptionList => {
                property.insert("type".to_string(), Value::String("array".to_string()));
                property.insert("minItems".to_string(), Value::from(2));
                property.insert("maxItems".to_string(), Value::from(4));
                property.insert(
                    "items".to_string(),
                    serde_json::json!({
                        "type": "object",
                        "properties": {
                            "label": {"type": "string"},
                            "description": {"type": "string"}
                        },
                        "required": ["label"]
                    }),
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
    parse_tool_action(name, arguments).map_err(|reason| match reason {
        InvalidToolCallReason::UnknownTool => {
            AiError::InvalidResponse(format!("unbekannte Aktion '{name}'"))
        }
        InvalidToolCallReason::MissingField(field) => {
            AiError::InvalidResponse(format!("Feld '{field}' fehlt oder ist kein String"))
        }
        InvalidToolCallReason::InvalidValue(field) => {
            AiError::InvalidResponse(format!("unbekannter {field}-Wert"))
        }
        InvalidToolCallReason::InvalidJson => {
            AiError::InvalidResponse("Tool-Argumente sind kein gültiges JSON".to_string())
        }
    })
}

/// Issue #604: a finished native tool call (name + raw JSON arguments) as
/// an event: a valid call is proposed; an unknown tool, broken JSON or
/// invalid fields become the non-fatal [`AiEvent::InvalidToolCall`] (logged,
/// never repaired into an action, never carrying the arguments).
pub(crate) fn tool_call_event(request_id: uuid::Uuid, name: &str, raw_arguments: &str) -> AiEvent {
    use crate::request_logging::{log_tool_call_parse_error, log_tool_call_parsed};
    let parsed = match serde_json::from_str::<Value>(raw_arguments) {
        Ok(args) => parse_tool_action(name, &args),
        // An unknown tool is reported as such even with empty arguments.
        Err(_) => Err(match parse_tool_action(name, &Value::Null) {
            Err(InvalidToolCallReason::UnknownTool) => InvalidToolCallReason::UnknownTool,
            _ => InvalidToolCallReason::InvalidJson,
        }),
    };
    match parsed {
        Ok(action) => {
            log_tool_call_parsed(request_id, &action);
            AiEvent::ActionProposed(action)
        }
        Err(reason) => {
            let err = AiError::InvalidResponse(reason.describe());
            log_tool_call_parse_error(request_id, name, raw_arguments, err.code(), &err);
            AiEvent::InvalidToolCall {
                tool_name: sanitize_tool_name(name),
                reason,
            }
        }
    }
}

fn parse_tool_action(name: &str, arguments: &Value) -> Result<AiAction, InvalidToolCallReason> {
    let get_str_aliases = |keys: &[&str]| -> Result<String, InvalidToolCallReason> {
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
        Err(InvalidToolCallReason::MissingField(keys[0].to_string()))
    };

    match name {
        "suggest_command" => Ok(AiAction::SuggestCommand {
            command: get_str_aliases(&["command", "cmd", "script", "code"])?,
        }),
        "propose_note_update" => {
            let new_content =
                get_str_aliases(&["new_content", "content", "notes", "note", "text"])?;
            // Spec 0016, Abschnitt 6: kein Freitext-`target_id`-Feld mehr —
            // die KI wählt nur noch zwischen zwei relativen Optionen, nie
            // eine ID. Fehlt `target`, gilt `current_server` als Default
            // (Spec-Vorgabe), nicht als Fehler.
            let target = match arguments.get("target").and_then(Value::as_str) {
                None | Some("current_server") => NoteTargetSelector::CurrentServer,
                Some("current_server_group") => NoteTargetSelector::CurrentServerGroup,
                Some(_) => return Err(InvalidToolCallReason::InvalidValue("target".to_string())),
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
        "read_remote_file" => Ok(AiAction::ReadRemoteFile {
            path: get_str_aliases(&["path", "file", "filename", "remote_path"])?,
        }),
        "write_remote_file" => Ok(AiAction::WriteRemoteFile {
            path: get_str_aliases(&["path", "file", "filename", "remote_path"])?,
            content: get_str_aliases(&["content", "new_content", "text", "body"])?,
        }),
        "ask_user" => {
            let question = get_str_aliases(&["question", "text", "prompt"])?;
            // Lenient on purpose: an invalid option list is not an error
            // here, the caller shows such a call as plain text.
            let options = arguments
                .get("options")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .map(|item| match item {
                            Value::String(s) => AskUserOption {
                                label: s.clone(),
                                description: None,
                            },
                            other => AskUserOption {
                                label: other
                                    .get("label")
                                    .and_then(Value::as_str)
                                    .unwrap_or_default()
                                    .to_string(),
                                description: other
                                    .get("description")
                                    .and_then(Value::as_str)
                                    .filter(|d| !d.is_empty())
                                    .map(str::to_string),
                            },
                        })
                        .collect()
                })
                .unwrap_or_default();
            let multi_select = arguments
                .get("multi_select")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            Ok(AiAction::AskUser {
                question,
                options,
                multi_select,
            })
        }
        _ => Err(InvalidToolCallReason::UnknownTool),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Issue #545: schema for `ask_user` (shared by all providers).
    #[test]
    fn test_ask_user_json_schema_has_option_array_and_boolean() {
        for schema in [ActionSchema::ask_user(), ActionSchema::ask_user_en()] {
            let json = parameters_json_schema(&schema);
            let options = &json["properties"]["options"];
            assert_eq!(options["type"], "array");
            assert_eq!(options["minItems"], 2);
            assert_eq!(options["maxItems"], 4);
            assert_eq!(options["items"]["type"], "object");
            assert_eq!(options["items"]["required"], serde_json::json!(["label"]));
            assert_eq!(json["properties"]["multi_select"]["type"], "boolean");
            assert_eq!(json["required"], serde_json::json!(["question", "options"]));
        }
    }

    #[test]
    fn test_action_from_tool_arguments_parses_ask_user_leniently() {
        let action = action_from_tool_arguments(
            "ask_user",
            &json!({"question": "Q?", "options": [{"label": "A"}]}),
        )
        .unwrap();
        // Too few options is not a parse error; the caller shows it as text.
        assert_eq!(
            action,
            AiAction::AskUser {
                question: "Q?".to_string(),
                options: vec![AskUserOption {
                    label: "A".to_string(),
                    description: None
                }],
                multi_select: false,
            }
        );
        assert!(action_from_tool_arguments("ask_user", &json!({"options": []})).is_err());
    }

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
        let action =
            action_from_tool_arguments("propose_note_update", &json!({"new_content": "neu"}))
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
        let result =
            action_from_tool_arguments("propose_note_update", &json!({"target": "current_server"}));

        assert!(matches!(result, Err(AiError::InvalidResponse(_))));
    }

    /// Issue #604: an invalid call is never repaired into an action and
    /// never carries its arguments, even when they hold a command string.
    #[test]
    fn test_invalid_tool_calls_never_become_actions_or_echo_arguments() {
        let id = uuid::Uuid::new_v4();
        let cases = [
            ("date", r#"{"command":"rm -rf /"}"#),
            ("suggest_command", r#"{"cmd_typo":"rm -rf /"}"#),
            ("suggest_command", r#"{"command":"rm -rf /""#),
            (
                "propose_note_update",
                r#"{"content":"x","target":"rm -rf /"}"#,
            ),
            ("da\u{0}te\n", "{}"),
        ];
        for (name, args) in cases {
            match tool_call_event(id, name, args) {
                AiEvent::InvalidToolCall { tool_name, reason } => {
                    assert!(!tool_name.chars().any(char::is_control));
                    assert!(!reason.describe().contains("rm -rf"), "{reason:?}");
                }
                other => panic!("{name} {args}: {other:?}"),
            }
        }
        assert!(matches!(
            tool_call_event(id, "suggest_command", r#"{"command":"ls"}"#),
            AiEvent::ActionProposed(_)
        ));
    }
}
