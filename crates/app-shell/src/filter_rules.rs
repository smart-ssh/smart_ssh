//! Reine, testbare Logik rund um Filter-Regeln (Spec 0009) — die
//! `#[tauri::command]`-Wrapper in `crate::commands` bleiben dünn, analog zu
//! `crate::groups`/`crate::server_credentials` (Spec 0008).

use chrono::Utc;
use uuid::Uuid;

use persistence_sqlite::{PolicyStoreError, SqlitePolicyStore};
use ssh_manager_core::filter::{FilterEngine, RuleId, Scope};

use crate::dto::{EvalContextInput, EvaluationTraceDto, RuleDto, RuleInput};

/// Neue Regel aus `input` anlegen — generiert eine frische [`RuleId`] (Spec
/// 0009, Abschnitt 3: `create_rule(input: RuleInput) -> RuleId`) und setzt
/// `created_at`/`updated_at` auf denselben Zeitpunkt, analog zu
/// `AiProviderConfigInput::into_new_config`.
pub async fn create_rule(
    policy_store: &SqlitePolicyStore,
    input: RuleInput,
) -> Result<RuleId, PolicyStoreError> {
    let id = RuleId(Uuid::new_v4().to_string());
    let now = Utc::now();
    let stored = input.into_stored_rule(id.clone(), now, now);
    policy_store.create(&stored).await?;
    Ok(id)
}

/// Aktualisiert Regel `id` mit den Feldern aus `input` — `created_at` bleibt
/// erhalten (`SqlitePolicyStore::update` rührt es nicht an), nur
/// `updated_at` wird neu gesetzt.
pub async fn update_rule(
    policy_store: &SqlitePolicyStore,
    id: RuleId,
    input: RuleInput,
) -> Result<(), PolicyStoreError> {
    let existing = policy_store.get(&id).await?;
    let stored = input.into_stored_rule(id, existing.created_at, Utc::now());
    policy_store.update(&stored).await
}

/// Alle Regeln, optional auf einen exakten Scope gefiltert (Spec 0009,
/// Abschnitt 3: `list_rules(scope_filter: Option<ScopeFilter>)` — hier
/// `Option<Scope>`, s. `RuleInput`-Doc-Kommentar zur `ScopeFilter`-
/// Vereinfachung). "Exakt" heißt: `Some(Scope::Server(id))` liefert nur
/// Regeln, die selbst mit `scope_type='server'` auf genau `id` angelegt
/// wurden — anders als `PolicyStore::rules_for`, das für eine
/// *Auswertung* auch global/Tag-Regeln einschließt, die für einen Server
/// zusätzlich gelten. Für die Regel-**Verwaltungsansicht** (Browsen/
/// Bearbeiten nach definiertem Scope) ist die exakte Filterung die
/// richtige Semantik.
pub async fn list_rules(
    policy_store: &SqlitePolicyStore,
    scope_filter: Option<Scope>,
) -> Result<Vec<RuleDto>, PolicyStoreError> {
    let all = policy_store.list_all().await?;
    Ok(all
        .iter()
        .filter(|rule| match &scope_filter {
            None => true,
            Some(filter) => &rule.scope == filter,
        })
        .map(RuleDto::from)
        .collect())
}

/// Baut eine [`EvaluationTraceDto`] für das Testen-Panel (Spec 0009,
/// Abschnitt 4/6). Eine neue [`FilterEngine`] pro Aufruf statt einer in
/// `AppState` gehaltenen Instanz: `SqlitePolicyStore` ist günstig zu klonen
/// (s. dortiger Doc-Kommentar) und `evaluate_explained` liest ohnehin bei
/// jedem Aufruf frisch aus der Datenbank — eine dauerhaft gehaltene
/// `FilterEngine` hätte hier keinen Vorteil, nur zusätzlichen State.
pub async fn evaluate_explained(
    policy_store: SqlitePolicyStore,
    command: String,
    ctx: EvalContextInput,
) -> EvaluationTraceDto {
    let engine = FilterEngine::new(policy_store);
    let trace = engine.evaluate_explained(&command, &ctx.into()).await;
    EvaluationTraceDto::from(trace)
}

#[cfg(test)]
mod tests {
    use ssh_manager_core::filter::RuleAction;

    use super::*;
    use crate::dto::PatternType;

    /// Kein `:memory:`-Store hier (anders als in `persistence-sqlite`
    /// selbst): dessen `SqliteConnectOptions`-Konstruktor ist bewusst nur
    /// `pub(crate)` innerhalb dieser Crate (s. dortiger Doc-Kommentar), der
    /// öffentliche Einstiegspunkt für alle anderen Crates ist `connect(db_path)`
    /// — ein echtes, temporäres Verzeichnis ist hier daher der korrekte Weg,
    /// nicht ein Workaround. Gibt den `TempDir`-Guard mit zurück: der Aufrufer
    /// muss ihn für die Dauer des Tests am Leben halten, sonst wird das
    /// Verzeichnis (und damit die Datenbankdatei) vorzeitig gelöscht.
    async fn in_memory_store() -> (tempfile::TempDir, SqlitePolicyStore) {
        let dir = tempfile::tempdir().expect("Temp-Verzeichnis sollte anlegbar sein");
        let db_path = dir.path().join("test.db");
        let store = persistence_sqlite::SqliteProfileStore::connect(&db_path)
            .await
            .expect(
                "frische SQLite-Datenbank mit angewendeten Migrationen sollte immer aufbaubar sein",
            );
        (dir, store.policy_store())
    }

    fn allow_ls_input(scope: Scope) -> RuleInput {
        RuleInput {
            pattern_type: PatternType::Glob,
            pattern_value: "ls *".to_string(),
            action: RuleAction::Allow,
            scope,
            priority: 0,
        }
    }

    #[tokio::test]
    async fn test_create_rule_then_list_roundtrip() {
        let (_dir, store) = in_memory_store().await;

        let id = create_rule(&store, allow_ls_input(Scope::Global))
            .await
            .unwrap();

        let listed = list_rules(&store, None).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, id);
        assert_eq!(listed[0].pattern_value, "ls *");
        assert_eq!(listed[0].action, RuleAction::Allow);
    }

    #[tokio::test]
    async fn test_update_rule_changes_action() {
        let (_dir, store) = in_memory_store().await;
        let id = create_rule(&store, allow_ls_input(Scope::Global))
            .await
            .unwrap();

        let mut updated_input = allow_ls_input(Scope::Global);
        updated_input.action = RuleAction::Deny;
        update_rule(&store, id.clone(), updated_input)
            .await
            .unwrap();

        let fetched = store.get(&id).await.unwrap();
        assert_eq!(fetched.action, RuleAction::Deny);
    }

    #[tokio::test]
    async fn test_list_rules_filters_by_exact_scope() {
        let (_dir, store) = in_memory_store().await;
        let server_id = ssh_manager_core::shared::ServerId::new();
        create_rule(&store, allow_ls_input(Scope::Global))
            .await
            .unwrap();
        create_rule(&store, allow_ls_input(Scope::Server(server_id)))
            .await
            .unwrap();
        create_rule(&store, allow_ls_input(Scope::Tag("production".to_string())))
            .await
            .unwrap();

        let server_only = list_rules(&store, Some(Scope::Server(server_id)))
            .await
            .unwrap();
        assert_eq!(server_only.len(), 1);
        assert_eq!(server_only[0].scope, Scope::Server(server_id));
    }

    /// Kernfall aus der Aufgabenstellung (Teil 1, Punkt 6): `ls -la && rm
    /// -rf /tmp/x` liefert sowohl je einen Trace pro Teilkommando als auch
    /// eine nachvollziehbare Gesamt-Entscheidung.
    #[tokio::test]
    async fn test_evaluate_explained_reports_chaining_sub_traces() {
        let (_dir, store) = in_memory_store().await;
        create_rule(&store, allow_ls_input(Scope::Global))
            .await
            .unwrap();

        let trace = evaluate_explained(
            store,
            "ls -la && rm -rf /tmp/x".to_string(),
            EvalContextInput {
                server_id: None,
                tags: Vec::new(),
            },
        )
        .await;

        assert_eq!(trace.sub_command_traces.len(), 2);
        assert!(matches!(
            trace.sub_command_traces[0].decision,
            ssh_manager_core::filter::Decision::AutoExec
        ));
        assert!(!matches!(
            trace.decision,
            ssh_manager_core::filter::Decision::AutoExec
        ));
    }

    #[tokio::test]
    async fn test_evaluate_explained_single_command_reports_matched_rule() {
        let (_dir, store) = in_memory_store().await;
        let id = create_rule(&store, allow_ls_input(Scope::Global))
            .await
            .unwrap();

        let trace = evaluate_explained(
            store,
            "ls -la".to_string(),
            EvalContextInput {
                server_id: None,
                tags: Vec::new(),
            },
        )
        .await;

        assert_eq!(trace.matched_rule, Some(id));
        assert!(matches!(
            trace.decision,
            ssh_manager_core::filter::Decision::AutoExec
        ));
    }
}
