//! DTOs: ai provider.

use chrono::Utc;
use serde::{Deserialize, Serialize};

use persistence_sqlite::{AiProviderConfig, AiProviderConfigUpdate};
use ssh_manager_core::ai::{ProviderId, ProviderType};
use ssh_manager_core::profiles::{trim_credential_value, CredentialRef};

/// Spec 0007, Abschnitt 8.2 — **bewusst KEIN `api_key`-Feld**, der Key geht
/// nie zurück ans Frontend. `credential_ref` ist zwar selbst kein Secret,
/// bleibt aber ebenfalls draußen: reines Backend-Implementierungsdetail,
/// das das Frontend für nichts in Teil 1 braucht.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
pub struct AiProviderConfigDto {
    pub id: ProviderId,
    pub provider_type: ProviderType,
    pub display_name: String,
    pub base_url: Option<String>,
    pub model: String,
    pub supports_native_tool_calling: bool,
    pub is_active: bool,
    /// Spec 0025, Abschnitt 3.
    pub extra_headers: Vec<(String, String)>,
    /// Spec 0025, Abschnitt 4.
    pub attestation_url: Option<String>,
    /// Spec 0065, Teil 4: `None` = „Automatisch" (Default) im Formular.
    pub max_tokens_override: Option<u32>,
    /// Issue #162: serverseitige Web-Recherche (nur Provider, die sie
    /// unterstützen).
    pub web_research_enabled: bool,
}

impl From<&AiProviderConfig> for AiProviderConfigDto {
    fn from(config: &AiProviderConfig) -> Self {
        Self {
            id: config.id,
            provider_type: config.provider_type,
            display_name: config.display_name.clone(),
            base_url: config.base_url.clone(),
            model: config.model.clone(),
            supports_native_tool_calling: config.supports_native_tool_calling,
            is_active: config.is_active,
            extra_headers: config.extra_headers.clone(),
            attestation_url: config.attestation_url.clone(),
            max_tokens_override: config.max_tokens_override,
            web_research_enabled: config.web_research_enabled,
        }
    }
}

/// Spec 0007, Abschnitt 8.2. `api_key` wird nie persistiert, nur an den
/// `CredentialStore` weitergereicht (`add_ai_provider`) bzw. bei
/// `update_ai_provider` interpretiert: leer = Credential unverändert
/// lassen (s. `app_shell::commands::update_ai_provider`-Doc-Kommentar).
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
pub struct AiProviderConfigInput {
    pub provider_type: ProviderType,
    pub display_name: String,
    pub base_url: Option<String>,
    pub model: String,
    pub supports_native_tool_calling: bool,
    pub api_key: String,
    /// Spec 0025, Abschnitt 3.
    pub extra_headers: Vec<(String, String)>,
    /// Spec 0025, Abschnitt 4.
    pub attestation_url: Option<String>,
    /// Spec 0065, Teil 4: `None` = „Automatisch" — validiert in
    /// [`Self::validate_max_tokens_override`] (positiv, sinnvolle
    /// Obergrenze), NICHT hier im reinen Datentyp (derselbe Grund wie bei
    /// `trimmed()`: eine
    /// Deserialize-Quelle kennt keine fachliche Validierung).
    pub max_tokens_override: Option<u32>,
    /// Issue #162: fehlt das Feld (älteres Frontend), gilt der Default
    /// „an".
    #[serde(default = "default_web_research_enabled")]
    pub web_research_enabled: bool,
}

fn default_web_research_enabled() -> bool {
    true
}

/// Spec 0065, Teil 4: sinnvolle Obergrenze für den Override — großzügig
/// über dem höchsten heute bekannten Modell-Maximum (128K, s.
/// `ai_providers::anthropic_model_max_output_tokens`/`openai_compatible_
/// model_max_output_tokens`), damit ein zukünftig größeres Modell nicht
/// sofort an dieser UI-Grenze scheitert, aber klein genug, um einen
/// offensichtlichen Tippfehler (z. B. eine zusätzliche Null) abzufangen.
pub const MAX_TOKENS_OVERRIDE_UPPER_BOUND: u32 = 1_000_000;

impl AiProviderConfigInput {
    /// Spec 0049, Fund 1: rand-trimmt `api_key` und die Endpunkt-Felder
    /// (`base_url`, `attestation_url`) — führende/nachfolgende Whitespaces
    /// inkl. `\r`/`\n`/Tabs, wie sie ein Copy-Paste unter Windows an einen
    /// eingefügten API-Key oder eine eingefügte URL anhängt. Zentrale
    /// Stelle statt UI-verstreutem Trimmen: jeder Aufrufer von
    /// `add_ai_provider`/`update_ai_provider`/`discover_models` ruft dies
    /// als Erstes auf `config` auf, bevor der Wert irgendwo verwendet
    /// wird — greift damit unabhängig vom Eingabeweg (Paste, Tippen,
    /// später Import). Nur der Rand wird angefasst, der Inhalt (auch ein
    /// `Some("")` nach dem Trimmen) bleibt unverändert — dieselbe "leer ==
    /// unverändert lassen"-Semantik wie vor dem Trimmen gilt unangetastet
    /// weiter, das ist nicht Teil dieses Funds.
    ///
    /// Spec 0073, A3: benutzt statt `str::trim` den geteilten
    /// [`trim_credential_value`] aus `core` — dieselbe Semantik wie
    /// bisher, zusätzlich fallen unsichtbare Randzeichen ohne
    /// `White_Space`-Eigenschaft weg (BOM, Zero-Width-Space und die
    /// weiteren aus `INVISIBLE_CREDENTIAL_EDGE_CHARS`), wie sie beim
    /// Kopieren aus einer Datei mit BOM oder von einer Webseite an einem
    /// Key hängen. Die Spec nennt genau diese drei Felder; `display_name`
    /// ist ein Anzeigetext und bleibt unangetastet (T12).
    ///
    /// `model` bleibt hier ebenfalls unangetastet — das ist **kein**
    /// Ergebnis dieser Spec, sondern der Stand vor ihr. Ob ein Modellname
    /// mit Randzeichen getrimmt gehört (er geht in jeden Provider-Request
    /// und in `provider_identity_key`, ein Randzeichen ergibt also ein
    /// `AI_MODEL_NOT_FOUND` ohne sichtbaren Grund), ist eine offene
    /// Produktfrage, s. ADR 0064.
    pub fn trimmed(mut self) -> Self {
        self.api_key = trim_credential_value(&self.api_key);
        self.base_url = self.base_url.map(|v| trim_credential_value(&v));
        self.attestation_url = self.attestation_url.map(|v| trim_credential_value(&v));
        self
    }

    /// Spec 0065, Teil 4: „positive Zahl, sinnvolle Obergrenze; leer =
    /// automatisch" — `0` ist ausdrücklich UNGÜLTIG (die Anthropic-API
    /// verlangt `max_tokens >= 1`, ein `Some(0)` wäre außerdem nicht von
    /// „kein Override" unterscheidbar gewesen, hätte diese Funktion `0`
    /// stillschweigend akzeptiert). Aufrufer: `commands::add_ai_provider`/
    /// `update_ai_provider`, VOR dem Erreichen von `into_new_config`/
    /// `into_update` — ein Store-Layer, der nie ein ungültiges
    /// `max_tokens_override` sieht, statt einer Validierung tief in der
    /// Persistenz.
    pub fn validate_max_tokens_override(&self) -> Result<(), String> {
        match self.max_tokens_override {
            None => Ok(()),
            Some(0) => Err("Max. Antwortlänge muss größer als 0 sein".to_string()),
            Some(value) if value > MAX_TOKENS_OVERRIDE_UPPER_BOUND => Err(format!(
                "Max. Antwortlänge darf {MAX_TOKENS_OVERRIDE_UPPER_BOUND} Tokens nicht überschreiten"
            )),
            Some(_) => Ok(()),
        }
    }

    /// Baut die volle [`AiProviderConfig`] für [`persistence_sqlite::SqliteAiProviderStore::create`]
    /// — `id`/`credential_ref` werden hier frisch vergeben (ein Aufruf pro
    /// `add_ai_provider`, s. Spec Abschnitt 8.2: "Backend generiert eine
    /// neue `ProviderId`, erzeugt daraus einen `CredentialRef`").
    pub fn into_new_config(self, id: ProviderId) -> AiProviderConfig {
        let now = Utc::now();
        AiProviderConfig {
            id,
            provider_type: self.provider_type,
            display_name: self.display_name,
            base_url: self.base_url,
            model: self.model,
            supports_native_tool_calling: self.supports_native_tool_calling,
            credential_ref: credential_ref_for(id),
            is_active: false,
            extra_headers: self.extra_headers,
            attestation_url: self.attestation_url,
            max_tokens_override: self.max_tokens_override,
            web_research_enabled: self.web_research_enabled,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn into_update(self, id: ProviderId) -> AiProviderConfigUpdate {
        AiProviderConfigUpdate {
            id,
            provider_type: self.provider_type,
            display_name: self.display_name,
            base_url: self.base_url,
            model: self.model,
            supports_native_tool_calling: self.supports_native_tool_calling,
            extra_headers: self.extra_headers,
            attestation_url: self.attestation_url,
            max_tokens_override: self.max_tokens_override,
            web_research_enabled: self.web_research_enabled,
            updated_at: Utc::now(),
        }
    }
}

/// `CredentialRef`-Schema exakt wie in Spec 0007 Abschnitt 8.2 vorgegeben:
/// `"ai-provider:{id}"`.
pub fn credential_ref_for(id: ProviderId) -> CredentialRef {
    CredentialRef::new(format!("ai-provider:{}", id.0))
}
