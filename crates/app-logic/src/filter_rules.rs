//! Reine, testbare Logik rund um Filter-Regeln (Spec 0009) — die
//! `#[tauri::command]`-Wrapper in `crate::commands` bleiben dünn, analog zu
//! `crate::groups`/`crate::server_credentials` (Spec 0008).

use chrono::Utc;
use uuid::Uuid;

use persistence_sqlite::{PolicyStoreError, SqlitePolicyStore};
use ssh_manager_core::filter::{FilterEngine, PatternError, RuleId, Scope};

use crate::dto::{EvalContextInput, EvaluationTraceDto, RuleDto, RuleInput};

/// Spec 0077, 3.1.3: Fehler beim Schreiben einer Filterregel.
///
/// Eigener Typ statt [`PolicyStoreError`], weil das Anlegen und Ändern
/// jetzt aus zwei Gründen scheitern kann und das Frontend sie
/// unterscheiden muss: ein Muster, das sich nicht übersetzen lässt (trägt
/// den stabilen Code `FILTER_RULE_PATTERN_INVALID`), und ein Fehler des
/// Speichers (trägt wie bisher keinen Code). `PolicyStoreError` selbst
/// kann keinen Code tragen.
///
/// Die Umwandlung nach `CommandError` macht
/// [`crate::error::rule_write_error`] — ausdrücklich, nie per `?`, sonst
/// ginge der Code still verloren (Begründung dort).
/// `Display`/`Error` und die beiden `From`-Impls sind von Hand
/// geschrieben statt über `thiserror`: `app-shell` hängt nicht von
/// `thiserror` ab, und diese Spec erlaubt keine neue Abhängigkeit dafür.
#[derive(Debug)]
pub enum RuleWriteError {
    InvalidPattern(PatternError),
    Store(PolicyStoreError),
}

impl std::fmt::Display for RuleWriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RuleWriteError::InvalidPattern(err) => write!(f, "{err}"),
            RuleWriteError::Store(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for RuleWriteError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            RuleWriteError::InvalidPattern(err) => Some(err),
            RuleWriteError::Store(err) => Some(err),
        }
    }
}

impl From<PatternError> for RuleWriteError {
    fn from(err: PatternError) -> Self {
        RuleWriteError::InvalidPattern(err)
    }
}

impl From<PolicyStoreError> for RuleWriteError {
    fn from(err: PolicyStoreError) -> Self {
        RuleWriteError::Store(err)
    }
}

/// Neue Regel aus `input` anlegen — generiert eine frische [`RuleId`] (Spec
/// 0009, Abschnitt 3: `create_rule(input: RuleInput) -> RuleId`) und setzt
/// `created_at`/`updated_at` auf denselben Zeitpunkt, analog zu
/// `AiProviderConfigInput::into_new_config`.
///
/// Spec 0077, 3.1.2: Das Muster wird geprüft, **bevor** der Speicher etwas
/// schreibt — bei einem Fehler wird nichts angelegt. Geprüft wird bewusst
/// `stored.pattern`, also genau das Muster, das sonst gespeichert würde,
/// und nicht eine zweite, aus `input` neu gebaute Fassung: So kann das
/// Geprüfte nicht vom Gespeicherten abweichen.
pub async fn create_rule(
    policy_store: &SqlitePolicyStore,
    input: RuleInput,
) -> Result<RuleId, RuleWriteError> {
    let id = RuleId(Uuid::new_v4().to_string());
    let now = Utc::now();
    let stored = input.into_stored_rule(id.clone(), now, now);
    stored.pattern.validate()?;
    policy_store.create(&stored).await?;
    Ok(id)
}

/// Aktualisiert Regel `id` mit den Feldern aus `input` — `created_at` bleibt
/// erhalten (`SqlitePolicyStore::update` rührt es nicht an), nur
/// `updated_at` wird neu gesetzt.
///
/// Spec 0077, 3.1.2: Auch hier wird vor dem Schreiben geprüft; scheitert
/// die Prüfung, behält die gespeicherte Regel ihr **altes** Muster. Das
/// `get` davor ist ein reiner Lesezugriff.
pub async fn update_rule(
    policy_store: &SqlitePolicyStore,
    id: RuleId,
    input: RuleInput,
) -> Result<(), RuleWriteError> {
    let existing = policy_store.get(&id).await?;
    let stored = input.into_stored_rule(id, existing.created_at, Utc::now());
    stored.pattern.validate()?;
    policy_store.update(&stored).await?;
    Ok(())
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
    use ssh_manager_core::filter::{Pattern, RuleAction};

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

    // --- Spec 0077, Schicht 1: ungültiges Muster wird nicht gespeichert ---

    fn rule_input(pattern_type: PatternType, pattern_value: &str, action: RuleAction) -> RuleInput {
        RuleInput {
            pattern_type,
            pattern_value: pattern_value.to_string(),
            action,
            scope: Scope::Global,
            priority: 0,
        }
    }

    fn assert_invalid_pattern(err: RuleWriteError) -> String {
        match err {
            RuleWriteError::InvalidPattern(pattern_err) => pattern_err.to_string(),
            RuleWriteError::Store(store_err) => {
                panic!("InvalidPattern erwartet, bekommen: Store({store_err})")
            }
        }
    }

    /// Spec 0077, T-1: Ein Regex, der sich nicht übersetzen lässt, wird
    /// beim Anlegen abgewiesen — und es liegt danach **nichts** im
    /// Speicher.
    ///
    /// *Scheitert gegen den Stand vor diesem Fix* (dort wurde die Regel
    /// gespeichert). Gegenbeweis geführt.
    #[tokio::test]
    async fn test_spec_0077_t1_create_rule_rejects_an_invalid_regex_and_stores_nothing() {
        let (_dir, store) = in_memory_store().await;

        let err = create_rule(
            &store,
            rule_input(PatternType::Regex, "^systemctl stop (.*", RuleAction::Deny),
        )
        .await
        .expect_err("ein Regex, der nicht übersetzt, darf nicht gespeichert werden");

        let message = assert_invalid_pattern(err);
        assert!(
            message.contains("unclosed group"),
            "der Fehlertext der Bibliothek soll durchgereicht werden: {message}"
        );
        assert!(
            store.list_all().await.unwrap().is_empty(),
            "nach einem abgewiesenen Muster darf keine Regel im Speicher liegen"
        );
    }

    /// Spec 0077, T-2: dasselbe mit einem Glob, der nicht übersetzt.
    #[tokio::test]
    async fn test_spec_0077_t2_create_rule_rejects_an_invalid_glob_and_stores_nothing() {
        let (_dir, store) = in_memory_store().await;

        let err = create_rule(
            &store,
            rule_input(PatternType::Glob, "systemctl [stop", RuleAction::Deny),
        )
        .await
        .expect_err("ein Glob, der nicht übersetzt, darf nicht gespeichert werden");

        let message = assert_invalid_pattern(err);
        assert!(
            message.contains("unclosed character class"),
            "der Fehlertext der Bibliothek soll durchgereicht werden: {message}"
        );
        assert!(store.list_all().await.unwrap().is_empty());
    }

    /// Spec 0077, T-3: Wird eine gültige Regel auf ein ungültiges Muster
    /// geändert, scheitert das Ändern — und die gespeicherte Regel trägt
    /// danach noch ihr **altes** Muster.
    ///
    /// Scheitert, wenn erst geschrieben und dann geprüft wird.
    #[tokio::test]
    async fn test_spec_0077_t3_update_rule_keeps_the_old_pattern_when_the_new_one_is_invalid() {
        let (_dir, store) = in_memory_store().await;
        let id = create_rule(&store, allow_ls_input(Scope::Global))
            .await
            .unwrap();

        let err = update_rule(
            &store,
            id.clone(),
            rule_input(PatternType::Regex, "^ls (.*", RuleAction::Allow),
        )
        .await
        .expect_err("ein ungültiges Muster darf nicht übernommen werden");
        assert_invalid_pattern(err);

        let fetched = store.get(&id).await.unwrap();
        assert_eq!(
            fetched.pattern.display_text(),
            "ls *",
            "die gespeicherte Regel muss ihr altes Muster behalten"
        );
    }

    /// Spec 0077, T-4 (app-shell-Teil): Auch ein pfadförmiger Glob, der
    /// **nur im strengen Zweig** scheitert, wird abgewiesen — Schicht 1
    /// prüft genau die Zweige, die die Auswertung übersetzt (3.1.1).
    ///
    /// Gemessen (§1, Tabelle „Gemessen, zweiter Befund"): `Glob::new`
    /// übersetzt `rm /x/[a/b]/../c`, der strenge Zweig scheitert an
    /// `rm /x/[a/c`. Scheitert, wenn `validate` nur `Glob::new` prüft.
    #[tokio::test]
    async fn test_spec_0077_t4_create_rule_rejects_a_glob_that_only_fails_in_the_strict_branch() {
        let (_dir, store) = in_memory_store().await;

        let err = create_rule(
            &store,
            rule_input(PatternType::Glob, "rm /x/[a/b]/../c", RuleAction::Deny),
        )
        .await
        .expect_err("ein im strengen Zweig ungültiges Muster darf nicht gespeichert werden");

        assert_invalid_pattern(err);
        assert!(store.list_all().await.unwrap().is_empty());
    }

    /// Spec 0077, T-5 (app-shell-Teil): Gültige Muster aller drei Typen
    /// werden weiter angenommen — fängt eine zu strenge Prüfung ab, die
    /// das Anlegen ganz blockieren würde.
    #[tokio::test]
    async fn test_spec_0077_t5_create_rule_still_accepts_valid_patterns() {
        let (_dir, store) = in_memory_store().await;

        for (pattern_type, pattern_value) in [
            (PatternType::Glob, "ls *"),
            (PatternType::Glob, "cat /var/log/**"),
            (PatternType::Glob, "ls [abc]*"),
            (PatternType::Glob, "curl http://example.com/*"),
            (PatternType::Regex, r"^(?:ls|cat)\s+(\S+)$"),
            (PatternType::Regex, "^echo [äöü]+$"),
            (PatternType::Exact, "ls -la"),
            // `Exact` wird nie übersetzt und ist deshalb immer gültig,
            // auch mit Zeichen, die als Glob/Regex kaputt wären.
            (PatternType::Exact, "rm [x"),
        ] {
            create_rule(
                &store,
                rule_input(pattern_type, pattern_value, RuleAction::Allow),
            )
            .await
            .unwrap_or_else(|e| panic!("{pattern_value:?} sollte gültig sein: {e}"));
        }

        assert_eq!(store.list_all().await.unwrap().len(), 8);
    }

    /// Spec 0077, T-6e (3.2.3): `list_rules` belegt `pattern_error` für
    /// eine Regel mit ungültigem Muster und lässt es bei einer gültigen
    /// auf `None`.
    ///
    /// Die ungültige Regel kommt über `SqlitePolicyStore::create` direkt
    /// in den Speicher, also an Schicht 1 vorbei — so entsteht genau die
    /// Zeile, die eine ältere Programmfassung hinterlassen haben kann.
    #[tokio::test]
    async fn test_spec_0077_t6e_list_rules_marks_a_rule_whose_pattern_does_not_compile() {
        let (_dir, store) = in_memory_store().await;
        let valid_id = create_rule(&store, allow_ls_input(Scope::Global))
            .await
            .unwrap();
        let invalid_id = insert_rule_bypassing_layer_one(
            &store,
            "legacy-broken",
            Pattern::Regex("^systemctl stop (.*".to_string()),
            RuleAction::Deny,
        )
        .await;

        let listed = list_rules(&store, None).await.unwrap();
        let valid = listed.iter().find(|r| r.id == valid_id).unwrap();
        let invalid = listed.iter().find(|r| r.id == invalid_id).unwrap();

        assert_eq!(valid.pattern_error, None);
        let message = invalid
            .pattern_error
            .as_ref()
            .expect("die ungültige Regel muss markiert sein");
        assert!(
            message.contains("unclosed group"),
            "der Fehlertext gehört als Detail in die Liste: {message}"
        );
    }

    /// Legt eine Regel **unter Umgehung von Schicht 1** an, über den
    /// Speicher selbst (Spec 0077, T-6c/T-6e).
    ///
    /// Bildet den Fall „Zeile stammt aus einer älteren Programmfassung"
    /// ab: `SqlitePolicyStore::create` prüft das Muster nicht, und
    /// `rules_for`/`pattern_from_db` laden es später unverändert zurück.
    /// Bewusst über die echte Speicher-API statt über rohes SQL — dann
    /// entsteht die Zeile in genau dem Format, das eine echte
    /// Installation geschrieben hätte, und `app-shell` braucht keine
    /// zusätzliche Abhängigkeit auf `sqlx`.
    async fn insert_rule_bypassing_layer_one(
        store: &SqlitePolicyStore,
        id: &str,
        pattern: Pattern,
        action: RuleAction,
    ) -> RuleId {
        let now = Utc::now();
        let id = RuleId(id.to_string());
        let stored = persistence_sqlite::StoredRule {
            id: id.clone(),
            pattern,
            action,
            scope: Scope::Global,
            priority: 100,
            created_at: now,
            updated_at: now,
        };
        store.create(&stored).await.unwrap();
        id
    }

    /// Spec 0077, T-6c: Der Akzeptanzfall „ältere Datenbank" — eine Zeile
    /// mit ungültigem Regex als Deny, die nie durch Schicht 1 lief.
    ///
    /// Geprüft wird gegen die **echte** [`FilterEngine`] über den echten
    /// `SqlitePolicyStore`: Die Entscheidung ist dieselbe wie ohne diese
    /// Regel (AutoExec durch die Allow-Regel daneben), die Auswertung
    /// bricht nicht ab, die übrigen Regeln greifen weiter, es gibt ein
    /// ERROR-Ereignis mit der Regel-ID, und `list_rules` markiert die
    /// Zeile.
    #[tokio::test]
    async fn test_spec_0077_t6c_rule_from_an_older_database_is_inert_but_visible() {
        crate::test_support::log_capture::start_recording();
        let (_dir, store) = in_memory_store().await;

        create_rule(
            &store,
            rule_input(PatternType::Glob, "systemctl *", RuleAction::Allow),
        )
        .await
        .unwrap();
        let broken_id = insert_rule_bypassing_layer_one(
            &store,
            "legacy-deny-broken",
            Pattern::Regex("^systemctl stop (.*".to_string()),
            RuleAction::Deny,
        )
        .await;

        // Die Zeile wird weiter geladen, nicht beim Lesen verworfen.
        assert!(
            store
                .list_all()
                .await
                .unwrap()
                .iter()
                .any(|r| r.id == broken_id),
            "`list_all` muss die Zeile weiterhin liefern"
        );

        let engine = FilterEngine::new(store.clone());
        let decision = engine
            .evaluate(
                "systemctl stop nginx",
                &ssh_manager_core::filter::EvalContext {
                    server_id: ssh_manager_core::shared::ServerId::new(),
                    tags: Vec::new(),
                },
            )
            .await;

        assert!(
            matches!(decision, ssh_manager_core::filter::Decision::AutoExec),
            "dieselbe Entscheidung wie ohne die kaputte Regel erwartet, bekommen: {decision:?}"
        );

        let error_lines = crate::test_support::log_capture::recorded_error_lines();
        assert!(
            error_lines
                .iter()
                .any(|line| line.contains("legacy-deny-broken")),
            "ERROR-Ereignis mit der Regel-ID erwartet, aufgezeichnet: {error_lines:?}"
        );

        let listed = list_rules(&store, None).await.unwrap();
        let broken = listed.iter().find(|r| r.id == broken_id).unwrap();
        assert!(
            broken.pattern_error.is_some(),
            "die Zeile muss in der Liste markiert sein"
        );
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
