//! UI preferences: the only way the webview changes `settings.json`
//! (issue #862).
//!
//! The webview may read the settings store but has no write permission on it
//! (capabilities). Plain UI preferences are written through
//! [`set_ui_preference`], which accepts only keys of the
//! [`UiPreferenceRegistry`] and only values of the shape registered for the
//! key. Security settings never belong in the registry: they have their own
//! backend commands (e.g. `risk_second_opinion::set_red_risk_always_confirm`).
//! An embedding app registers its own keys through `Wiring::ui_preferences`.

use std::collections::HashMap;

use serde_json::Value;
use tauri::{AppHandle, Runtime};

use app_logic::error::{CommandError, CommandResult, UI_PREFERENCE_REJECTED};

/// Keys that control security behaviour. They are written by dedicated
/// commands and can never be registered as UI preference.
const SECURITY_KEYS: &[&str] = &[
    "redRiskAlwaysConfirm",
    "riskClassifierEnabled",
    "riskClassifierProviderId",
    "localServerTags",
    "elevatedIdleTimeoutMinutes",
];

/// Every key starting with one of these belongs to a security feature.
const SECURITY_KEY_PREFIXES: &[&str] = &["mcpServer"];

/// Upper bound for the length of a key, to keep error messages and the file sane.
const MAX_KEY_LEN: usize = 64;

/// The JSON shape a UI preference value must have.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiPreferenceShape {
    Bool,
    /// An integer within the inclusive range.
    Integer {
        min: i64,
        max: i64,
    },
    /// A string of at most `max_len` characters.
    String {
        max_len: usize,
    },
    /// Any JSON value whose serialised form is at most `max_bytes` long.
    Json {
        max_bytes: usize,
    },
}

/// One registered preference key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiPreference {
    pub key: String,
    pub shape: UiPreferenceShape,
}

impl UiPreference {
    pub fn new(key: impl Into<String>, shape: UiPreferenceShape) -> Self {
        Self {
            key: key.into(),
            shape,
        }
    }
}

fn is_security_key(key: &str) -> bool {
    SECURITY_KEYS.contains(&key) || SECURITY_KEY_PREFIXES.iter().any(|p| key.starts_with(p))
}

fn rejected(message: impl Into<String>) -> CommandError {
    CommandError::with_code(message, UI_PREFERENCE_REJECTED)
}

impl UiPreferenceShape {
    fn check(&self, value: &Value) -> Result<(), String> {
        match self {
            Self::Bool if value.is_boolean() => Ok(()),
            Self::Bool => Err("expected a boolean".into()),
            Self::Integer { min, max } => match value.as_i64() {
                Some(n) if (*min..=*max).contains(&n) => Ok(()),
                _ => Err(format!("expected an integer between {min} and {max}")),
            },
            Self::String { max_len } => match value.as_str() {
                Some(s) if s.chars().count() <= *max_len => Ok(()),
                _ => Err(format!("expected a string of at most {max_len} characters")),
            },
            Self::Json { max_bytes } => match serde_json::to_vec(value) {
                Ok(bytes) if bytes.len() <= *max_bytes => Ok(()),
                _ => Err(format!("value larger than {max_bytes} bytes")),
            },
        }
    }
}

/// The set of keys `set_ui_preference` accepts. Managed as Tauri state.
#[derive(Debug, Clone)]
pub struct UiPreferenceRegistry {
    entries: HashMap<String, UiPreferenceShape>,
}

impl UiPreferenceRegistry {
    /// The preferences the community frontend keeps in the store.
    pub fn builtin() -> Self {
        use UiPreferenceShape::*;
        let entries = [
            ("language", String { max_len: 16 }),
            ("theme", String { max_len: 16 }),
            ("first_run_notice_acknowledged", Bool),
            ("voiceInputEnabled", Bool),
            (
                "autoReconnectSeconds",
                Integer {
                    min: 0,
                    max: 86_400,
                },
            ),
            (
                "fileTypeDefaultApps",
                Json {
                    max_bytes: 64 * 1024,
                },
            ),
            ("fileManagerColumnWidths", Json { max_bytes: 1024 }),
            (
                "aiSshSplitWidthPx",
                Integer {
                    min: 1,
                    max: 10_000,
                },
            ),
            ("fileManagerHiddenColumns", Json { max_bytes: 256 }),
            ("fileManagerSort", Json { max_bytes: 256 }),
        ];
        Self {
            entries: entries
                .into_iter()
                .map(|(k, s)| (k.to_string(), s))
                .collect(),
        }
    }

    /// The built-in keys plus `extra`. Fails for a security key, a key that
    /// is already registered (also within `extra`) and an unusable key name.
    pub fn with_extra(extra: Vec<UiPreference>) -> Result<Self, String> {
        let mut registry = Self::builtin();
        for UiPreference { key, shape } in extra {
            if key.is_empty() || key.len() > MAX_KEY_LEN {
                return Err(format!("invalid UI preference key {key:?}"));
            }
            if is_security_key(&key) {
                return Err(format!(
                    "{key:?} is a security setting and cannot be a UI preference"
                ));
            }
            if registry.entries.insert(key.clone(), shape).is_some() {
                return Err(format!("UI preference {key:?} is already registered"));
            }
        }
        Ok(registry)
    }

    /// Checks `key` and `value` without writing anything.
    pub fn validate(&self, key: &str, value: &Value) -> CommandResult<()> {
        // Defence in depth: the security check also holds for a registry
        // that was built without `with_extra`.
        if is_security_key(key) {
            return Err(rejected("This setting cannot be changed here."));
        }
        let Some(shape) = self.entries.get(key) else {
            return Err(rejected("Unknown UI preference."));
        };
        shape
            .check(value)
            .map_err(|reason| rejected(format!("Invalid value for {key}: {reason}")))
    }
}

/// Builds the registry at startup. A security key or duplicate among the
/// embedding app's extra keys is a programming error: fail before anything
/// starts.
pub(crate) fn startup_registry(wiring: &mut crate::Wiring) -> UiPreferenceRegistry {
    UiPreferenceRegistry::with_extra(std::mem::take(&mut wiring.ui_preferences))
        .unwrap_or_else(|err| panic!("invalid Wiring::ui_preferences: {err}"))
}

/// Validates, then writes in one atomic update. A rejected value leaves the
/// settings file untouched.
pub(crate) fn write_ui_preference<R: Runtime>(
    app: &AppHandle<R>,
    registry: &UiPreferenceRegistry,
    key: &str,
    value: Value,
) -> CommandResult<()> {
    registry.validate(key, &value)?;
    crate::settings_store::update(app, |changes| changes.set(key, value))
}

#[tauri::command]
pub fn set_ui_preference(
    app: AppHandle,
    registry: tauri::State<'_, UiPreferenceRegistry>,
    key: String,
    value: Value,
) -> CommandResult<()> {
    write_ui_preference(&app, &registry, &key, value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::first_run_notice::test_support::{lock, test_app};
    use crate::settings_store::{settings_store_path, SETTINGS_STORE_FILE};
    use serde_json::json;
    use tauri_plugin_store::StoreExt;

    fn valid_sample(shape: &UiPreferenceShape) -> Value {
        match shape {
            UiPreferenceShape::Bool => json!(true),
            UiPreferenceShape::Integer { min, .. } => json!(min),
            UiPreferenceShape::String { .. } => json!("x"),
            UiPreferenceShape::Json { .. } => json!({"a": 1}),
        }
    }

    #[test]
    fn test_every_builtin_key_accepts_a_valid_value() {
        let registry = UiPreferenceRegistry::builtin();
        for (key, shape) in &registry.entries {
            registry
                .validate(key, &valid_sample(shape))
                .unwrap_or_else(|e| panic!("{key}: {e:?}"));
        }
    }

    #[test]
    fn test_builtin_registry_has_no_security_key() {
        let registry = UiPreferenceRegistry::builtin();
        assert!(registry.entries.keys().all(|k| !is_security_key(k)));
        assert_eq!(registry.entries.len(), 10);
    }

    #[test]
    fn test_rejects_unknown_and_every_security_key() {
        let registry = UiPreferenceRegistry::builtin();
        let mut keys = vec!["nope", "chatSessionRetentionDays", "mcpServerToken"];
        keys.extend(SECURITY_KEYS);
        keys.extend([
            "mcpServerEnabled",
            "mcpServerAllowedServerIds",
            "mcpServerConfirmTimeoutSecs",
        ]);
        for key in keys {
            let err = registry.validate(key, &json!(true)).unwrap_err();
            assert_eq!(err.code, Some(UI_PREFERENCE_REJECTED), "{key}");
        }
    }

    #[test]
    fn test_rejects_wrong_shape_and_size() {
        let registry = UiPreferenceRegistry::builtin();
        for (key, value) in [
            ("voiceInputEnabled", json!("yes")),
            ("autoReconnectSeconds", json!(-1)),
            ("autoReconnectSeconds", json!(1_000_000)),
            ("autoReconnectSeconds", json!(1.5)),
            ("language", json!("x".repeat(17))),
            ("language", json!(3)),
            ("fileManagerColumnWidths", json!("x".repeat(2000))),
        ] {
            assert!(registry.validate(key, &value).is_err(), "{key} {value}");
        }
    }

    #[test]
    fn test_extra_keys_are_registered_and_security_or_duplicate_fail() {
        let ok = UiPreferenceRegistry::with_extra(vec![UiPreference::new(
            "extraFlag",
            UiPreferenceShape::Bool,
        )])
        .unwrap();
        assert!(ok.validate("extraFlag", &json!(false)).is_ok());
        assert!(ok.validate("extraFlag", &json!(1)).is_err());

        for key in ["redRiskAlwaysConfirm", "mcpServerFoo", "localServerTags"] {
            assert!(
                UiPreferenceRegistry::with_extra(vec![UiPreference::new(
                    key,
                    UiPreferenceShape::Bool
                )])
                .is_err(),
                "{key}"
            );
        }
        // Already built in, and twice within the extras.
        assert!(UiPreferenceRegistry::with_extra(vec![UiPreference::new(
            "language",
            UiPreferenceShape::Bool
        )])
        .is_err());
        let twice = UiPreference::new("twice", UiPreferenceShape::Bool);
        assert!(UiPreferenceRegistry::with_extra(vec![twice.clone(), twice]).is_err());
    }

    #[test]
    fn test_write_persists_valid_and_leaves_file_unchanged_on_rejection() {
        let _guard = lock();
        let app = test_app();
        let handle = app.handle();
        let registry = UiPreferenceRegistry::builtin();
        let path = settings_store_path(handle).unwrap();
        let before = std::fs::read(&path).ok();

        let rejected_writes = [
            ("unknownKey", json!(1)),
            ("redRiskAlwaysConfirm", json!(false)),
            ("voiceInputEnabled", json!("no")),
        ];
        for (key, value) in rejected_writes {
            assert!(write_ui_preference(handle, &registry, key, value).is_err());
        }
        assert_eq!(std::fs::read(&path).ok(), before);

        let original = handle
            .store(SETTINGS_STORE_FILE)
            .unwrap()
            .get("autoReconnectSeconds");
        write_ui_preference(handle, &registry, "autoReconnectSeconds", json!(300)).unwrap();
        let on_disk: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(on_disk["autoReconnectSeconds"], json!(300));

        // Restore what the shared settings file held before.
        let restore = original.unwrap_or(json!(60));
        write_ui_preference(handle, &registry, "autoReconnectSeconds", restore).unwrap();
    }

    /// Store permissions that let the webview change the settings file.
    const WRITE_STORE_PERMISSIONS: &[&str] = &[
        "store:default",
        "store:allow-set",
        "store:allow-delete",
        "store:allow-clear",
        "store:allow-reset",
        "store:allow-save",
    ];

    /// The offending permission identifiers in a capability file's JSON.
    fn writable_store_permissions(capability: &str) -> Vec<String> {
        let json: Value = serde_json::from_str(capability).expect("capability is JSON");
        json["permissions"]
            .as_array()
            .expect("permissions array")
            .iter()
            .filter_map(|p| p.as_str().or_else(|| p["identifier"].as_str()))
            .filter(|id| WRITE_STORE_PERMISSIONS.contains(id) || id.starts_with("store:allow-s"))
            .map(str::to_string)
            .collect()
    }

    #[test]
    fn test_capabilities_grant_no_store_write_permission() {
        for (name, text) in [
            (
                "default",
                include_str!("../../../apps/smart-ssh-community/capabilities/default.json"),
            ),
            (
                "mobile",
                include_str!("../../../apps/smart-ssh-community/capabilities/mobile.json"),
            ),
        ] {
            assert!(
                text.contains("store:allow-get"),
                "{name} must still read the store"
            );
            assert_eq!(
                writable_store_permissions(text),
                Vec::<String>::new(),
                "{name}"
            );
        }
    }

    #[test]
    fn test_capability_check_detects_a_write_permission() {
        let sample = r#"{"permissions": ["core:default", "store:allow-get", "store:allow-set"]}"#;
        assert_eq!(writable_store_permissions(sample), vec!["store:allow-set"]);
        let default = r#"{"permissions": [{"identifier": "store:default"}]}"#;
        assert_eq!(writable_store_permissions(default), vec!["store:default"]);
    }
}
