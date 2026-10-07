//! Tauri-freier Kern der lesenden MCP-Tools `list_servers` und
//! `get_server_notes` (Spec 0028, Issue #35).
//!
//! `app_shell::mcp_backend::AppMcpBackend` hält nur ein `AppHandle` und
//! ließe sich ohne laufende Tauri-App nicht bauen. Die Schritte, an denen
//! Schutzschichten hängen — Allow-Liste (Spec 0028, Abschnitt 6) sowie
//! Redaction → Fencing der Notizen (Spec 0039, ADR 0034, ADR 0103) —
//! liegen deshalb hier und sind gegen die In-Memory-Stores aus
//! `test_support` testbar. `AppMcpBackend` reicht nur noch die Teile aus
//! `AppState` durch.
//!
//! `propose_action`/`ensure_session` bleiben bewusst in `app-shell`: sie
//! brauchen die Sitzungs- und Event-Maschinerie.

use std::collections::HashSet;
use std::sync::Mutex;

use mcp_server::{LookupError, ServerSummary};
use ssh_manager_core::profiles::{CredentialStore, ProfileStore};
use ssh_manager_core::shared::ServerId;

use crate::server_redaction::{redacted_fenced_effective_notes, server_redactor};
use crate::state::AppState;

/// Die Abhängigkeiten der lesenden MCP-Tools, einzeln statt als ganzer
/// `AppState`, damit Tests nur die nötigen Doubles bauen müssen.
pub struct McpLookup<'a> {
    pub allowed_servers: &'a Mutex<HashSet<ServerId>>,
    pub profile_store: &'a dyn ProfileStore,
    pub credential_store: &'a (dyn CredentialStore + Send + Sync),
}

impl<'a> McpLookup<'a> {
    /// Die Sicht des MCP-Backends auf den App-Zustand.
    pub fn from_state(state: &'a AppState) -> Self {
        Self {
            allowed_servers: &state.mcp.allowed_servers,
            profile_store: state.profile_store.as_ref(),
            credential_store: state.credential_store.as_ref(),
        }
    }

    /// Ob `server_id` auf der MCP-Allow-Liste steht.
    pub fn is_allowed(&self, server_id: &ServerId) -> bool {
        self.allowed_servers
            .lock()
            .expect("allowed_servers-Mutex vergiftet")
            .contains(server_id)
    }

    /// Nur Server auf der Allow-Liste, die im Profil-Store (noch)
    /// existieren. Die Liste wird vor dem ersten `await` kopiert, damit der
    /// Mutex nicht über einen Await-Punkt gehalten wird.
    pub async fn list_servers(&self) -> Vec<ServerSummary> {
        let allowed: Vec<ServerId> = self
            .allowed_servers
            .lock()
            .expect("allowed_servers-Mutex vergiftet")
            .iter()
            .copied()
            .collect();

        let mut summaries = Vec::with_capacity(allowed.len());
        for id in allowed {
            if let Ok(server) = self.profile_store.get_server(&id).await {
                summaries.push(ServerSummary {
                    id,
                    name: server.name,
                });
            }
        }
        summaries
    }

    /// Effektive Notizen von `server_id` für einen externen MCP-Client.
    ///
    /// Reihenfolge ist Teil des Vertrags: zuerst die Allow-Liste (ein nicht
    /// freigegebener Server ist für den Client nicht von einem
    /// unbekannten zu unterscheiden, `LookupError::UnknownServer`), dann
    /// Server-Lookup, dann wie im Kontext der eingebauten KI erst
    /// redigiert (Session-Redactor dieses Servers, inkl. Sudo-Passwort),
    /// danach je Abschnitt als `ServerNote` gefenct (Issue #18).
    pub async fn server_notes(&self, server_id: ServerId) -> Result<String, LookupError> {
        if !self.is_allowed(&server_id) {
            return Err(LookupError::UnknownServer);
        }
        let server = self
            .profile_store
            .get_server(&server_id)
            .await
            .map_err(|_| LookupError::UnknownServer)?;
        let redactor = server_redactor(self.credential_store, server_id);
        redacted_fenced_effective_notes(&server, self.profile_store, redactor.as_ref())
            .await
            .map_err(|_| LookupError::UnknownServer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use chrono::Utc;
    use ssh_manager_core::ai::REDACTED_PLACEHOLDER;
    use ssh_manager_core::profiles::{AuthMethod, Group, GroupId, PostIngestPolicy, Server};

    use crate::server_credentials::sudo_password_credential_ref;
    use crate::test_support::{InMemoryCredentialStore, InMemoryProfileStore};

    const TOKEN: &str = "ghp_0123456789abcdefghijABCDEFGHIJ012345";
    const KEY_BODY: &str = "b3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAAB";
    const SUDO_PASSWORD: &str = "Zq9!unusual.sudo";

    fn server(name: &str, notes: &str, group_id: Option<GroupId>) -> Server {
        let now = Utc::now();
        Server {
            id: ServerId::new(),
            name: name.to_string(),
            host: "example.invalid".to_string(),
            port: 22,
            username: "deploy".to_string(),
            group_id,
            tags: Vec::new(),
            auth: AuthMethod::Agent,
            notes: notes.to_string(),
            jump_host: None,
            post_ingest_policy: PostIngestPolicy::default(),
            ai_injection_check_enabled: false,
            sftp_server_path: None,
            start_directory: None,
            created_at: now,
            updated_at: now,
        }
    }

    fn group(name: &str, notes: &str) -> Group {
        let now = Utc::now();
        Group {
            id: GroupId::new(),
            name: name.to_string(),
            parent_id: None,
            notes: notes.to_string(),
            created_at: now,
            updated_at: now,
        }
    }

    fn secret_notes() -> String {
        format!(
            "Deploy-Token: {TOKEN}\n\
             -----BEGIN OPENSSH PRIVATE KEY-----\n{KEY_BODY}\n-----END OPENSSH PRIVATE KEY-----\n\
             sudo geht mit {SUDO_PASSWORD}\n\
             Wartungsfenster: So 02:00"
        )
    }

    fn allow(ids: &[ServerId]) -> Mutex<HashSet<ServerId>> {
        Mutex::new(ids.iter().copied().collect())
    }

    /// Issue #35, AC 1: Ein Server, der im Profil-Store existiert, aber
    /// nicht auf der Allow-Liste steht, liefert `UnknownServer` — keine
    /// Notizen, auch keine redigierten.
    #[tokio::test]
    async fn test_server_notes_for_non_allowlisted_server_is_unknown_server() {
        let hidden = server("hidden", &secret_notes(), None);
        let visible = server("visible", "", None);
        let profiles = InMemoryProfileStore::new()
            .with_server(hidden.clone())
            .with_server(visible.clone());
        let credentials = InMemoryCredentialStore::new();
        let allowed = allow(&[visible.id]);
        let lookup = McpLookup {
            allowed_servers: &allowed,
            profile_store: &profiles,
            credential_store: &credentials,
        };

        assert_eq!(
            lookup.server_notes(hidden.id).await,
            Err(LookupError::UnknownServer)
        );
    }

    /// Ein freigegebener, aber nicht (mehr) existierender Server ist
    /// ebenso `UnknownServer`.
    #[tokio::test]
    async fn test_server_notes_for_allowlisted_but_missing_server_is_unknown_server() {
        let profiles = InMemoryProfileStore::new();
        let credentials = InMemoryCredentialStore::new();
        let missing = ServerId::new();
        let allowed = allow(&[missing]);
        let lookup = McpLookup {
            allowed_servers: &allowed,
            profile_store: &profiles,
            credential_store: &credentials,
        };

        assert_eq!(
            lookup.server_notes(missing).await,
            Err(LookupError::UnknownServer)
        );
    }

    /// Issue #35, AC 2: Private Key, API-Token und das Sudo-Passwort des
    /// Servers kommen nicht im Klartext beim MCP-Client an; der Platzhalter
    /// steht an ihrer Stelle, harmloser Text bleibt erhalten.
    #[tokio::test]
    async fn test_server_notes_redacts_private_key_and_sudo_password() {
        let srv = server("web-01", &secret_notes(), None);
        let profiles = InMemoryProfileStore::new().with_server(srv.clone());
        let credentials = InMemoryCredentialStore::new()
            .with_secret(&sudo_password_credential_ref(srv.id), SUDO_PASSWORD);
        let allowed = allow(&[srv.id]);
        let lookup = McpLookup {
            allowed_servers: &allowed,
            profile_store: &profiles,
            credential_store: &credentials,
        };

        let out = lookup.server_notes(srv.id).await.expect("Notizen");

        assert!(!out.contains(KEY_BODY), "Schlüssel im Klartext: {out}");
        assert!(!out.contains(TOKEN), "Token im Klartext: {out}");
        assert!(
            !out.contains(SUDO_PASSWORD),
            "Sudo-Passwort im Klartext: {out}"
        );
        assert!(out.contains(REDACTED_PLACEHOLDER), "{out}");
        assert!(out.contains("Wartungsfenster: So 02:00"), "{out}");
    }

    /// Issue #35, AC 3: Jeder Abschnitt (Gruppe, dann Server) ist einzeln
    /// als `ServerNote` gefenct, mit seiner Quelle.
    #[tokio::test]
    async fn test_server_notes_fences_each_section_as_server_note() {
        let grp = group("prod", &format!("token={TOKEN}"));
        let srv = server("web-01", "nur Server-Notiz", Some(grp.id));
        let profiles = InMemoryProfileStore::new()
            .with_group(grp)
            .with_server(srv.clone());
        let credentials = InMemoryCredentialStore::new();
        let allowed = allow(&[srv.id]);
        let lookup = McpLookup {
            allowed_servers: &allowed,
            profile_store: &profiles,
            credential_store: &credentials,
        };

        let out = lookup.server_notes(srv.id).await.expect("Notizen");

        assert!(out.starts_with("<server_note>\n<source>"), "{out}");
        assert!(out.ends_with("</server_note>"), "{out}");
        assert_eq!(out.matches("<server_note>").count(), 2, "{out}");
        assert_eq!(out.matches("</server_note>").count(), 2, "{out}");
        assert!(out.contains("<source>prod</source>"), "{out}");
        assert!(out.contains("<source>Server \"web-01\"</source>"), "{out}");
        assert!(out.contains("nur Server-Notiz"), "{out}");
        assert!(!out.contains(TOKEN), "{out}");
        let group_pos = out.find("<source>prod").unwrap();
        let server_pos = out.find("<source>Server").unwrap();
        assert!(group_pos < server_pos, "allgemein vor spezifisch: {out}");
    }

    /// Issue #35, AC 4: `list_servers` nennt nur freigegebene Server; ein
    /// freigegebener, aber gelöschter Server fällt still heraus.
    #[tokio::test]
    async fn test_list_servers_returns_only_allowlisted_servers() {
        let a = server("allowed-a", "", None);
        let b = server("allowed-b", "", None);
        let hidden = server("hidden", "", None);
        let profiles = InMemoryProfileStore::new()
            .with_server(a.clone())
            .with_server(b.clone())
            .with_server(hidden.clone());
        let credentials = InMemoryCredentialStore::new();
        let allowed = allow(&[a.id, b.id, ServerId::new()]);
        let lookup = McpLookup {
            allowed_servers: &allowed,
            profile_store: &profiles,
            credential_store: &credentials,
        };

        let mut listed = lookup.list_servers().await;
        listed.sort_by(|x, y| x.name.cmp(&y.name));

        assert_eq!(
            listed,
            vec![
                ServerSummary {
                    id: a.id,
                    name: "allowed-a".to_string()
                },
                ServerSummary {
                    id: b.id,
                    name: "allowed-b".to_string()
                },
            ]
        );
        assert!(listed.iter().all(|s| s.id != hidden.id));
    }

    /// Leere Allow-Liste: nichts ist sichtbar, obwohl Server existieren.
    #[tokio::test]
    async fn test_list_servers_with_empty_allowlist_is_empty() {
        let srv = server("web-01", "", None);
        let profiles = InMemoryProfileStore::new().with_server(srv);
        let credentials = InMemoryCredentialStore::new();
        let allowed = allow(&[]);
        let lookup = McpLookup {
            allowed_servers: &allowed,
            profile_store: &profiles,
            credential_store: &credentials,
        };

        assert!(lookup.list_servers().await.is_empty());
    }
}
