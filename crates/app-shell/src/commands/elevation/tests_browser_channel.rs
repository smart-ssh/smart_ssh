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
            sudo_auth: Default::default(),
            server_logging: false,
            sftp: Box::new(sftp),
        },
    );
    registry
}

#[tokio::test]
async fn test_elevated_request_without_active_channel_fails_instead_of_falling_back() {
    let session = app_logic::test_support::session_with_transport(Box::new(NoTransport));
    session
        .set_sftp_for_tests(Box::new(ssh_manager_core::ssh::mock::MockSftpSession::new()))
        .await;
    let registry = ElevatedSftpRegistry::default();
    let session_id = SessionId::new_v4();

    let channel = BrowserChannel::from_request(&registry, session_id, Some("root".to_string()));
    let mut guard = lock_browser_sftp(&session, &channel).await;
    let err = guard
        .sftp()
        .await
        .err()
        .expect("ohne aktiven erhöhten Kanal muss es scheitern");
    assert!(err.message.contains("nicht mehr aktiv"), "{}", err.message);
}

/// spec-reviewer-Fund (Spec 0067): läuft der erhöhte Kanal inzwischen
/// als ein anderer Nutzer als erwartet (z. B. Edit-Flow als www-data
/// geöffnet, danach als root neu eingeschaltet), scheitert die Aktion.
#[tokio::test]
async fn test_elevated_request_for_another_user_than_active_fails() {
    let session = app_logic::test_support::session_with_transport(Box::new(NoTransport));
    let session_id = SessionId::new_v4();
    let registry = registry_with_channel(
        session_id,
        "root",
        ssh_manager_core::ssh::mock::MockSftpSession::new(),
    );

    let channel = BrowserChannel::from_request(&registry, session_id, Some("www-data".to_string()));
    let mut guard = lock_browser_sftp(&session, &channel).await;
    let err = guard
        .sftp()
        .await
        .err()
        .expect("anderer Nutzer als erwartet muss scheitern");
    assert!(err.message.contains("www-data"), "{}", err.message);
}

#[tokio::test]
async fn test_elevated_request_uses_the_elevated_channel_not_the_normal_one() {
    let session = app_logic::test_support::session_with_transport(Box::new(NoTransport));
    let normal = ssh_manager_core::ssh::mock::MockSftpSession::new().with_file("/x", "USER");
    let elevated = ssh_manager_core::ssh::mock::MockSftpSession::new().with_file("/x", "ROOT");
    session.set_sftp_for_tests(Box::new(normal)).await;
    let session_id = SessionId::new_v4();
    let registry = registry_with_channel(session_id, "root", elevated);

    let channel = BrowserChannel::from_request(&registry, session_id, Some("root".to_string()));
    let mut guard = lock_browser_sftp(&session, &channel).await;
    assert_eq!(
        guard.sftp().await.unwrap().read_file("/x").await.unwrap(),
        b"ROOT"
    );
    drop(guard);
    let channel = BrowserChannel::from_request(&registry, session_id, None);
    let mut guard = lock_browser_sftp(&session, &channel).await;
    assert_eq!(
        guard.sftp().await.unwrap().read_file("/x").await.unwrap(),
        b"USER"
    );
}

/// Issue #1291: the elevated channel forwards `read_link` to its own session
/// (not the trait default, which would make every link target unknown).
#[tokio::test]
async fn test_elevated_channel_delegates_read_link_to_the_elevated_session() {
    let session = app_logic::test_support::session_with_transport(Box::new(NoTransport));
    let normal = ssh_manager_core::ssh::mock::MockSftpSession::new().with_link("/l", "USER");
    let elevated = ssh_manager_core::ssh::mock::MockSftpSession::new().with_link("/l", "/root/x");
    session.set_sftp_for_tests(Box::new(normal)).await;
    let session_id = SessionId::new_v4();
    let registry = registry_with_channel(session_id, "root", elevated);

    let channel = BrowserChannel::from_request(&registry, session_id, Some("root".to_string()));
    let mut guard = lock_browser_sftp(&session, &channel).await;
    assert_eq!(
        guard.sftp().await.unwrap().read_link("/l").await.unwrap(),
        "/root/x"
    );
}

/// Spec 0084, T7: Zwei gleichzeitige Sitzungen zum **selben** Server,
/// der erhöhte Kanal ist nur in A aktiv. B muss über den normalen Kanal
/// laufen, und Ausschalten in B darf A nicht berühren. Scheitert, wenn
/// die Zuordnung nicht an der Sitzung hängt (z. B. am Server).
#[tokio::test]
async fn test_t7_an_elevated_channel_belongs_to_its_session_not_to_the_server() {
    let server_id = ssh_manager_core::shared::ServerId::new();
    let mut session_a = app_logic::test_support::session_with_transport(Box::new(NoTransport));
    let mut session_b = app_logic::test_support::session_with_transport(Box::new(NoTransport));
    session_a.set_server_id_for_tests(server_id);
    session_b.set_server_id_for_tests(server_id);
    session_b
        .set_sftp_for_tests(Box::new(
            ssh_manager_core::ssh::mock::MockSftpSession::new().with_file("/x", "USER-B"),
        ))
        .await;

    let id_a = SessionId::new_v4();
    let id_b = SessionId::new_v4();
    let sessions = app_logic::session::SessionManager::new();
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
        channel_b.is_elevated() && !channel_b.has_elevated_slot(),
        "B darf den erhöhten Kanal von A nicht sehen"
    );
    let mut guard = lock_browser_sftp(&session_b, &channel_b).await;
    assert!(
        guard.sftp().await.is_err(),
        "eine erhöhte Aktion in B muss scheitern statt A's Kanal zu benutzen"
    );
    drop(guard);

    // B über den normalen Kanal: liest B's eigene Daten.
    let channel_b = BrowserChannel::from_request(&registry, id_b, None);
    let mut guard = lock_browser_sftp(&session_b, &channel_b).await;
    assert_eq!(
        guard.sftp().await.unwrap().read_file("/x").await.unwrap(),
        b"USER-B"
    );
    drop(guard);

    // Ausschalten in B lässt A unberührt.
    crate::elevated_sftp::disable(&sessions, &registry, id_b, false, &BrowserAccess(()))
        .await
        .expect("B ist bekannt, hat aber keinen Kanal — kein Fehler");
    let channel_a = BrowserChannel::from_request(&registry, id_a, Some("root".to_string()));
    let mut guard = lock_browser_sftp(&session_a, &channel_a).await;
    assert_eq!(
        guard.sftp().await.unwrap().read_file("/x").await.unwrap(),
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
            .await
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
    let session = std::sync::Arc::new(app_logic::test_support::session_with_transport(Box::new(
        NoTransport,
    )));
    let session_id = SessionId::new_v4();
    let sessions = app_logic::session::SessionManager::new();
    sessions.insert(session_id, session.clone());
    let registry = registry_with_channel(
        session_id,
        "root",
        ssh_manager_core::ssh::mock::MockSftpSession::new().with_file("/x", "ROOT"),
    );

    let channel = held_channel(session_id, &registry, &session).await;

    crate::elevated_sftp::disable(&sessions, &registry, session_id, false, &BrowserAccess(()))
        .await
        .expect("Ausschalten gelingt");

    let mut guard = lock_browser_sftp(&session, &channel).await;
    let err = guard
        .sftp()
        .await
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
    crate::elevated_sftp::enable(&f.ctx(), "deploy", None, None, false, &BrowserAccess(()))
        .await
        .expect("Vorbedingung: Aktivieren als root gelingt");
    let channel = BrowserChannel::from_request(&f.registry, f.session_id, Some("root".to_string()));
    let mut guard = lock_browser_sftp(&f.session, &channel).await;
    assert!(
        guard.sftp().await.is_ok(),
        "Vorbedingung: die erste Nutzung gelingt"
    );
    drop(guard);

    crate::elevated_sftp::enable(
        &f.ctx(),
        "deploy",
        None,
        Some("www-data"),
        false,
        &BrowserAccess(()),
    )
    .await
    .expect("Neu-Aktivieren als www-data gelingt");

    let mut guard = lock_browser_sftp(&f.session, &channel).await;
    let err = guard
        .sftp()
        .await
        .err()
        .expect("der festgehaltene root-Kanal darf nach dem Umschalten nicht mehr tragen");
    assert!(err.message.contains("nicht mehr aktiv"), "{}", err.message);
}

/// Spec 0084, T10c: dasselbe, aber die Sitzung wird über A2.1 entfernt
/// (Trennen). Auch dann darf der festgehaltene Kanal nicht mehr tragen.
#[tokio::test]
async fn test_t10c_a_held_channel_cannot_be_used_after_the_session_was_removed() {
    let session = std::sync::Arc::new(app_logic::test_support::session_with_transport(Box::new(
        NoTransport,
    )));
    let session_id = SessionId::new_v4();
    let sessions = app_logic::session::SessionManager::new();
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
        .await
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
    let session = std::sync::Arc::new(app_logic::test_support::session_with_transport(Box::new(
        NoTransport,
    )));
    let session_id = SessionId::new_v4();
    let sessions = std::sync::Arc::new(app_logic::session::SessionManager::new());
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
    // Gegenbeweis bleibt gültig: Eine Variante, die beim Entfernen auf die
    // Kanal-Sperre wartet, kehrt nie zurück, solange `transfer_guard`
    // gehalten wird — sie scheitert an der Obergrenze statt an 5 s.
    crate::test_support::waiting::expect_within(
        "Trennen kehrt zurück, ohne auf den laufenden Transfer zu warten",
        || format!("Kanal widerrufen: {}", held.is_revoked()),
        removal,
    )
    .await
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
    assert!(guard.sftp().await.is_err());
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
    crate::elevated_sftp::enable(&f.ctx(), "deploy", None, None, false, &BrowserAccess(()))
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
            false,
            &BrowserAccess(()),
        )
        .await
    });
    // Wartet auf das Ereignis selbst (der Widerruf ist gesetzt), nicht auf
    // eine Dauer. Die Abfrage gibt nach jedem Blick ab, damit der
    // Umschalt-Task auf derselben Ein-Thread-Laufzeit weiterkommt.
    crate::test_support::waiting::expect_within(
        "der Widerruf wirkt, bevor die Sperre frei wird",
        || {
            format!(
                "Kanal nicht widerrufen, Umschalten beendet: {}",
                switch.is_finished()
            )
        },
        async {
            while !held.is_revoked() {
                tokio::task::yield_now().await;
            }
        },
    )
    .await;

    // Jetzt gibt A die Sperre frei, B kommt dran — und scheitert.
    drop(guard_a);
    let mut guard = lock_browser_sftp(&f_for_switch.session, &channel_b).await;
    let err = guard
        .sftp()
        .await
        .err()
        .expect("B darf nach dem Nutzerwechsel nicht mehr als root arbeiten");
    assert!(err.message.contains("nicht mehr aktiv"), "{}", err.message);
    drop(guard);

    crate::test_support::waiting::expect_within(
        "das Umschalten endet, nachdem B die Sperre freigegeben hat",
        || format!("alter Kanal widerrufen: {}", held.is_revoked()),
        switch,
    )
    .await
    .expect("der Umschalt-Task darf nicht panisch enden")
    .expect("das Umschalten selbst gelingt");
}

// --- Spec 0084, T5/T5b -------------------------------------------------
//
// Reine `orchestration`-Tests, aber sie müssen den erhöhten Kanal
// aufbauen können (`ElevatedSftpRegistry`, `BrowserAccess`) — beides
// bleibt nach Spec 0084 A1 unerreichbar für `app-logic`. Deshalb leben
// sie hier statt in `app_logic::orchestration::chat_turn` (Modulpfad
// darf sich ändern, A6.3 — die Testnamen bleiben wörtlich erhalten).

/// Lässt jedes Kommando zu — für Tests, die die Filter-Engine nicht
/// prüfen wollen, nur die Kanalwahl.
struct AllowEverythingPolicyStore;
#[async_trait::async_trait]
impl ssh_manager_core::filter::PolicyStore for AllowEverythingPolicyStore {
    async fn rules_for(
        &self,
        _scope: &ssh_manager_core::filter::EffectiveScope,
    ) -> ssh_manager_core::filter::PolicyRules {
        ssh_manager_core::filter::PolicyRules::complete(vec![ssh_manager_core::filter::Rule {
            id: ssh_manager_core::filter::RuleId("allow-all".to_string()),
            pattern: ssh_manager_core::filter::Pattern::Glob("*".to_string()),
            action: ssh_manager_core::filter::RuleAction::Allow,
            scope: ssh_manager_core::filter::Scope::Global,
            priority: 0,
            origin: ssh_manager_core::filter::RuleOrigin::User,
        }])
    }
}

/// Schlägt die Aktionen nur in der **ersten** Runde vor und antwortet in
/// jeder Folgerunde mit `Done`.
///
/// Issue #114: Der feste `MockAiProvider` liefert bei jedem Aufruf
/// dieselben Events. Ein KI-Zweig mit ihm schlug das Lesen in jeder
/// Folgerunde erneut vor, bis zur Rundengrenze (zehn Runden), und jede
/// Runde wartete vorher den echten Mindestabstand zwischen zwei
/// KI-Anfragen ab (300 ms). Das waren rund 3 s echte Zeit in einer Frist
/// von 5 s, und unter Last lief die Frist ab. Eine Runde mit der Aktion
/// reicht für die Aussage des Tests.
struct ProposeOnceAiProvider {
    first_round: Vec<ssh_manager_core::ai::AiEvent>,
    sent: std::sync::atomic::AtomicBool,
}

impl ProposeOnceAiProvider {
    fn new(first_round: Vec<ssh_manager_core::ai::AiEvent>) -> Self {
        Self {
            first_round,
            sent: std::sync::atomic::AtomicBool::new(false),
        }
    }
}

impl ssh_manager_core::ai::AiProvider for ProposeOnceAiProvider {
    fn send(
        &self,
        _context: ssh_manager_core::ai::RedactedContext,
    ) -> std::pin::Pin<Box<dyn futures::Stream<Item = ssh_manager_core::ai::AiEvent> + Send>> {
        let events = if self.sent.swap(true, std::sync::atomic::Ordering::SeqCst) {
            vec![ssh_manager_core::ai::AiEvent::Done]
        } else {
            self.first_round.clone()
        };
        Box::pin(futures::stream::iter(events))
    }
}

/// Genehmigt die erste vorgeschlagene Aktion, sobald ihr
/// `chat-action-proposed`-Event gesendet ist — wartet auf das Event
/// statt die Event-Liste in einer Schleife abzufragen.
async fn approve_first_proposal(
    emitter: &crate::test_support::waiting::NotifyingEmitter,
    confirmations: &app_logic::confirmation::ConfirmationRegistry<
        app_logic::state::ActionId,
        app_logic::dto::ActionUserDecision,
    >,
) {
    let proposed = emitter.wait_for_event("chat-action-proposed").await;
    let id = proposed["actionId"]
        .as_str()
        .expect("chat-action-proposed trägt eine actionId")
        .parse()
        .expect("actionId ist eine UUID");
    confirmations
        .resolve(&id, app_logic::dto::ActionUserDecision::Approve)
        .expect("die vorgeschlagene Aktion wartet auf eine Bestätigung");
}

/// Spec 0067, A5 (Regressionstest): ist der erhöhte Dateibrowser-Kanal
/// aktiv, lesen und schreiben KI- und MCP-Aktionen trotzdem über den
/// NORMALEN Kanal — der erhöhte ist nur für Browser-Commands da.
#[tokio::test]
#[expect(clippy::large_futures, reason = "test future built inline; not boxed")]
async fn test_ai_and_mcp_file_actions_never_use_the_elevated_channel() {
    use ssh_manager_core::ai::AiEvent;
    use ssh_manager_core::profiles::AiAction;
    use ssh_manager_core::ssh::mock::MockSftpSession;

    use app_logic::confirmation::ConfirmationRegistry;
    use app_logic::orchestration::{handle_mcp_action_proposed, run_chat_turn};
    use app_logic::test_support::InMemoryProfileStore;

    use crate::test_support::waiting::{expect_within, render_events, NotifyingEmitter};

    for origin in ["ai", "mcp"] {
        let ai_events = if origin == "ai" {
            vec![
                AiEvent::ActionProposed(AiAction::ReadRemoteFile {
                    path: "/etc/secret.conf".to_string(),
                }),
                AiEvent::Done,
            ]
        } else {
            vec![AiEvent::Done]
        };
        let mut session = app_logic::test_support::session_with_ai_and_transport(
            ProposeOnceAiProvider::new(ai_events),
            Box::new(NoTransport),
        );
        session.set_filter_engine_for_tests(Box::new(ssh_manager_core::filter::FilterEngine::new(
            AllowEverythingPolicyStore,
        )));
        let normal = MockSftpSession::new().with_file("/etc/secret.conf", b"USER-VIEW".to_vec());
        let elevated = MockSftpSession::new().with_file("/etc/secret.conf", b"ROOT-VIEW".to_vec());
        session.set_sftp_for_tests(Box::new(normal.clone())).await;
        // Spec 0084, A1: Der erhöhte Kanal hängt nicht mehr an der
        // `Session`, sondern an der Zuordnung in `app-shell`. Er wird
        // hier trotzdem aufgebaut — genau darum geht es: er ist aktiv,
        // und KI und MCP kommen trotzdem nicht an ihn heran.
        let session_id = SessionId::new_v4();
        let elevated_registry = registry_with_channel(session_id, "root", elevated.clone());

        let emitter = NotifyingEmitter::default();
        let profile_store = InMemoryProfileStore::default();
        let confirmations = ConfirmationRegistry::new();
        if origin == "ai" {
            // Spec 0085, A5: auch der KI-Zweig steht unter der
            // Obergrenze — ohne sie würde ein künftig auf Bestätigung
            // wartender KI-Zweig bis zu `PENDING_ACTION_CONFIRM_TIMEOUT`
            // (3600 s) hängen, statt sichtbar zu scheitern.
            expect_within(
                "KI-Zweig: der Turn endet nach dem Lese-Ergebnis",
                || render_events(&emitter.inner),
                run_chat_turn(
                    &session,
                    session_id,
                    &emitter,
                    &profile_store,
                    &confirmations,
                ),
            )
            .await;
        } else {
            // MCP verlangt immer eine Bestätigung — hier genehmigt,
            // sobald sie angefragt ist, damit die Aktion wirklich
            // ausgeführt wird.
            let action = handle_mcp_action_proposed(
                &session,
                session_id,
                AiAction::ReadRemoteFile {
                    path: "/etc/secret.conf".to_string(),
                },
                &emitter,
                &profile_store,
                &confirmations,
                Some("test-client".to_string()),
            );
            expect_within(
                "MCP-Zweig: die Aktion endet nach der Bestätigung",
                || render_events(&emitter.inner),
                async { tokio::join!(action, approve_first_proposal(&emitter, &confirmations)) },
            )
            .await;
        }

        let events = emitter.inner.events.lock().unwrap().clone();
        let result = events
            .iter()
            .find(|(name, _)| name == "chat-action-result")
            .unwrap_or_else(|| panic!("{origin}: kein Ergebnis-Event"));
        let content = result.1["result"]["content"].as_str().unwrap();
        assert!(content.contains("USER-VIEW"), "{origin}: {content}");
        assert!(!content.contains("ROOT-VIEW"), "{origin}: {content}");
        assert!(
            elevated.calls().is_empty(),
            "{origin}: der erhöhte Kanal darf nie berührt werden, war: {:?}",
            elevated.calls()
        );
        assert!(
            !normal.calls().is_empty(),
            "{origin}: normaler Kanal wurde benutzt"
        );
        assert!(
            elevated_registry
                .slot(session_id, &BrowserAccess(()))
                .is_some(),
            "{origin}: Vorbedingung — der erhöhte Kanal war die ganze Zeit aktiv"
        );
    }
}

/// Spec 0084, T5b (Gegenstück zu T5 für eine **Schreib**aktion): Auch
/// eine MCP-Schreibaktion läuft bei aktivem erhöhtem Kanal über den
/// NORMALEN Kanal — sie darf die Datei nie mit den Rechten des erhöhten
/// Kanals schreiben.
#[tokio::test]
#[expect(clippy::large_futures, reason = "test future built inline; not boxed")]
async fn test_mcp_write_actions_never_use_the_elevated_channel() {
    use ssh_manager_core::ai::AiEvent;
    use ssh_manager_core::profiles::AiAction;
    use ssh_manager_core::ssh::mock::MockSftpSession;

    use app_logic::confirmation::ConfirmationRegistry;
    use app_logic::orchestration::handle_mcp_action_proposed;
    use app_logic::test_support::InMemoryProfileStore;

    use crate::test_support::waiting::{expect_within, render_events, NotifyingEmitter};

    let mut session = app_logic::test_support::session_with_ai_and_transport(
        crate::test_support::MockAiProvider::new(vec![AiEvent::Done]),
        Box::new(NoTransport),
    );
    session.set_filter_engine_for_tests(Box::new(ssh_manager_core::filter::FilterEngine::new(
        AllowEverythingPolicyStore,
    )));
    let normal = MockSftpSession::new().with_file("/etc/secret.conf", b"USER-VIEW".to_vec());
    let elevated = MockSftpSession::new().with_file("/etc/secret.conf", b"ROOT-VIEW".to_vec());
    session.set_sftp_for_tests(Box::new(normal.clone())).await;

    let session_id = SessionId::new_v4();
    let elevated_registry = registry_with_channel(session_id, "root", elevated.clone());

    let emitter = NotifyingEmitter::default();
    let profile_store = InMemoryProfileStore::default();
    let confirmations = ConfirmationRegistry::new();
    let action = handle_mcp_action_proposed(
        &session,
        session_id,
        AiAction::WriteRemoteFile {
            path: "/etc/secret.conf".to_string(),
            content: "MCP-WRITE".to_string(),
        },
        &emitter,
        &profile_store,
        &confirmations,
        Some("test-client".to_string()),
    );
    expect_within(
        "MCP-Schreibaktion endet nach der Bestätigung",
        || render_events(&emitter.inner),
        async { tokio::join!(action, approve_first_proposal(&emitter, &confirmations)) },
    )
    .await;

    assert!(
        elevated.calls().is_empty(),
        "der erhöhte Kanal darf beim Schreiben nie berührt werden, war: {:?}",
        elevated.calls()
    );
    assert_eq!(
        normal.file_content("/etc/secret.conf").as_deref(),
        Some(b"MCP-WRITE".as_slice()),
        "geschrieben wurde über den normalen Kanal"
    );
    assert_eq!(
        elevated.file_content("/etc/secret.conf").as_deref(),
        Some(b"ROOT-VIEW".as_slice()),
        "die Datei hinter dem erhöhten Kanal bleibt unverändert"
    );
    assert!(
        elevated_registry
            .slot(session_id, &BrowserAccess(()))
            .is_some(),
        "Vorbedingung — der erhöhte Kanal war die ganze Zeit aktiv"
    );
}
