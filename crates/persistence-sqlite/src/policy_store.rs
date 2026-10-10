//! SQLite-gestützte Verwaltung von `filter_rules` (Spec 0009, Abschnitt 2)
//! und Implementierung des `PolicyStore`-Traits aus `core::filter` (Spec
//! 0002 Abschnitt 5, seit Spec 0009 async — s.
//! `docs/adr/0002-sudo-dual-text-matching.md` für den Matching-Kontext und
//! den Commit "refactor(core): make PolicyStore trait async" für den
//! Trait-Umbau selbst).
//!
//! **Eigener Store statt Erweiterung von [`crate::SqliteProfileStore`]**,
//! aus demselben Grund wie [`crate::SqliteAiProviderStore`] (s. dortiger
//! Modul-Kommentar): Filter-Regeln sind fachlich unabhängig von
//! Server-/Gruppen-Profilen, teilen sich aber denselben `SqlitePool`
//! (s. [`SqlitePolicyStore::new`] und `crate::store::SqliteProfileStore::policy_store`).
//!
//! **`StoredRule` vs. `core::filter::Rule`**: `Rule` (Spec 0002) kennt keine
//! Zeitstempel — die braucht nur die Persistenz (Anzeige/Audit, analog zu
//! `AiProviderConfig`), nicht die Auswertungslogik in `core`. `StoredRule`
//! trägt deshalb zusätzlich `created_at`/`updated_at`; [`StoredRule::into_rule`]
//! reduziert auf die für `PolicyStore::rules_for` nötigen Felder.

use chrono::{DateTime, Utc};
use sqlx::sqlite::SqlitePool;
use sqlx::Row;

use async_trait::async_trait;
use uuid::Uuid;

use ssh_manager_core::filter::{
    scope_applies, EffectiveScope, Pattern, PatternError, PolicySource, PolicySourceError,
    PolicySourceResult, PolicyStore, Rule, RuleAction, RuleId, RuleOrigin, Scope,
};
use ssh_manager_core::shared::ServerId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyStoreError {
    NotFound(RuleId),
    /// Das Muster der Regel lässt sich nicht übersetzen; `create`/`update`
    /// haben nichts geschrieben (Spec 0077, 3.1.2, Schicht 2).
    InvalidPattern(PatternError),
    Backend(String),
}

impl std::fmt::Display for PolicyStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PolicyStoreError::NotFound(id) => write!(f, "Filter-Regel '{id}' nicht gefunden"),
            PolicyStoreError::InvalidPattern(err) => write!(f, "{err}"),
            PolicyStoreError::Backend(msg) => write!(f, "Datenbankfehler: {msg}"),
        }
    }
}

impl std::error::Error for PolicyStoreError {}

fn backend_err(e: sqlx::Error) -> PolicyStoreError {
    PolicyStoreError::Backend(e.to_string())
}

fn parse_timestamp(raw: &str) -> Result<DateTime<Utc>, PolicyStoreError> {
    DateTime::parse_from_rfc3339(raw)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| PolicyStoreError::Backend(format!("ungültiger Zeitstempel '{raw}': {e}")))
}

fn pattern_to_db(pattern: &Pattern) -> (&'static str, &str) {
    match pattern {
        Pattern::Glob(v) => ("glob", v.as_str()),
        Pattern::Regex(v) => ("regex", v.as_str()),
        Pattern::Exact(v) => ("exact", v.as_str()),
    }
}

fn pattern_from_db(pattern_type: &str, pattern_value: String) -> Result<Pattern, PolicyStoreError> {
    match pattern_type {
        "glob" => Ok(Pattern::Glob(pattern_value)),
        "regex" => Ok(Pattern::Regex(pattern_value)),
        "exact" => Ok(Pattern::Exact(pattern_value)),
        other => Err(PolicyStoreError::Backend(format!(
            "unbekannter pattern_type '{other}' in der Datenbank"
        ))),
    }
}

fn action_to_db(action: &RuleAction) -> &'static str {
    match action {
        RuleAction::Allow => "allow",
        RuleAction::Confirm => "confirm",
        RuleAction::Deny => "deny",
    }
}

fn action_from_db(raw: &str) -> Result<RuleAction, PolicyStoreError> {
    match raw {
        "allow" => Ok(RuleAction::Allow),
        "confirm" => Ok(RuleAction::Confirm),
        "deny" => Ok(RuleAction::Deny),
        other => Err(PolicyStoreError::Backend(format!(
            "unbekannte action '{other}' in der Datenbank"
        ))),
    }
}

fn scope_to_db(scope: &Scope) -> (&'static str, Option<String>) {
    match scope {
        Scope::Global => ("global", None),
        Scope::Server(id) => ("server", Some(id.0.to_string())),
        Scope::Tag(tag) => ("tag", Some(tag.clone())),
    }
}

fn scope_from_db(scope_type: &str, scope_value: Option<String>) -> Result<Scope, PolicyStoreError> {
    match scope_type {
        "global" => Ok(Scope::Global),
        "server" => {
            let raw = scope_value.ok_or_else(|| {
                PolicyStoreError::Backend("scope_type='server' ohne scope_value".to_string())
            })?;
            let uuid = Uuid::parse_str(&raw).map_err(|e| {
                PolicyStoreError::Backend(format!("ungültige Server-UUID in scope_value: {e}"))
            })?;
            Ok(Scope::Server(ServerId(uuid)))
        }
        "tag" => {
            let raw = scope_value.ok_or_else(|| {
                PolicyStoreError::Backend("scope_type='tag' ohne scope_value".to_string())
            })?;
            Ok(Scope::Tag(raw))
        }
        other => Err(PolicyStoreError::Backend(format!(
            "unbekannter scope_type '{other}' in der Datenbank"
        ))),
    }
}

/// Vollständige gespeicherte Regel inkl. Zeitstempel — s. Moduldoc.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredRule {
    pub id: RuleId,
    pub pattern: Pattern,
    pub action: RuleAction,
    pub scope: Scope,
    pub priority: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl StoredRule {
    /// Reduziert auf die von `core::filter::Rule` (Auswertungslogik)
    /// benötigten Felder — s. Moduldoc.
    pub fn into_rule(self) -> Rule {
        Rule {
            id: self.id,
            pattern: self.pattern,
            action: self.action,
            scope: self.scope,
            priority: self.priority,
            // Spec 0037, Abschnitt 5: der SQLite-Regelspeicher ist und
            // bleibt die einzige `User`-Quelle.
            origin: RuleOrigin::User,
        }
    }
}

fn row_to_stored_rule(row: &sqlx::sqlite::SqliteRow) -> Result<StoredRule, PolicyStoreError> {
    let id: String = row.get("id");
    let pattern_type: String = row.get("pattern_type");
    let pattern_value: String = row.get("pattern_value");
    let action: String = row.get("action");
    let scope_type: String = row.get("scope_type");
    let scope_value: Option<String> = row.get("scope_value");
    let created_at: String = row.get("created_at");
    let updated_at: String = row.get("updated_at");

    Ok(StoredRule {
        id: RuleId(id),
        pattern: pattern_from_db(&pattern_type, pattern_value)?,
        action: action_from_db(&action)?,
        scope: scope_from_db(&scope_type, scope_value)?,
        priority: row.get("priority"),
        created_at: parse_timestamp(&created_at)?,
        updated_at: parse_timestamp(&updated_at)?,
    })
}

/// `#[derive(Clone)]`: anders als [`crate::SqliteAiProviderStore`] (immer
/// hinter einem `Arc` gehalten) wird `SqlitePolicyStore` in `app-shell`
/// sowohl in `AppState` gehalten als auch bei jedem `connect()` und jedem
/// `evaluate_explained`-Aufruf per Wert in eine neue
/// `FilterEngine<SqlitePolicyStore>` verschoben (`FilterEngine::new`
/// verlangt ein besessenes `S: PolicyStore`, `Session`/Commands halten aber
/// nur eine `&AppState`-Referenz). `SqlitePool` ist intern bereits
/// referenzgezählt (s. `SqliteProfileStore::ai_provider_store`, das
/// `self.pool.clone()` genau dafür nutzt) — Klonen ist daher dieselbe
/// günstige Operation wie überall sonst in dieser Crate, kein zweiter
/// Verbindungsaufbau.
#[derive(Clone)]
pub struct SqlitePolicyStore {
    pool: SqlitePool,
}

impl SqlitePolicyStore {
    /// Teilt sich `pool` mit einem bereits verbundenen [`crate::SqliteProfileStore`]
    /// (dessen `connect()` die Migrationen — inkl. `0003_filter_rules.sql` —
    /// bereits ausgeführt hat), analog zu [`crate::SqliteAiProviderStore::new`].
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Alle gespeicherten Regeln, unabhängig vom Scope — Grundlage sowohl
    /// für [`PolicyStore::rules_for`] (dort nach `EffectiveScope` gefiltert)
    /// als auch für `list_rules` in `app-shell` (dort optional nach einem
    /// exakten `Scope` für die Regel-Verwaltungsansicht gefiltert). Beide
    /// Filterungen laufen bewusst in Rust statt als zwei verschiedene SQL-
    /// `WHERE`-Klauseln — bei der zu erwartenden Regelzahl (typischerweise
    /// wenige bis niedrige Hunderte) unproblematisch, hält die Scope-Logik
    /// aber an einer einzigen Stelle (`core::filter::scope_applies`) statt
    /// sie in SQL zu duplizieren.
    pub async fn list_all(&self) -> Result<Vec<StoredRule>, PolicyStoreError> {
        let rows = sqlx::query(
            "SELECT id, pattern_type, pattern_value, action, scope_type, scope_value, \
             priority, created_at, updated_at FROM filter_rules \
             ORDER BY scope_type, scope_value, priority DESC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(backend_err)?;

        rows.iter().map(row_to_stored_rule).collect()
    }

    pub async fn get(&self, id: &RuleId) -> Result<StoredRule, PolicyStoreError> {
        let row = sqlx::query(
            "SELECT id, pattern_type, pattern_value, action, scope_type, scope_value, \
             priority, created_at, updated_at FROM filter_rules WHERE id = ?",
        )
        .bind(&id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(backend_err)?
        .ok_or_else(|| PolicyStoreError::NotFound(id.clone()))?;

        row_to_stored_rule(&row)
    }

    /// Lehnt eine Regel mit nicht übersetzbarem Muster ab, **bevor** etwas
    /// geschrieben wird (Spec 0077, 3.1.2, Schicht 2) — unabhängig davon, ob
    /// der Aufrufer selbst schon geprüft hat.
    pub async fn create(&self, rule: &StoredRule) -> Result<(), PolicyStoreError> {
        rule.pattern
            .validate()
            .map_err(PolicyStoreError::InvalidPattern)?;
        self.insert_unchecked(rule).await
    }

    /// Schreibt ohne Musterprüfung — **nur für Tests**, die eine Regel aus
    /// einer älteren Datenbank nachstellen (Spec 0077, T-6c).
    #[cfg(any(test, feature = "test-support"))]
    pub async fn create_unchecked_for_tests(
        &self,
        rule: &StoredRule,
    ) -> Result<(), PolicyStoreError> {
        self.insert_unchecked(rule).await
    }

    async fn insert_unchecked(&self, rule: &StoredRule) -> Result<(), PolicyStoreError> {
        let (pattern_type, pattern_value) = pattern_to_db(&rule.pattern);
        let (scope_type, scope_value) = scope_to_db(&rule.scope);

        sqlx::query(
            "INSERT INTO filter_rules \
             (id, pattern_type, pattern_value, action, scope_type, scope_value, priority, \
              created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&rule.id.0)
        .bind(pattern_type)
        .bind(pattern_value)
        .bind(action_to_db(&rule.action))
        .bind(scope_type)
        .bind(scope_value)
        .bind(rule.priority)
        .bind(rule.created_at.to_rfc3339())
        .bind(rule.updated_at.to_rfc3339())
        .execute(&self.pool)
        .await
        .map_err(backend_err)?;
        Ok(())
    }

    /// Aktualisiert alle Felder außer `created_at` (bleibt beim ursprünglichen
    /// Anlage-Zeitpunkt, analog zu `AiProviderConfigUpdate`).
    ///
    /// Prüft das Muster wie [`Self::create`]; bei einem Fehler behält die
    /// gespeicherte Regel ihr altes Muster.
    pub async fn update(&self, rule: &StoredRule) -> Result<(), PolicyStoreError> {
        rule.pattern
            .validate()
            .map_err(PolicyStoreError::InvalidPattern)?;
        let (pattern_type, pattern_value) = pattern_to_db(&rule.pattern);
        let (scope_type, scope_value) = scope_to_db(&rule.scope);

        let result = sqlx::query(
            "UPDATE filter_rules SET pattern_type = ?, pattern_value = ?, action = ?, \
             scope_type = ?, scope_value = ?, priority = ?, updated_at = ? WHERE id = ?",
        )
        .bind(pattern_type)
        .bind(pattern_value)
        .bind(action_to_db(&rule.action))
        .bind(scope_type)
        .bind(scope_value)
        .bind(rule.priority)
        .bind(rule.updated_at.to_rfc3339())
        .bind(&rule.id.0)
        .execute(&self.pool)
        .await
        .map_err(backend_err)?;

        if result.rows_affected() == 0 {
            return Err(PolicyStoreError::NotFound(rule.id.clone()));
        }
        Ok(())
    }

    /// Vertauscht die Prioritäten der Regeln `a` und `b` in **einer**
    /// Transaktion (Spec 0077, 3.2.3): Entweder tragen danach beide die
    /// Priorität der jeweils anderen, oder keine hat sich geändert. Die
    /// Prioritäten werden innerhalb der Transaktion gelesen, nicht vom
    /// Aufrufer übergeben, damit ein dazwischen liegender Schreibzugriff
    /// nicht überschrieben wird.
    pub async fn swap_priorities(
        &self,
        a: &RuleId,
        b: &RuleId,
        updated_at: DateTime<Utc>,
    ) -> Result<(), PolicyStoreError> {
        let mut tx = self.pool.begin().await.map_err(backend_err)?;

        let priority_of = |row: Option<sqlx::sqlite::SqliteRow>, id: &RuleId| match row {
            Some(row) => row.try_get::<i32, _>("priority").map_err(backend_err),
            None => Err(PolicyStoreError::NotFound(id.clone())),
        };
        let row_a = sqlx::query("SELECT priority FROM filter_rules WHERE id = ?")
            .bind(&a.0)
            .fetch_optional(&mut *tx)
            .await
            .map_err(backend_err)?;
        let priority_a = priority_of(row_a, a)?;
        let row_b = sqlx::query("SELECT priority FROM filter_rules WHERE id = ?")
            .bind(&b.0)
            .fetch_optional(&mut *tx)
            .await
            .map_err(backend_err)?;
        let priority_b = priority_of(row_b, b)?;

        for (id, priority) in [(a, priority_b), (b, priority_a)] {
            sqlx::query("UPDATE filter_rules SET priority = ?, updated_at = ? WHERE id = ?")
                .bind(priority)
                .bind(updated_at.to_rfc3339())
                .bind(&id.0)
                .execute(&mut *tx)
                .await
                .map_err(backend_err)?;
        }

        // Wird `tx` vorher durch `?` verworfen, rollt sqlx zurück.
        tx.commit().await.map_err(backend_err)
    }

    pub async fn delete(&self, id: &RuleId) -> Result<(), PolicyStoreError> {
        let result = sqlx::query("DELETE FROM filter_rules WHERE id = ?")
            .bind(&id.0)
            .execute(&self.pool)
            .await
            .map_err(backend_err)?;

        if result.rows_affected() == 0 {
            return Err(PolicyStoreError::NotFound(id.clone()));
        }
        Ok(())
    }
}

#[async_trait]
impl PolicyStore for SqlitePolicyStore {
    /// `PolicyStore::rules_for` (Spec 0002 Abschnitt 5) kennt kein
    /// `Result` — ein Datenbankfehler wird deshalb geloggt und als leere
    /// Regelliste behandelt statt propagiert. Das ist NICHT rundum
    /// fail-safe, auch wenn es das auf den ersten Blick scheint: die
    /// Hard-Blacklist bleibt zwar unverändert aktiv, und eine fehlende
    /// Allow-Regel landet korrekt auf `Confirm` statt `AutoExec` — aber eine
    /// vom Nutzer explizit gesetzte **Deny**-Regel für ein Kommando, das die
    /// Hard-Blacklist selbst nicht abdeckt, geht in diesem Fall ebenso
    /// verloren und degradiert zu einem anklickbaren `Confirm` (unabhängiger
    /// Review-Pass, Spec 0002). `tracing::error!` statt `eprintln!`, damit
    /// dieser sicherheitsrelevante Fall zumindest im strukturierten Log
    /// (Spec 0016) sichtbar ist, statt nur auf stderr zu verschwinden — eine
    /// sichtbare UI-Meldung wäre der nächste Schritt, ist aber (Stand jetzt)
    /// noch nicht verdrahtet.
    async fn rules_for(&self, scope: &EffectiveScope) -> Vec<Rule> {
        match self.list_all().await {
            Ok(stored) => stored
                .into_iter()
                .filter(|rule| scope_applies(&rule.scope, scope))
                .map(StoredRule::into_rule)
                .collect(),
            Err(err) => {
                tracing::error!(
                    error = %err,
                    "Filter-Regeln konnten nicht geladen werden — werte ohne \
                     Nutzerregeln aus (auch Deny-Regeln greifen währenddessen nicht)",
                );
                Vec::new()
            }
        }
    }
}

/// Spec 0037, Abschnitt 5: "Der bestehende SQLite-Regelspeicher ... ist und
/// bleibt die einzige `User`-Quelle" — bindet ihn als [`PolicySource`] ein,
/// damit er über [`ssh_manager_core::filter::CombinedPolicySource`] neben
/// künftigen `Organization`-Quellen (privates Repo, `OrgPolicy`-Modul)
/// verwendbar ist. In der Community Edition ist er die einzige Quelle
/// (s. Spec-Text) — dieselbe `PolicyStore`-Auswertungslogik wie zuvor,
/// nur über den neuen Trait erreichbar.
#[async_trait]
impl PolicySource for SqlitePolicyStore {
    fn origin(&self) -> RuleOrigin {
        RuleOrigin::User
    }

    async fn rules(&self) -> PolicySourceResult<Vec<Rule>> {
        self.list_all()
            .await
            .map(|stored| stored.into_iter().map(StoredRule::into_rule).collect())
            .map_err(|err| PolicySourceError(err.to_string()))
    }

    /// Kein Push-Mechanismus für SQLite-Änderungen in diesem Schritt — der
    /// zurückgegebene `Receiver` liefert einmalig den Stand zum Zeitpunkt
    /// des Aufrufs, aktualisiert sich danach nie von selbst (derselbe
    /// "liefert nur den initialen Wert"-Ansatz wie
    /// `entitlements::FixedEntitlements::watch`, dortiger Kommentar zur
    /// Begründung des `_tx`-Drops). Ein echter Live-Update-Mechanismus
    /// (z. B. Polling oder SQLite-Update-Hooks) ist nicht Teil dieser Spec
    /// — `rules_for`/`rules()` bleiben der maßgebliche, stets aktuelle
    /// Abfrageweg; `watch()` existiert nur, um den `PolicySource`-Trait
    /// vollständig zu implementieren.
    async fn watch(&self) -> PolicySourceResult<tokio::sync::watch::Receiver<Vec<Rule>>> {
        let rules = PolicySource::rules(self).await?;
        let (_tx, rx) = tokio::sync::watch::channel(rules);
        Ok(rx)
    }
}

#[cfg(test)]
mod tests {
    use sqlx::sqlite::SqliteConnectOptions;

    use super::*;
    use crate::SqliteProfileStore;

    async fn in_memory_policy_store() -> SqlitePolicyStore {
        let options = SqliteConnectOptions::new().filename(":memory:");
        SqliteProfileStore::connect_with(options)
            .await
            .expect("In-Memory-Store mit angewendeten Migrationen sollte immer aufbaubar sein")
            .policy_store()
    }

    fn make_rule(id: &str, scope: Scope, priority: i32) -> StoredRule {
        let now = Utc::now();
        StoredRule {
            id: RuleId(id.to_string()),
            pattern: Pattern::Glob("ls *".to_string()),
            action: RuleAction::Allow,
            scope,
            priority,
            created_at: now,
            updated_at: now,
        }
    }

    #[tokio::test]
    async fn test_create_and_list_roundtrip() {
        let store = in_memory_policy_store().await;
        let rule = make_rule("allow-ls", Scope::Global, 5);

        store.create(&rule).await.unwrap();

        let listed = store.list_all().await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, rule.id);
        assert_eq!(listed[0].pattern, Pattern::Glob("ls *".to_string()));
        assert_eq!(listed[0].scope, Scope::Global);
        assert_eq!(listed[0].priority, 5);
    }

    #[tokio::test]
    async fn test_update_changes_fields_but_not_created_at() {
        let store = in_memory_policy_store().await;
        let rule = make_rule("confirm-restart", Scope::Global, 0);
        store.create(&rule).await.unwrap();

        let mut updated = rule.clone();
        updated.action = RuleAction::Deny;
        updated.priority = 42;
        updated.updated_at = Utc::now();
        store.update(&updated).await.unwrap();

        let fetched = store.get(&rule.id).await.unwrap();
        assert_eq!(fetched.action, RuleAction::Deny);
        assert_eq!(fetched.priority, 42);
        assert_eq!(
            fetched.created_at.to_rfc3339(),
            rule.created_at.to_rfc3339(),
            "update darf created_at nicht ändern"
        );
    }

    /// Issue #260: `create` und `update` weisen ein nicht übersetzbares
    /// Muster am Speicher ab und lassen den Bestand unverändert.
    ///
    /// *Scheitert gegen den Stand vor #260* (dort wurde die Regel
    /// gespeichert).
    #[tokio::test]
    async fn test_issue_260_create_and_update_reject_an_invalid_pattern() {
        let store = in_memory_policy_store().await;

        let mut bad = make_rule("bad", Scope::Global, 0);
        bad.pattern = Pattern::Regex("^a(".to_string());
        let err = store.create(&bad).await.unwrap_err();
        assert!(
            matches!(err, PolicyStoreError::InvalidPattern(_)),
            "{err:?}"
        );
        assert!(store.list_all().await.unwrap().is_empty());

        let good = make_rule("good", Scope::Global, 0);
        store.create(&good).await.unwrap();
        let mut changed = good.clone();
        changed.pattern = Pattern::Glob("ls [abc *".to_string());
        let err = store.update(&changed).await.unwrap_err();
        assert!(
            matches!(err, PolicyStoreError::InvalidPattern(_)),
            "{err:?}"
        );
        let stored = store.get(&good.id).await.unwrap();
        assert_eq!(stored.pattern, Pattern::Glob("ls *".to_string()));
    }

    #[tokio::test]
    async fn test_update_on_unknown_id_yields_not_found() {
        let store = in_memory_policy_store().await;
        let rule = make_rule("nie-angelegt", Scope::Global, 0);

        let result = store.update(&rule).await;

        assert_eq!(result, Err(PolicyStoreError::NotFound(rule.id)));
    }

    #[tokio::test]
    async fn test_swap_priorities_exchanges_both_values() {
        let store = in_memory_policy_store().await;
        store
            .create(&make_rule("a", Scope::Global, 10))
            .await
            .unwrap();
        store
            .create(&make_rule("b", Scope::Global, 3))
            .await
            .unwrap();

        store
            .swap_priorities(&RuleId("a".into()), &RuleId("b".into()), Utc::now())
            .await
            .unwrap();

        assert_eq!(store.get(&RuleId("a".into())).await.unwrap().priority, 3);
        assert_eq!(store.get(&RuleId("b".into())).await.unwrap().priority, 10);
    }

    /// Ein Trigger lässt genau den zweiten Schreibzugriff scheitern. Ohne
    /// Transaktion bliebe die erste Änderung stehen und beide Regeln trügen
    /// dieselbe Priorität.
    #[tokio::test]
    async fn test_swap_priorities_changes_nothing_when_the_second_write_fails() {
        let store = in_memory_policy_store().await;
        store
            .create(&make_rule("a", Scope::Global, 10))
            .await
            .unwrap();
        store
            .create(&make_rule("b", Scope::Global, 3))
            .await
            .unwrap();
        sqlx::query(
            "CREATE TRIGGER fail_b BEFORE UPDATE ON filter_rules WHEN OLD.id = 'b' \
             BEGIN SELECT RAISE(ABORT, 'simulated failure'); END",
        )
        .execute(&store.pool)
        .await
        .unwrap();

        let result = store
            .swap_priorities(&RuleId("a".into()), &RuleId("b".into()), Utc::now())
            .await;

        assert!(matches!(result, Err(PolicyStoreError::Backend(_))));
        assert_eq!(store.get(&RuleId("a".into())).await.unwrap().priority, 10);
        assert_eq!(store.get(&RuleId("b".into())).await.unwrap().priority, 3);
    }

    #[tokio::test]
    async fn test_swap_priorities_with_unknown_rule_yields_not_found_and_changes_nothing() {
        let store = in_memory_policy_store().await;
        store
            .create(&make_rule("a", Scope::Global, 10))
            .await
            .unwrap();

        let result = store
            .swap_priorities(&RuleId("a".into()), &RuleId("nope".into()), Utc::now())
            .await;

        assert_eq!(
            result,
            Err(PolicyStoreError::NotFound(RuleId("nope".into())))
        );
        assert_eq!(store.get(&RuleId("a".into())).await.unwrap().priority, 10);
    }

    #[tokio::test]
    async fn test_delete_removes_rule() {
        let store = in_memory_policy_store().await;
        let rule = make_rule("loeschbar", Scope::Global, 0);
        store.create(&rule).await.unwrap();

        store.delete(&rule.id).await.unwrap();

        assert!(store.list_all().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_delete_unknown_id_yields_not_found() {
        let store = in_memory_policy_store().await;

        let result = store.delete(&RuleId("unbekannt".to_string())).await;

        assert_eq!(
            result,
            Err(PolicyStoreError::NotFound(RuleId("unbekannt".to_string())))
        );
    }

    #[tokio::test]
    async fn test_rules_for_filters_by_effective_scope() {
        let store = in_memory_policy_store().await;
        let server_id = ServerId::new();
        store
            .create(&make_rule("global-rule", Scope::Global, 0))
            .await
            .unwrap();
        store
            .create(&make_rule("server-rule", Scope::Server(server_id), 0))
            .await
            .unwrap();
        store
            .create(&make_rule(
                "other-server-rule",
                Scope::Server(ServerId::new()),
                0,
            ))
            .await
            .unwrap();
        store
            .create(&make_rule("tag-rule", Scope::Tag("prod".to_string()), 0))
            .await
            .unwrap();

        let effective = EffectiveScope {
            server_id,
            tags: vec!["prod".to_string()],
        };
        let rules = PolicyStore::rules_for(&store, &effective).await;

        let ids: Vec<String> = rules.iter().map(|r| r.id.0.clone()).collect();
        assert!(ids.contains(&"global-rule".to_string()));
        assert!(ids.contains(&"server-rule".to_string()));
        assert!(ids.contains(&"tag-rule".to_string()));
        assert!(!ids.contains(&"other-server-rule".to_string()));
    }

    /// Spec 0037, Abschnitt 5: "der bestehende SQLite-Regelspeicher ist
    /// und bleibt die einzige `User`-Quelle" — `origin()` und jede über
    /// `PolicySource::rules()` gelieferte `Rule` müssen `RuleOrigin::User`
    /// tragen.
    #[tokio::test]
    async fn test_sqlite_policy_store_is_a_policy_source_with_user_origin() {
        let store = in_memory_policy_store().await;
        store
            .create(&make_rule("allow-ls", Scope::Global, 0))
            .await
            .unwrap();

        assert_eq!(PolicySource::origin(&store), RuleOrigin::User);

        let rules = PolicySource::rules(&store).await.unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].origin, RuleOrigin::User);
    }

    #[tokio::test]
    async fn test_get_unknown_id_yields_not_found() {
        let store = in_memory_policy_store().await;

        let result = store.get(&RuleId("unbekannt".to_string())).await;

        assert_eq!(
            result,
            Err(PolicyStoreError::NotFound(RuleId("unbekannt".to_string())))
        );
    }
}
