//! Spec 0067: erhöhter SFTP-Kanal für den Dateibrowser (Zugangsnachweis,
//! Kanalwahl, Ein-/Ausschalten) — Teil der Spec-0083-Aufteilung von
//! `commands.rs`.

use std::sync::Arc;

use tauri::State;

use ssh_manager_core::ssh::SftpSession;

use crate::elevated_sftp::{ElevatedSftpRegistry, ElevatedSftpSlot, ElevationContext};
use crate::error::{CommandError, CommandResult};
use crate::session::Session;
use crate::state::{AppState, SessionId};

// --- Spec 0020, Abschnitt 5: Manueller Dateibrowser -------------------------
//
// Bewusst OHNE Filter-Engine-Prüfung — anders als `ReadRemoteFile`/
// `WriteRemoteFile` (Spec 0020, Abschnitt 4, `crate::orchestration`) laufen
// diese Befehle nie über den KI-Chat, sondern sind direkte Nutzeraktionen im
// Dateibrowser-Panel, analog zum interaktiven Terminal (Spec 0005, Abschnitt
// 1: auch dort läuft rohe Tastatureingabe ungefiltert durch).
//
// Historische Anmerkung (Spec 0020, Teil 1): `remove()` (SFTP `REMOVE`)
// wirkt nur auf Dateien, `sftp_download`/`sftp_delete` waren deshalb lange
// auf Dateien beschränkt. Spec 0054 hebt das auf: `sftp_download_default`/
// `sftp_download_dir` (Teil 2) laden Ordner rekursiv herunter, `sftp_delete`
// (Teil 3, unten) löscht sie rekursiv über die neuen Trait-Methoden
// `remove_dir`/das Zusammenspiel mit `list_dir`. "Umbenennen" (SFTP
// `RENAME`, für beide Eintragstypen) war davon nie betroffen.

/// Spec 0067, Teil A: Zugangsnachweis für den erhöhten SFTP-Kanal
/// (`crate::elevated_sftp::ElevatedSftpSlot::lock_owned` und jedes
/// Nachschlagen in der `ElevatedSftpRegistry`). Das private Feld macht ihn
/// außerhalb dieses Moduls unkonstruierbar — KI (`orchestration`) und MCP
/// (`mcp_backend`) kommen so nie an den erhöhten Kanal.
///
/// Spec 0084, A1: Seit der Kanal nicht mehr an der `Session` hängt, kommt
/// eine zweite Hürde dazu — wer ihn erreichen will, braucht zusätzlich die
/// Zuordnung, und die bekommt nur, wer sie als Tauri-Zustand anfordert.
pub(crate) struct BrowserAccess(());

#[cfg(test)]
impl BrowserAccess {
    /// Nur für Tests des erhöhten Kanals (`crate::elevated_sftp`).
    pub(crate) fn for_tests() -> Self {
        Self(())
    }
}

/// Welcher SFTP-Kanal eine Browser-Aktion ausführt. Das Frontend wählt ihn
/// pro Aufruf (`elevated_user`); ist der erhöhte Kanal inzwischen weg oder
/// läuft er als ein anderer Nutzer als erwartet, schlägt die Aktion fehl —
/// nie ein stiller Rückfall auf den normalen Kanal oder einen anderen
/// Nutzer (Spec 0067, A5: "kein stiller Upload als normaler Nutzer";
/// spec-reviewer-Fund: Edit-Flow darf nicht unter einem inzwischen
/// umgeschalteten Nutzer hochladen).
///
/// Spec 0084, A1: Der erhöhte Kanal hängt nicht mehr an der `Session`,
/// sondern an der [`ElevatedSftpRegistry`]. Er wird deshalb hier — beim
/// Übersetzen der Frontend-Anfrage — einmal nachgeschlagen und mitgeführt;
/// `slot: None` heißt „für diese Sitzung ist kein erhöhter Kanal aktiv“ und
/// lässt jede Aktion scheitern, nie zurückfallen.
pub(super) enum BrowserChannel {
    Normal,
    Elevated {
        /// Erwarteter Ziel-Nutzer des erhöhten Kanals.
        expected_user: String,
        slot: Option<ElevatedSftpSlot>,
    },
}

impl BrowserChannel {
    pub(super) fn from_request(
        registry: &ElevatedSftpRegistry,
        session_id: SessionId,
        elevated_user: Option<String>,
    ) -> Self {
        match elevated_user {
            Some(expected_user) => Self::Elevated {
                expected_user,
                slot: registry.slot(session_id, &BrowserAccess(())),
            },
            None => Self::Normal,
        }
    }
}

const ELEVATED_CHANNEL_INACTIVE: &str =
    "Der erhöhte Modus ist nicht mehr aktiv (Verbindung getrennt oder ausgeschaltet) — \
     die Aktion wurde nicht ausgeführt. Bitte den erhöhten Modus erneut einschalten.";

/// Gesperrter SFTP-Kanal einer Browser-Aktion — normal oder erhöht.
pub(super) enum BrowserSftpGuard<'a> {
    Normal(tokio::sync::MutexGuard<'a, Option<Box<dyn SftpSession>>>),
    Elevated {
        /// Spec 0084, §4: eine *eigene* Sperre je Sitzung, unabhängig von
        /// der Sperre der Zuordnung — ein laufender Vorgang hier hält das
        /// Trennen einer anderen Sitzung nicht auf. `Owned`, weil der Kanal
        /// nicht mehr an der `Session` hängt, sondern an einem Eintrag, der
        /// währenddessen aus der Zuordnung verschwinden darf.
        guard: tokio::sync::OwnedMutexGuard<Option<crate::elevated_sftp::ElevatedSftp>>,
        /// Spec 0084, §9: Derselbe Slot, um beim Zugriff den Widerruf zu
        /// prüfen — der wirkt sofort, auch während ein anderer Vorgang die
        /// Sperre oben noch hält.
        slot: ElevatedSftpSlot,
        expected_user: String,
    },
    /// Spec 0084, A1/A2: Für diese Sitzung ist kein erhöhter Kanal
    /// eingetragen (nie eingeschaltet, oder die Sitzung wurde getrennt).
    /// Jede Aktion scheitert — nie ein stiller Rückfall auf den normalen
    /// Kanal (Spec 0067, A5).
    ElevatedInactive,
}

impl BrowserSftpGuard<'_> {
    pub(super) fn sftp(&mut self) -> CommandResult<&mut Box<dyn SftpSession>> {
        match self {
            Self::Normal(guard) => Ok(guard
                .as_mut()
                .expect("browser_session öffnet den normalen Kanal vorab")),
            Self::ElevatedInactive => Err(CommandError::from(ELEVATED_CHANNEL_INACTIVE)),
            // Spec 0084, §9: Der Widerruf wird **hier** geprüft, im Moment
            // der Nutzung — nicht beim Befehlsbeginn. Der Merker geht dem
            // Herausnehmen des Kanalwerts voraus und wirkt deshalb auch für
            // einen Befehl, der schon auf der Sperre wartete, als der
            // Nutzer ausgeschaltet oder umgeschaltet hat.
            Self::Elevated { slot, .. } if slot.is_revoked() => {
                Err(CommandError::from(ELEVATED_CHANNEL_INACTIVE))
            }
            Self::Elevated {
                guard,
                expected_user,
                ..
            } => match guard.as_mut() {
                None => Err(CommandError::from(ELEVATED_CHANNEL_INACTIVE)),
                Some(elevated) if elevated.target_user != *expected_user => {
                    Err(CommandError::from(format!(
                        "Der erhöhte Modus läuft inzwischen als „{}“, nicht als „{}“ — die \
                         Aktion wurde nicht ausgeführt.",
                        elevated.target_user, expected_user
                    )))
                }
                Some(elevated) => Ok(&mut elevated.sftp),
            },
        }
    }
}

impl BrowserSftpGuard<'_> {
    /// Ziel-Nutzer, wenn diese Aktion über den erhöhten Kanal läuft.
    pub(super) fn elevated_user(&self) -> Option<String> {
        match self {
            Self::Normal(_) | Self::ElevatedInactive => None,
            // Ein widerrufener Kanal meldet keinen Ziel-Nutzer mehr: über
            // ihn läuft ohnehin keine Aktion mehr (s. `sftp`), und die
            // Audit-Zeile soll keine Erhöhung behaupten, die es nicht gab.
            Self::Elevated { slot, .. } if slot.is_revoked() => None,
            Self::Elevated { guard, .. } => guard.as_ref().map(|e| e.target_user.clone()),
        }
    }
}

pub(super) fn write_local_download(
    path: &std::path::Path,
    bytes: &[u8],
    owner_only: bool,
) -> std::io::Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    if owner_only {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    #[cfg(not(unix))]
    let _ = owner_only;
    let mut file = options.open(path)?;
    std::io::Write::write_all(&mut file, bytes)?;
    #[cfg(unix)]
    if owner_only {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

/// Spec 0067, A5 (baut auf der Audit-Erfassbarkeit aus 0054 auf): jede
/// server-verändernde Browser-Aktion im erhöhten Modus hinterlässt eine
/// strukturierte Log-Zeile mit Kennzeichnung "erhöht" + Ziel-Nutzer, Quelle
/// "manuell". Nur Aktion und Pfade, nie Dateiinhalte.
/// Auch bei einem Fehlschlag protokolliert (`ok = false`) — ein rekursives
/// Löschen/chmod kann mittendrin scheitern, nachdem schon Einträge geändert
/// wurden.
pub(super) fn audit_elevated_change(
    elevated_user: Option<&str>,
    action: &'static str,
    path: &str,
    ok: bool,
) {
    if let Some(target_user) = elevated_user {
        tracing::info!(
            action,
            path,
            ok,
            elevated = true,
            target_user,
            source = "manual",
            "file browser change with elevated rights"
        );
    }
}

pub(super) async fn lock_browser_sftp<'a>(
    session: &'a Session,
    channel: &BrowserChannel,
) -> BrowserSftpGuard<'a> {
    match channel {
        BrowserChannel::Normal => BrowserSftpGuard::Normal(session.sftp.lock().await),
        BrowserChannel::Elevated {
            expected_user,
            slot: Some(slot),
        } => BrowserSftpGuard::Elevated {
            guard: slot.lock_owned(&BrowserAccess(())).await,
            slot: slot.clone(),
            expected_user: expected_user.clone(),
        },
        BrowserChannel::Elevated { slot: None, .. } => BrowserSftpGuard::ElevatedInactive,
    }
}

/// Liefert die Session und stellt den gewählten Kanal bereit — gemeinsame
/// Vorbedingung aller `sftp_*`-Befehle unten. Normal: öffnet die
/// SFTP-Verbindung bei Bedarf (Spec 0020, Abschnitt 3). Erhöht: nur, wenn
/// der erhöhte Kanal bereits aktiv ist; er wird hier nie implizit geöffnet.
pub(super) async fn browser_session(
    state: &AppState,
    session_id: SessionId,
    channel: &BrowserChannel,
) -> CommandResult<Arc<Session>> {
    let session = state
        .sessions
        .get(session_id)
        .ok_or("Session nicht gefunden")?;
    match channel {
        BrowserChannel::Normal => crate::orchestration::ensure_sftp_open(&session).await?,
        BrowserChannel::Elevated { slot, .. } => {
            if slot.is_none() {
                return Err(ELEVATED_CHANNEL_INACTIVE.into());
            }
        }
    }
    Ok(session)
}

/// Spec 0067, A1–A3: schaltet den erhöhten Dateibrowser-Kanal ein (`sudo
/// -n <sftp-server>` über einen Exec-Kanal). Nur passwortloses sudo; ein
/// Fehlschlag kommt als strukturierte `failure` zurück (inkl. sudoers-Zeile),
/// nicht als `Err`. Nie persistiert — lebt nur in der Session.
#[tauri::command]
pub async fn sftp_elevation_enable(
    state: State<'_, AppState>,
    elevated: State<'_, ElevatedSftpRegistry>,
    session_id: SessionId,
    target_user: Option<String>,
) -> CommandResult<crate::dto::ElevationResultDto> {
    let session = state
        .sessions
        .get(session_id)
        .ok_or("Session nicht gefunden")?;
    // Transport-Grenze statt Sonderfall in der Logik: der lokale
    // Pseudo-Server hat kein sudo/sftp-server (CLAUDE.md, "No
    // special-casing ... in the core loop").
    if crate::dto::is_local(session.server_id) {
        return Ok(crate::dto::ElevationResultDto {
            active: false,
            target_user: target_user.unwrap_or_else(|| {
                ssh_manager_core::ssh::elevated::DEFAULT_ELEVATION_USER.to_string()
            }),
            sftp_server_path: None,
            failure: Some(crate::dto::ElevationFailureDto {
                kind: crate::dto::ElevationFailureKind::Unsupported,
                sudoers_line: None,
                detail: None,
            }),
        });
    }
    let server = state.profile_store.get_server(&session.server_id).await?;
    crate::elevated_sftp::enable(
        &ElevationContext {
            sessions: &state.sessions,
            registry: elevated.inner(),
            session_id,
            session: &session,
        },
        &server.username,
        server.sftp_server_path.as_deref(),
        target_user.as_deref(),
        &BrowserAccess(()),
    )
    .await
}

/// Spec 0067, A5: schaltet den erhöhten Kanal aus. Das Frontend ruft das
/// auch beim Öffnen des Browsers auf, damit ein evtl. noch offener Kanal nie
/// unbemerkt aktiv bleibt.
///
/// Spec 0084, §9 (Klarstellung 2026-09-28): Der Kanal wird nicht nur
/// ausgehängt, sondern widerrufen — ein Befehl, der ihn gerade festhält,
/// kann ihn danach nicht mehr benutzen; der Widerruf wartet auf einen
/// laufenden Vorgang. Eine unbekannte Sitzungskennung liefert wie bisher
/// `Err("Session nicht gefunden")` (T9).
#[tauri::command]
pub async fn sftp_elevation_disable(
    state: State<'_, AppState>,
    elevated: State<'_, ElevatedSftpRegistry>,
    session_id: SessionId,
) -> CommandResult<()> {
    crate::elevated_sftp::disable(
        &state.sessions,
        elevated.inner(),
        session_id,
        &BrowserAccess(()),
    )
    .await
}

/// Spec 0067, A5: Ziel-Nutzer des aktiven erhöhten Kanals, `None` = aus.
#[tauri::command]
pub async fn sftp_elevation_status(
    state: State<'_, AppState>,
    elevated: State<'_, ElevatedSftpRegistry>,
    session_id: SessionId,
) -> CommandResult<Option<String>> {
    state
        .sessions
        .get(session_id)
        .ok_or("Session nicht gefunden")?;
    let Some(slot) = elevated.slot(session_id, &BrowserAccess(())) else {
        return Ok(None);
    };
    let guard = slot.lock_owned(&BrowserAccess(())).await;
    Ok(guard.as_ref().map(|elevated| elevated.target_user.clone()))
}

pub(super) fn file_name_of(path: &str) -> String {
    // Führender Trim gegen einen abschließenden `/` (z. B. `/srv/data/`) —
    // ohne ihn liefert `rsplit('/').next()` einen leeren String, der
    // Filter greift, und der `unwrap_or(path)`-Fallback gibt versehentlich
    // den GESAMTEN Pfad statt nur seines letzten Segments zurück (Spec-
    // Reviewer-Fund, Spec 0054, Review des Gesamtpakets).
    path.trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or(path)
        .to_string()
}

/// Spec-Reviewer-Fund (Spec 0054, Review des Gesamtpakets, ERHÖHTE
/// Priorität): jeder Punkt, an dem ein vom SFTP-SERVER gelieferter Name
/// (`RemoteEntry::name`, oder ein daraus über `file_name_of` abgeleiteter
/// Name) als LOKALES Pfadsegment verwendet wird (rekursiver Ordner-
/// Download, "Lokal öffnen"), ist ein klassisches Zip-Slip-Risiko: ein
/// (kompromittierter oder fehlerhaft implementierter) Server könnte statt
/// eines normalen Dateinamens `"../../.zshrc"` oder einen absoluten Pfad
/// wie `"/Users/u/.ssh/authorized_keys"` liefern — `PathBuf::join(..)`
/// verlässt bei `..`-Segmenten das Zielverzeichnis, und bei einem
/// absoluten Pfad ERSETZT `join()` den kompletten bisherigen Präfix statt
/// ihn anzuhängen. Lehnt jeden Namen ab, der nicht GENAU EIN normales
/// Pfadsegment ist (kein `.`/`..`, kein eingebetteter Separator, keine
/// führende Root/Präfix-Komponente) — funktioniert plattformunabhängig,
/// da `Path::components()` die jeweils betriebssystemeigene
/// Separator-/Präfix-Erkennung übernimmt.
pub(super) fn safe_local_segment(name: &str) -> CommandResult<()> {
    use std::path::{Component, Path};
    let mut components = Path::new(name).components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(_)), None) => Ok(()),
        _ => Err(CommandError::from(format!(
            "Unsicherer Dateiname vom Server abgelehnt: '{name}'"
        ))),
    }
}

#[cfg(test)]
mod safe_local_segment_tests {
    //! Spec-Reviewer-Fund (Spec 0054, Review des Gesamtpakets, ERHÖHTE
    //! Priorität): Zip-Slip über servergelieferte Dateinamen beim
    //! rekursiven Ordner-Download/"Lokal öffnen". `download_recursive`/
    //! `download_entry_to`/`sftp_open_for_editing` rufen `safe_local_segment`
    //! jetzt vor jedem `PathBuf::join(entry.name)` auf — diese Tests prüfen
    //! nur die reine Funktion (`std::path::Path::join`s Verhalten bei
    //! `..`-Segmenten/absoluten Pfaden ist dokumentiertes, stabiles
    //! Standardbibliotheks-Verhalten, kein separat zu beweisender Teil).
    use super::*;

    #[test]
    fn test_accepts_a_plain_file_name() {
        assert!(safe_local_segment("readme.md").is_ok());
        assert!(safe_local_segment("nginx.conf.smartssh-backup-123").is_ok());
    }

    #[test]
    fn test_rejects_parent_directory_traversal() {
        assert!(safe_local_segment("..").is_err());
        assert!(safe_local_segment("../etc/passwd").is_err());
        assert!(safe_local_segment("../../.zshrc").is_err());
    }

    #[test]
    fn test_rejects_current_directory_segment() {
        assert!(safe_local_segment(".").is_err());
    }

    #[test]
    fn test_rejects_an_embedded_separator() {
        assert!(safe_local_segment("a/b").is_err());
    }

    #[test]
    fn test_rejects_an_absolute_path() {
        assert!(safe_local_segment("/etc/passwd").is_err());
        assert!(safe_local_segment("/Users/u/.ssh/authorized_keys").is_err());
    }
}

#[cfg(test)]
mod browser_channel_tests {
    use ssh_manager_core::ssh::{CommandOutput, PtySize, SshError, SshTransport};

    use super::*;

    struct NoTransport;
    #[async_trait::async_trait]
    impl SshTransport for NoTransport {
        async fn execute(&mut self, _command: &str) -> Result<CommandOutput, SshError> {
            unreachable!()
        }
        async fn open_shell(
            &mut self,
            _size: PtySize,
        ) -> Result<Box<dyn ssh_manager_core::ssh::InteractiveShell>, SshError> {
            unreachable!()
        }
        async fn disconnect(&mut self) -> Result<(), SshError> {
            Ok(())
        }
    }

    /// Spec 0084, A1: der erhöhte Kanal liegt in der Zuordnung, nicht an der
    /// `Session` — Tests bauen ihre Ausgangslage deshalb hier auf.
    fn registry_with_channel(
        session_id: SessionId,
        target_user: &str,
        sftp: ssh_manager_core::ssh::mock::MockSftpSession,
    ) -> ElevatedSftpRegistry {
        let registry = ElevatedSftpRegistry::default();
        registry.insert_for_tests(
            session_id,
            crate::elevated_sftp::ElevatedSftp {
                target_user: target_user.to_string(),
                sftp: Box::new(sftp),
            },
        );
        registry
    }

    #[tokio::test]
    async fn test_elevated_request_without_active_channel_fails_instead_of_falling_back() {
        let session = crate::test_support::session_with_transport(Box::new(NoTransport));
        *session.sftp.lock().await =
            Some(Box::new(ssh_manager_core::ssh::mock::MockSftpSession::new()));
        let registry = ElevatedSftpRegistry::default();
        let session_id = SessionId::new_v4();

        let channel = BrowserChannel::from_request(&registry, session_id, Some("root".to_string()));
        let mut guard = lock_browser_sftp(&session, &channel).await;
        let err = guard
            .sftp()
            .err()
            .expect("ohne aktiven erhöhten Kanal muss es scheitern");
        assert!(err.message.contains("nicht mehr aktiv"), "{}", err.message);
    }

    /// spec-reviewer-Fund (Spec 0067): läuft der erhöhte Kanal inzwischen
    /// als ein anderer Nutzer als erwartet (z. B. Edit-Flow als www-data
    /// geöffnet, danach als root neu eingeschaltet), scheitert die Aktion.
    #[tokio::test]
    async fn test_elevated_request_for_another_user_than_active_fails() {
        let session = crate::test_support::session_with_transport(Box::new(NoTransport));
        let session_id = SessionId::new_v4();
        let registry = registry_with_channel(
            session_id,
            "root",
            ssh_manager_core::ssh::mock::MockSftpSession::new(),
        );

        let channel =
            BrowserChannel::from_request(&registry, session_id, Some("www-data".to_string()));
        let mut guard = lock_browser_sftp(&session, &channel).await;
        let err = guard
            .sftp()
            .err()
            .expect("anderer Nutzer als erwartet muss scheitern");
        assert!(err.message.contains("www-data"), "{}", err.message);
    }

    #[tokio::test]
    async fn test_elevated_request_uses_the_elevated_channel_not_the_normal_one() {
        let session = crate::test_support::session_with_transport(Box::new(NoTransport));
        let normal = ssh_manager_core::ssh::mock::MockSftpSession::new().with_file("/x", "USER");
        let elevated = ssh_manager_core::ssh::mock::MockSftpSession::new().with_file("/x", "ROOT");
        *session.sftp.lock().await = Some(Box::new(normal));
        let session_id = SessionId::new_v4();
        let registry = registry_with_channel(session_id, "root", elevated);

        let channel = BrowserChannel::from_request(&registry, session_id, Some("root".to_string()));
        let mut guard = lock_browser_sftp(&session, &channel).await;
        assert_eq!(
            guard.sftp().unwrap().read_file("/x").await.unwrap(),
            b"ROOT"
        );
        drop(guard);
        let channel = BrowserChannel::from_request(&registry, session_id, None);
        let mut guard = lock_browser_sftp(&session, &channel).await;
        assert_eq!(
            guard.sftp().unwrap().read_file("/x").await.unwrap(),
            b"USER"
        );
    }

    /// Spec 0084, T7: Zwei gleichzeitige Sitzungen zum **selben** Server,
    /// der erhöhte Kanal ist nur in A aktiv. B muss über den normalen Kanal
    /// laufen, und Ausschalten in B darf A nicht berühren. Scheitert, wenn
    /// die Zuordnung nicht an der Sitzung hängt (z. B. am Server).
    #[tokio::test]
    async fn test_t7_an_elevated_channel_belongs_to_its_session_not_to_the_server() {
        let server_id = ssh_manager_core::shared::ServerId::new();
        let mut session_a = crate::test_support::session_with_transport(Box::new(NoTransport));
        let mut session_b = crate::test_support::session_with_transport(Box::new(NoTransport));
        session_a.server_id = server_id;
        session_b.server_id = server_id;
        *session_b.sftp.lock().await = Some(Box::new(
            ssh_manager_core::ssh::mock::MockSftpSession::new().with_file("/x", "USER-B"),
        ));

        let id_a = SessionId::new_v4();
        let id_b = SessionId::new_v4();
        let sessions = crate::session::SessionManager::new();
        let session_a = std::sync::Arc::new(session_a);
        let session_b = std::sync::Arc::new(session_b);
        sessions.insert(id_a, session_a.clone());
        sessions.insert(id_b, session_b.clone());
        let registry = registry_with_channel(
            id_a,
            "root",
            ssh_manager_core::ssh::mock::MockSftpSession::new().with_file("/x", "ROOT-A"),
        );

        // B fragt den erhöhten Kanal an: es gibt keinen für B.
        let channel_b = BrowserChannel::from_request(&registry, id_b, Some("root".to_string()));
        assert!(
            matches!(channel_b, BrowserChannel::Elevated { slot: None, .. }),
            "B darf den erhöhten Kanal von A nicht sehen"
        );
        let mut guard = lock_browser_sftp(&session_b, &channel_b).await;
        assert!(
            guard.sftp().is_err(),
            "eine erhöhte Aktion in B muss scheitern statt A's Kanal zu benutzen"
        );
        drop(guard);

        // B über den normalen Kanal: liest B's eigene Daten.
        let channel_b = BrowserChannel::from_request(&registry, id_b, None);
        let mut guard = lock_browser_sftp(&session_b, &channel_b).await;
        assert_eq!(
            guard.sftp().unwrap().read_file("/x").await.unwrap(),
            b"USER-B"
        );
        drop(guard);

        // Ausschalten in B lässt A unberührt.
        crate::elevated_sftp::disable(&sessions, &registry, id_b, &BrowserAccess(()))
            .await
            .expect("B ist bekannt, hat aber keinen Kanal — kein Fehler");
        let channel_a = BrowserChannel::from_request(&registry, id_a, Some("root".to_string()));
        let mut guard = lock_browser_sftp(&session_a, &channel_a).await;
        assert_eq!(
            guard.sftp().unwrap().read_file("/x").await.unwrap(),
            b"ROOT-A",
            "Ausschalten in B darf den Kanal von A nicht entfernen"
        );
    }

    // --- Spec 0084, §9 (Klarstellung 2026-09-28): Widerruf wirkt beim
    // Zugriff, nicht beim Befehlsbeginn ----------------------------------
    //
    // Angriffsbild aus dem Review: `sftp_download` schlägt den erhöhten
    // Kanal zu Befehlsbeginn nach und öffnet danach den Speichern-Dialog.
    // Schaltet der Nutzer währenddessen die erhöhten Rechte aus (oder auf
    // einen anderen Nutzer um, oder trennt die Sitzung), darf der bereits
    // festgehaltene Kanal danach nicht mehr benutzt werden.
    //
    // Alle drei Tests scheitern gegen die Variante „Kanal bei Befehlsbeginn
    // festhalten“, also gegen ein Widerrufen, das den Kanal weder
    // herausnimmt noch als widerrufen markiert.
    //
    // T10 und T10b laufen über `disable`/`enable`, die den Kanalwert
    // zusätzlich herausnehmen; T10c läuft über `remove_session`, das nur
    // markiert (damit das Trennen auf keinen Transfer wartet) — dort kann
    // der Fehler deshalb ausschließlich aus der Widerrufs-Prüfung in
    // `sftp()` stammen.

    /// Fängt genau den Ablauf ein, den ein Befehl durchläuft: Kanal zu
    /// Beginn nachschlagen, erste Nutzung, Wartezeit, zweite Nutzung.
    async fn held_channel(
        session_id: SessionId,
        registry: &ElevatedSftpRegistry,
        session: &Session,
    ) -> BrowserChannel {
        let channel = BrowserChannel::from_request(registry, session_id, Some("root".to_string()));
        let mut guard = lock_browser_sftp(session, &channel).await;
        assert_eq!(
            guard
                .sftp()
                .expect("Vorbedingung: die erste Nutzung gelingt")
                .read_file("/x")
                .await
                .unwrap(),
            b"ROOT"
        );
        channel
    }

    /// Spec 0084, T10: Kanal von einem Befehl festgehalten, Nutzer schaltet
    /// die erhöhten Rechte aus — die nächste Nutzung scheitert.
    #[tokio::test]
    async fn test_t10_a_held_channel_cannot_be_used_after_disabling() {
        let session = std::sync::Arc::new(crate::test_support::session_with_transport(Box::new(
            NoTransport,
        )));
        let session_id = SessionId::new_v4();
        let sessions = crate::session::SessionManager::new();
        sessions.insert(session_id, session.clone());
        let registry = registry_with_channel(
            session_id,
            "root",
            ssh_manager_core::ssh::mock::MockSftpSession::new().with_file("/x", "ROOT"),
        );

        let channel = held_channel(session_id, &registry, &session).await;

        crate::elevated_sftp::disable(&sessions, &registry, session_id, &BrowserAccess(()))
            .await
            .expect("Ausschalten gelingt");

        let mut guard = lock_browser_sftp(&session, &channel).await;
        let err = guard
            .sftp()
            .err()
            .expect("nach dem Ausschalten darf der festgehaltene Kanal nicht mehr tragen");
        assert!(err.message.contains("nicht mehr aktiv"), "{}", err.message);
    }

    /// Spec 0084, T10b: dasselbe, aber der Kanal wird zwischendurch für
    /// einen **anderen** Nutzer neu aktiviert. Der festgehaltene root-Kanal
    /// darf danach nicht weiterlaufen.
    #[tokio::test]
    async fn test_t10b_a_held_channel_cannot_be_used_after_reactivating_for_another_user() {
        let f = crate::test_support::elevation::fixture(Box::new(
            crate::test_support::elevation::working_transport(),
        ));
        // Ausgangslage über den echten Weg: erhöhte Rechte als root aktiv.
        crate::elevated_sftp::enable(&f.ctx(), "deploy", None, None, &BrowserAccess(()))
            .await
            .expect("Vorbedingung: Aktivieren als root gelingt");
        let channel =
            BrowserChannel::from_request(&f.registry, f.session_id, Some("root".to_string()));
        let mut guard = lock_browser_sftp(&f.session, &channel).await;
        assert!(
            guard.sftp().is_ok(),
            "Vorbedingung: die erste Nutzung gelingt"
        );
        drop(guard);

        crate::elevated_sftp::enable(
            &f.ctx(),
            "deploy",
            None,
            Some("www-data"),
            &BrowserAccess(()),
        )
        .await
        .expect("Neu-Aktivieren als www-data gelingt");

        let mut guard = lock_browser_sftp(&f.session, &channel).await;
        let err = guard
            .sftp()
            .err()
            .expect("der festgehaltene root-Kanal darf nach dem Umschalten nicht mehr tragen");
        assert!(err.message.contains("nicht mehr aktiv"), "{}", err.message);
    }

    /// Spec 0084, T10c: dasselbe, aber die Sitzung wird über A2.1 entfernt
    /// (Trennen). Auch dann darf der festgehaltene Kanal nicht mehr tragen.
    #[tokio::test]
    async fn test_t10c_a_held_channel_cannot_be_used_after_the_session_was_removed() {
        let session = std::sync::Arc::new(crate::test_support::session_with_transport(Box::new(
            NoTransport,
        )));
        let session_id = SessionId::new_v4();
        let sessions = crate::session::SessionManager::new();
        sessions.insert(session_id, session.clone());
        let registry = registry_with_channel(
            session_id,
            "root",
            ssh_manager_core::ssh::mock::MockSftpSession::new().with_file("/x", "ROOT"),
        );

        let channel = held_channel(session_id, &registry, &session).await;

        registry
            .remove_session(&sessions, session_id)
            .expect("die Sitzung war eingetragen");

        let mut guard = lock_browser_sftp(&session, &channel).await;
        let err = guard
            .sftp()
            .err()
            .expect("nach dem Trennen darf der festgehaltene Kanal nicht mehr tragen");
        assert!(err.message.contains("nicht mehr aktiv"), "{}", err.message);
    }

    /// spec-reviewer-Fund (Runde 2): Das Trennen darf nicht hinter einem
    /// laufenden erhöhten Transfer stehenbleiben. Der Widerruf beim
    /// Entfernen einer Sitzung ist deshalb der sofort wirksame Merker, kein
    /// Warten auf die Kanal-Sperre — sonst reagierte ein Klick auf
    /// „Trennen" während eines hängenden Uploads sichtbar gar nicht.
    ///
    /// Scheitert (mit Zeitüberschreitung) gegen die Variante, die beim
    /// Entfernen auf die Kanal-Sperre wartet.
    #[tokio::test]
    async fn test_removing_a_session_does_not_wait_for_a_running_elevated_transfer() {
        let session = std::sync::Arc::new(crate::test_support::session_with_transport(Box::new(
            NoTransport,
        )));
        let session_id = SessionId::new_v4();
        let sessions = std::sync::Arc::new(crate::session::SessionManager::new());
        sessions.insert(session_id, session.clone());
        let registry = std::sync::Arc::new(registry_with_channel(
            session_id,
            "root",
            ssh_manager_core::ssh::mock::MockSftpSession::new().with_file("/x", "ROOT"),
        ));

        // Ein laufender Transfer hält die Kanal-Sperre über seine ganze
        // Dauer — genau das tun `sftp_upload`/`download_one_file`.
        let held = registry
            .slot(session_id, &BrowserAccess(()))
            .expect("Vorbedingung: Kanal aktiv");
        let transfer_guard = held.lock_owned(&BrowserAccess(())).await;

        // In einem eigenen Thread, damit eine Variante, die doch auf die
        // Kanal-Sperre wartet, als Zeitüberschreitung sichtbar wird, statt
        // den ganzen Testlauf hängen zu lassen.
        let registry_for_removal = registry.clone();
        let sessions_for_removal = sessions.clone();
        let removal = tokio::task::spawn_blocking(move || {
            registry_for_removal.remove_session(&sessions_for_removal, session_id)
        });
        tokio::time::timeout(std::time::Duration::from_secs(5), removal)
            .await
            .expect("Trennen darf nicht auf den laufenden Transfer warten")
            .expect("der Entfernen-Task darf nicht panisch enden")
            .expect("die Sitzung war eingetragen");

        assert!(
            held.is_revoked(),
            "der Widerruf muss schon wirken, während der Transfer die Sperre noch hält"
        );

        // Und der Transfer selbst ist ab sofort nicht mehr erhöht nutzbar.
        drop(transfer_guard);
        let channel = BrowserChannel::from_request(&registry, session_id, Some("root".to_string()));
        let mut guard = lock_browser_sftp(&session, &channel).await;
        assert!(guard.sftp().is_err());
    }

    /// spec-reviewer-Fund (Runde 2): Ein Browser-Befehl, der beim
    /// Umschalten des Ziel-Nutzers **bereits auf der Kanal-Sperre wartet**,
    /// bekommt sie vor dem Widerruf (die Sperre ist fair) — er darf den
    /// alten Kanal danach trotzdem nicht mehr benutzen.
    ///
    /// Scheitert gegen die Variante, die den Widerruf erst am Ende der
    /// Warteschlange wirken lässt (Widerruf nur per `take()` unter der
    /// Sperre, ohne sofort wirksamen Merker).
    #[tokio::test]
    async fn test_a_command_already_waiting_on_the_channel_fails_after_the_user_switched() {
        let f = crate::test_support::elevation::fixture(Box::new(
            crate::test_support::elevation::working_transport(),
        ));
        crate::elevated_sftp::enable(&f.ctx(), "deploy", None, None, &BrowserAccess(()))
            .await
            .expect("Vorbedingung: Aktivieren als root gelingt");

        // Befehl B hat den Kanal schon nachgeschlagen und wartet auf die
        // Sperre, die Befehl A gerade hält.
        let channel_b =
            BrowserChannel::from_request(&f.registry, f.session_id, Some("root".to_string()));
        let held = f
            .registry
            .slot(f.session_id, &BrowserAccess(()))
            .expect("Vorbedingung: Kanal aktiv");
        let guard_a = held.lock_owned(&BrowserAccess(())).await;

        // Der Nutzer schaltet auf www-data um. Das Aktivieren wartet
        // seinerseits auf A, der Widerruf des alten Kanals muss aber schon
        // vorher greifen.
        let f_for_switch = std::sync::Arc::new(f);
        let f_for_task = f_for_switch.clone();
        let switch = tokio::spawn(async move {
            crate::elevated_sftp::enable(
                &f_for_task.ctx(),
                "deploy",
                None,
                Some("www-data"),
                &BrowserAccess(()),
            )
            .await
        });
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while !held.is_revoked() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("der Widerruf muss wirken, bevor die Sperre frei wird");

        // Jetzt gibt A die Sperre frei, B kommt dran — und scheitert.
        drop(guard_a);
        let mut guard = lock_browser_sftp(&f_for_switch.session, &channel_b).await;
        let err = guard
            .sftp()
            .err()
            .expect("B darf nach dem Nutzerwechsel nicht mehr als root arbeiten");
        assert!(err.message.contains("nicht mehr aktiv"), "{}", err.message);
        drop(guard);

        tokio::time::timeout(std::time::Duration::from_secs(5), switch)
            .await
            .expect("das Umschalten muss enden")
            .expect("der Umschalt-Task darf nicht panisch enden")
            .expect("das Umschalten selbst gelingt");
    }
}
