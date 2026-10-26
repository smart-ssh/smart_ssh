//! Test-only setters of `Session` (gated by `any(test, feature = "test-support")`).

use super::*;

impl Session {
    // Spec 0086, A3.3 / ADR 0080 (decision 4): Test-Setter, je einer pro
    // Feld, das Tests tatsächlich überschreiben. Alle hinter dem Feature
    // `test-support` wie [`Session::set_sftp_for_tests`]: Produktivbauten
    // aktivieren es nicht, also gibt es für Produktivcode keinen Weg, ein
    // Feld einer bestehenden Sitzung zu überschreiben (`cargo build
    // --workspace` lehnt einen Aufruf ab; `clippy --all-targets`/`cargo
    // test` haben das Feature an). Jeder Setter ändert genau ein Feld —
    // `set_filter_engine_for_tests`, `set_ai_provider_for_tests` und
    // `set_sudo_password_for_tests` sind die sicherheitsrelevanten.

    pub fn set_chat_content_for_tests(
        &mut self,
        value: Arc<crate::chat_content_extension::ChatContentRegistry>,
    ) {
        self.parts.chat_content = value;
    }

    pub fn set_outgoing_transform_for_tests(
        &mut self,
        value: Option<Arc<dyn ssh_manager_core::outgoing::OutgoingTransform>>,
    ) {
        self.parts.outgoing_transform = value;
    }

    pub fn set_filter_engine_for_tests(&mut self, value: Box<dyn CommandEvaluator>) {
        self.parts.filter_engine = value;
    }

    pub fn set_model_context_window_tokens_for_tests(&mut self, value: usize) {
        self.parts.model_context_window_tokens = value;
    }

    pub fn set_ai_provider_for_tests(&mut self, value: Box<dyn AiProvider>) {
        self.parts.ai_provider = value;
    }

    pub fn set_post_ingest_policy_for_tests(
        &mut self,
        value: ssh_manager_core::profiles::PostIngestPolicy,
    ) {
        self.parts.post_ingest_policy = value;
    }

    pub fn set_sudo_password_for_tests(&mut self, value: Option<secrecy::SecretString>) {
        self.parts.sudo_password = SwappableSudoPassword::new(value);
    }

    pub fn set_injection_check_provider_for_tests(&mut self, value: Option<Box<dyn AiProvider>>) {
        self.parts.injection_check_provider = value;
    }

    pub fn set_injection_check_budget_for_tests(
        &mut self,
        value: Option<Arc<ai_providers::ProviderBudgetGuard>>,
    ) {
        self.parts.injection_check_budget = value;
    }

    pub fn set_server_id_for_tests(&mut self, value: ServerId) {
        self.parts.server_id = value;
    }

    pub fn set_red_risk_always_confirm_for_tests(&mut self, value: bool) {
        self.parts.red_risk_always_confirm = value;
    }

    pub fn set_chat_session_id_for_tests(&mut self, value: AsyncMutex<Option<uuid::Uuid>>) {
        self.parts.chat_session_id = value;
    }

    pub fn set_system_context_parts_for_tests(
        &mut self,
        value: AsyncMutex<crate::compaction::SystemContextParts>,
    ) {
        self.parts.system_context_parts = value;
    }

    pub fn set_risk_second_opinion_provider_for_tests(
        &mut self,
        value: Option<Box<dyn AiProvider>>,
    ) {
        self.parts.risk_second_opinion_provider = value;
    }

    pub fn set_risk_second_opinion_budget_for_tests(
        &mut self,
        value: Option<Arc<ai_providers::ProviderBudgetGuard>>,
    ) {
        self.parts.risk_second_opinion_budget = value;
    }

    pub fn set_chat_session_store_for_tests(
        &mut self,
        value: Option<persistence_sqlite::SqliteChatSessionStore>,
    ) {
        self.parts.chat_session_store = value;
    }

    pub fn set_ledger_store_for_tests(
        &mut self,
        value: Option<persistence_sqlite::SqliteLedgerStore>,
    ) {
        self.parts.ledger_store = value;
    }

    pub fn set_transport_for_tests(&mut self, value: SessionTransport) {
        self.parts.transport = value;
    }

    pub fn set_ai_provider_budget_for_tests(
        &mut self,
        value: Arc<ai_providers::ProviderBudgetGuard>,
    ) {
        self.parts.ai_provider_budget = value;
    }

    /// Spec 0085, A3.3: Setzt einen Test-Kanal ein. Hinter dem Feature
    /// `test-support` (Spec 0084, A5) statt hinter reinem `#[cfg(test)]`,
    /// weil `app-shell`s Tests ihn ebenfalls brauchen; Produktivbauten
    /// aktivieren das Feature nicht.
    pub async fn set_sftp_for_tests(&self, sftp: Box<dyn SftpSession>) {
        *self.sftp.inner.lock().await = Some(sftp);
    }
}
