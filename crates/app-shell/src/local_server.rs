//! Lokaler Pseudo-Server "Localhost" (Spec 0032) — existiert nicht als
//! Zeile in der `servers`-Tabelle, sondern wird zur Laufzeit synthetisiert
//! (Abschnitt 3). Notizen/Tags sind trotzdem editierbar, aber bewusst
//! **nicht** über `note_revisions`/`server_tags` gespeichert: beide Tabellen
//! erzwingen eine existierende `servers`-Zeile (`server_tags.server_id` hat
//! einen `FOREIGN KEY`, `record_note_revision` schreibt in derselben
//! Transaktion ein `UPDATE servers SET notes = ... WHERE id = ?` und schlägt
//! mit `ServerNotFound` fehl, wenn das null Zeilen trifft — beides für eine
//! nicht existierende `servers`-Zeile unvermeidbar). Deshalb hier über
//! denselben `tauri-plugin-store` wie andere reine App-Einstellungen (Spec
//! 0024-Muster) — bewusster Funktionsverzicht gegenüber einem echten
//! Server: keine Notiz-**Historie** für den lokalen Pseudo-Server, nur der
//! aktuelle Stand (Spec 0032 selbst verlangt nur "editierbar", keine
//! Historie).

use tauri::{AppHandle, Runtime};
use tauri_plugin_store::StoreExt;
use uuid::Uuid;

use ssh_manager_core::profiles::AuthMethod;
use ssh_manager_core::profiles::PostIngestPolicy;
use ssh_manager_core::profiles::Server;
use ssh_manager_core::shared::ServerId;

/// Spec 0032, Abschnitt 3: fest reservierte, konstante `ServerId` — die
/// Nil-UUID kann nie von `ServerId::new()` (UUID v4) erzeugt werden, daher
/// als Reservierung kollisionsfrei.
pub const LOCAL_SERVER_ID: ServerId = ServerId(Uuid::nil());

const SETTINGS_STORE_FILE: &str = "settings.json";
const NOTES_KEY: &str = "localServerNotes";
const TAGS_KEY: &str = "localServerTags";

pub fn is_local(id: ServerId) -> bool {
    id == LOCAL_SERVER_ID
}

fn load_notes<R: Runtime>(app: &AppHandle<R>) -> String {
    app.store(SETTINGS_STORE_FILE)
        .ok()
        .and_then(|store| store.get(NOTES_KEY))
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
}

fn load_tags<R: Runtime>(app: &AppHandle<R>) -> Vec<String> {
    app.store(SETTINGS_STORE_FILE)
        .ok()
        .and_then(|store| store.get(TAGS_KEY))
        .and_then(|v| v.as_array().cloned())
        .map(|arr| {
            arr.into_iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

pub fn save_notes<R: Runtime>(app: &AppHandle<R>, notes: &str) -> Result<(), String> {
    let store = app.store(SETTINGS_STORE_FILE).map_err(|e| e.to_string())?;
    store.set(NOTES_KEY, serde_json::json!(notes));
    store.save().map_err(|e| e.to_string())
}

pub fn save_tags<R: Runtime>(app: &AppHandle<R>, tags: &[String]) -> Result<(), String> {
    let store = app.store(SETTINGS_STORE_FILE).map_err(|e| e.to_string())?;
    store.set(TAGS_KEY, serde_json::json!(tags));
    store.save().map_err(|e| e.to_string())
}

/// Baut den synthetischen `Server` für die Kernschleife (Filter-Engine-
/// `EvalContext`, `effective_notes()`, Session-Aufbau) — `host`/`port`/
/// `username`/`auth` sind bedeutungslose Platzhalter (Spec 0032, Abschnitt
/// 3: im Formular ohnehin ausgeblendet), `group_id`/`jump_host` bewusst
/// `None` (Spec 0032/0033: nie in einer Gruppe, nie als Jump-Host
/// referenzierbar).
pub fn synthetic_server<R: Runtime>(app: &AppHandle<R>) -> Server {
    let now = chrono::Utc::now();
    Server {
        id: LOCAL_SERVER_ID,
        name: "Localhost".to_string(),
        host: "localhost".to_string(),
        port: 0,
        username: whoami_fallback(),
        group_id: None,
        tags: load_tags(app),
        auth: AuthMethod::Agent,
        notes: load_notes(app),
        jump_host: None,
        // Der lokale Pseudo-Server hat kein Einstellungs-UI für diese Stufe
        // (keine `servers`-Zeile, s. Moduldoc) — Default wie jeder neue
        // Server.
        post_ingest_policy: PostIngestPolicy::default(),
        ai_injection_check_enabled: false,
        created_at: now,
        updated_at: now,
    }
}

fn whoami_fallback() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "local".to_string())
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;

    use ssh_manager_core::filter::{
        EffectiveScope, EvalContext, FilterEngine, Pattern, PolicyStore, Rule, RuleAction, RuleId,
        RuleOrigin, Scope,
    };

    use crate::first_run_notice::test_support::{lock, test_app};

    use super::*;

    fn reset_local_store<R: Runtime>(app: &AppHandle<R>) {
        if let Ok(store) = app.store(SETTINGS_STORE_FILE) {
            store.delete(NOTES_KEY);
            store.delete(TAGS_KEY);
            let _ = store.save();
        }
    }

    #[test]
    fn test_local_server_id_is_the_nil_uuid_and_never_equals_a_fresh_server_id() {
        assert_eq!(LOCAL_SERVER_ID.0, Uuid::nil());
        assert_ne!(LOCAL_SERVER_ID, ServerId::new());
    }

    #[test]
    fn test_synthetic_server_carries_persisted_notes_and_tags() {
        let _guard = lock();
        let app = test_app();
        let handle = app.handle().clone();
        reset_local_store(&handle);

        save_notes(&handle, "Notiz zum lokalen Rechner").unwrap();
        save_tags(&handle, &["dev".to_string(), "local".to_string()]).unwrap();

        let server = synthetic_server(&handle);

        assert_eq!(server.id, LOCAL_SERVER_ID);
        assert_eq!(server.notes, "Notiz zum lokalen Rechner");
        assert_eq!(server.tags, vec!["dev".to_string(), "local".to_string()]);
        assert_eq!(server.group_id, None);
        assert_eq!(server.jump_host, None);

        reset_local_store(&handle);
    }

    #[test]
    fn test_synthetic_server_defaults_to_empty_notes_and_tags_when_never_saved() {
        let _guard = lock();
        let app = test_app();
        let handle = app.handle().clone();
        reset_local_store(&handle);

        let server = synthetic_server(&handle);

        assert_eq!(server.notes, "");
        assert!(server.tags.is_empty());
    }

    struct SingleRulePolicyStore(Rule);

    #[async_trait]
    impl PolicyStore for SingleRulePolicyStore {
        // Spiegelt exakt, was `SqlitePolicyStore` (und jede andere echte
        // Implementierung) hier auch tut: die Scope-Filterung passiert IN
        // `rules_for`, nicht erst in `FilterEngine::evaluate` — s.
        // `ssh_manager_core::filter::scope_applies`-Doc-Kommentar
        // ("öffentlich, damit PolicyStore-Implementierungen dieselbe Logik
        // verwenden").
        async fn rules_for(&self, scope: &EffectiveScope) -> Vec<Rule> {
            if ssh_manager_core::filter::scope_applies(&self.0.scope, scope) {
                vec![self.0.clone()]
            } else {
                Vec::new()
            }
        }
    }

    /// Spec 0032, Abschnitt 4: die Filter-Engine kennt `LOCAL_SERVER_ID`
    /// nicht als Sonderfall — eine `Scope::Server`-Regel muss für den
    /// lokalen Pseudo-Server exakt so greifen wie für jeden anderen Server
    /// (kein Bypass). Verglichen wird direkt gegen einen zweiten, "echten"
    /// `ServerId`, für den dieselbe Regel (falscher Scope) nicht matcht.
    #[tokio::test]
    async fn test_filter_engine_evaluates_local_pseudo_server_like_any_real_server() {
        let deny_rule = Rule {
            id: RuleId("deny-rm".to_string()),
            pattern: Pattern::Glob("rm -rf *".to_string()),
            action: RuleAction::Deny,
            scope: Scope::Server(LOCAL_SERVER_ID),
            priority: 0,
            origin: RuleOrigin::User,
        };
        let engine = FilterEngine::new(SingleRulePolicyStore(deny_rule));

        let local_ctx = EvalContext {
            server_id: LOCAL_SERVER_ID,
            tags: Vec::new(),
        };
        let local_decision = engine.evaluate("rm -rf /tmp/x", &local_ctx).await;
        assert!(
            matches!(local_decision, ssh_manager_core::filter::Decision::Deny { .. }),
            "Deny-Regel mit Scope::Server(LOCAL_SERVER_ID) muss für den lokalen Pseudo-Server greifen"
        );

        // Dieselbe Regel (Scope::Server(LOCAL_SERVER_ID)) darf für einen
        // ANDEREN Server nicht greifen — bestätigt, dass die Engine
        // tatsächlich nach ID unterscheidet statt den lokalen Server
        // pauschal zu bevorzugen/übergehen.
        let other_ctx = EvalContext {
            server_id: ServerId::new(),
            tags: Vec::new(),
        };
        let other_decision = engine.evaluate("rm -rf /tmp/x", &other_ctx).await;
        assert!(
            matches!(
                other_decision,
                ssh_manager_core::filter::Decision::Confirm { .. }
            ),
            "dieselbe Server-Scope-Regel darf für einen fremden Server nicht greifen"
        );
    }

    /// Spiegelbild von oben: eine ganz normale `Scope::Server(<echte ID>)`-
    /// Regel darf umgekehrt auch nicht versehentlich den lokalen
    /// Pseudo-Server treffen.
    #[tokio::test]
    async fn test_filter_engine_rule_for_real_server_does_not_leak_to_local_pseudo_server() {
        let real_id = ServerId::new();
        let deny_rule = Rule {
            id: RuleId("deny-rm-real".to_string()),
            pattern: Pattern::Glob("rm -rf *".to_string()),
            action: RuleAction::Deny,
            scope: Scope::Server(real_id),
            priority: 0,
            origin: RuleOrigin::User,
        };
        let engine = FilterEngine::new(SingleRulePolicyStore(deny_rule));

        let local_ctx = EvalContext {
            server_id: LOCAL_SERVER_ID,
            tags: Vec::new(),
        };
        let decision = engine.evaluate("rm -rf /tmp/x", &local_ctx).await;
        assert!(matches!(
            decision,
            ssh_manager_core::filter::Decision::Confirm { .. }
        ));
    }
}
