//! Tauri-Commands (Spec 0007, Abschnitt 4) — Teil 1: `list_servers` und die
//! AI-Provider-Verwaltung. `connect`/Terminal/Chat-Befehle folgen in
//! späteren Teilen.

use secrecy::SecretString;
use tauri::State;

use ssh_manager_core::ai::ProviderId;

use crate::dto::{credential_ref_for, AiProviderConfigDto, AiProviderConfigInput, ServerDto};
use crate::error::CommandResult;
use crate::state::AppState;

#[tauri::command]
pub async fn list_servers(state: State<'_, AppState>) -> CommandResult<Vec<ServerDto>> {
    let servers = state.profile_store.list_servers().await?;
    Ok(servers.iter().map(ServerDto::from).collect())
}

#[tauri::command]
pub async fn list_ai_providers(
    state: State<'_, AppState>,
) -> CommandResult<Vec<AiProviderConfigDto>> {
    let configs = state.ai_provider_store.list().await?;
    Ok(configs.iter().map(AiProviderConfigDto::from).collect())
}

/// Spec 0007, Abschnitt 8.2: Backend generiert eine neue `ProviderId`,
/// speichert `api_key` zuerst über den `CredentialStore`, danach erst die
/// restlichen Felder in `ai_provider_configs`.
#[tauri::command]
pub async fn add_ai_provider(
    state: State<'_, AppState>,
    config: AiProviderConfigInput,
) -> CommandResult<ProviderId> {
    let id = ProviderId::new();
    let credential_ref = credential_ref_for(id);
    state
        .credential_store
        .set(&credential_ref, SecretString::from(config.api_key.clone()))?;

    let new_config = config.into_new_config(id);
    if let Err(err) = state.ai_provider_store.create(&new_config).await {
        // Best-effort-Aufräumen: ohne diesen Rückbau bliebe bei einem
        // DB-Fehler ein verwaister Credential-Eintrag im Keychain zurück,
        // auf den keine `ai_provider_configs`-Zeile mehr verweist. Ein
        // Fehler beim Aufräumen selbst wird bewusst verschluckt (nicht per
        // `?` weitergereicht) — der eigentliche Fehler (`err`) ist die
        // relevante Information für den Aufrufer, ein sekundärer
        // Keychain-Fehler beim Aufräumversuch soll ihn nicht überdecken.
        let _ = state.credential_store.delete(&credential_ref);
        return Err(err.into());
    }
    Ok(id)
}

/// Spec 0007, Abschnitt 8.2: leeres `api_key`-Feld heißt "Credential
/// unverändert lassen", nicht "löschen". Reihenfolge bewusst umgekehrt zu
/// `add_ai_provider`/`delete_ai_provider`: erst die DB-Metadaten
/// aktualisieren (schlägt sauber mit `NotFound` fehl, falls `id` nicht
/// existiert), erst danach — nur bei nicht-leerem `api_key` — den
/// Credential überschreiben. So wird nie ein Secret für eine `id`
/// geschrieben, die sich als ungültig herausstellt.
#[tauri::command]
pub async fn update_ai_provider(
    state: State<'_, AppState>,
    id: ProviderId,
    config: AiProviderConfigInput,
) -> CommandResult<()> {
    let api_key = config.api_key.clone();
    state
        .ai_provider_store
        .update_fields(&config.into_update(id))
        .await?;

    if !api_key.is_empty() {
        state
            .credential_store
            .set(&credential_ref_for(id), SecretString::from(api_key))?;
    }
    Ok(())
}

/// Spec 0007, Abschnitt 8.2/9: erst `CredentialStore::delete()`, dann die
/// DB-Zeile — aber erst, nachdem geprüft wurde, dass der Provider nicht
/// aktiv ist (Abschnitt 9: Löschen eines aktiven Providers ist verboten).
/// Würde man `is_active` nicht **vor** dem Credential-Löschen prüfen, könnte
/// ein verbotener Löschversuch trotzdem den Credential eines weiterhin
/// aktiven, in der DB unverändert bleibenden Providers entfernen.
#[tauri::command]
pub async fn delete_ai_provider(state: State<'_, AppState>, id: ProviderId) -> CommandResult<()> {
    let existing = state.ai_provider_store.get(&id).await?;
    if existing.is_active {
        return Err(
            persistence_sqlite::AiProviderStoreError::ActiveProviderDeletionForbidden(id).into(),
        );
    }

    state.credential_store.delete(&existing.credential_ref)?;
    state.ai_provider_store.delete(&id).await?;
    Ok(())
}

#[tauri::command]
pub async fn set_active_ai_provider(
    state: State<'_, AppState>,
    id: ProviderId,
) -> CommandResult<()> {
    state
        .ai_provider_store
        .set_active(&id, chrono::Utc::now())
        .await?;
    Ok(())
}
