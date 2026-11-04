use std::sync::Arc;

use super::*;
use crate::test_support::elevation::{access, fixture, output, transport, working_probes, PATH};
// Issue #149: Die Verschränkungstests T8/T8b warten auf Ereignisse, nicht
// auf ein festes Zeitbudget. `HANG_GUARD` greift nur, wenn das Ereignis
// nie kommt, und die Meldung nennt dann, worauf der Test wartete.
use crate::test_support::waiting::{expect_within, HANG_GUARD};

#[tokio::test]
async fn test_enable_probes_path_checks_sudo_and_opens_exec_channel() {
    let (probe, check) = working_probes();
    let (t, log) = transport(probe, check);
    let f = fixture(Box::new(t));

    let result = enable(&f.ctx(), "deploy", None, None, false, &access())
        .await
        .expect("Sitzung steht, das Aktivieren darf nicht mit Err enden");

    assert!(result.active, "{result:?}");
    assert_eq!(result.target_user, "root");
    assert_eq!(result.sftp_server_path.as_deref(), Some(PATH));
    let log = log.lock().unwrap().clone();
    assert_eq!(log.last().unwrap(), &format!("sftp-exec:sudo -n {PATH}"));
    assert!(log
        .iter()
        .any(|c| c == &format!("exec:env LC_ALL=C sudo -n -l {PATH}")));
    assert!(f.is_active().await);
}

#[tokio::test]
async fn test_password_required_yields_tailored_sudoers_line_and_no_channel() {
    let (t, log) = transport(
        output(0, "/usr/lib/openssh/sftp-server\n", ""),
        output(1, "", "sudo: a password is required\n"),
    );
    let f = fixture(Box::new(t));

    let result = enable(&f.ctx(), "deploy", None, None, false, &access())
        .await
        .expect("ein gescheiterter Versuch ist ein Ergebnis, kein Err");

    assert!(!result.active);
    let failure = result.failure.unwrap();
    assert_eq!(failure.kind, ElevationFailureKind::PasswordRequired);
    assert_eq!(
        failure.sudoers_line.as_deref(),
        Some("deploy ALL=(root) NOPASSWD: /usr/lib/openssh/sftp-server \"\"")
    );
    assert!(!log
        .lock()
        .unwrap()
        .iter()
        .any(|c| c.starts_with("sftp-exec:")));
    assert!(!f.is_active().await);
}

fn unsafe_outputs() -> Vec<ssh_manager_core::ssh::CommandOutput> {
    vec![
        output(0, &format!("UNSAFE {PATH}\n"), ""),
        output(0, "garbage", ""),
        output(4, "", "find: no such file"),
    ]
}

#[tokio::test]
async fn test_unsafe_binary_with_password_required_withholds_sudoers_line() {
    for safety in unsafe_outputs() {
        let (mut t, log) = transport(
            output(0, "/usr/lib/openssh/sftp-server\n", ""),
            output(1, "", "sudo: a password is required\n"),
        );
        t.safety = safety;
        let f = fixture(Box::new(t));

        let result = enable(&f.ctx(), "deploy", None, None, false, &access())
            .await
            .expect("ein gescheiterter Versuch ist ein Ergebnis, kein Err");

        assert!(!result.active);
        let failure = result.failure.unwrap();
        assert_eq!(failure.kind, ElevationFailureKind::SftpServerUnsafe);
        assert_eq!(failure.sudoers_line, None);
        assert_eq!(failure.detail.as_deref(), Some(PATH));
        assert!(!log
            .lock()
            .unwrap()
            .iter()
            .any(|c| c.starts_with("sftp-exec:")));
        assert!(!f.is_active().await);
    }
}

#[tokio::test]
async fn test_unsafe_binary_with_sudo_not_allowed_withholds_sudoers_line() {
    let (mut t, _log) = transport(
        output(0, "/usr/lib/openssh/sftp-server\n", ""),
        output(1, "", "Sorry, user deploy is not allowed to execute it.\n"),
    );
    t.safety = output(0, "UNSAFE /usr/lib/openssh\n", "");
    let f = fixture(Box::new(t));
    let result = enable(&f.ctx(), "deploy", None, None, false, &access())
        .await
        .unwrap();
    let failure = result.failure.unwrap();
    assert_eq!(failure.kind, ElevationFailureKind::SftpServerUnsafe);
    assert_eq!(failure.sudoers_line, None);
    assert_eq!(failure.detail.as_deref(), Some("/usr/lib/openssh"));
}

#[tokio::test]
async fn test_unsafe_binary_with_sudo_allowed_starts_channel_with_warning() {
    for safety in unsafe_outputs() {
        let (probe, check) = working_probes();
        let (mut t, log) = transport(probe, check);
        t.safety = safety;
        let f = fixture(Box::new(t));

        let result = enable(&f.ctx(), "deploy", None, None, false, &access())
            .await
            .unwrap();

        assert!(result.active, "{result:?}");
        let warning = result.warning.expect("Warnung bei unsicherem Binary");
        assert_eq!(warning.kind, ElevationWarningKind::SftpServerUnsafe);
        assert_eq!(warning.path, PATH);
        assert!(log
            .lock()
            .unwrap()
            .iter()
            .any(|c| c.starts_with("sftp-exec:")));
        assert!(f.is_active().await);
    }
}

#[tokio::test]
async fn test_safe_binary_has_no_warning_and_probe_runs_before_sudo_check() {
    let (probe, check) = working_probes();
    let (t, log) = transport(probe, check);
    let f = fixture(Box::new(t));
    let result = enable(&f.ctx(), "deploy", None, None, false, &access())
        .await
        .unwrap();
    assert!(result.active && result.warning.is_none(), "{result:?}");
    let log = log.lock().unwrap().clone();
    let pos = |needle: &str| log.iter().position(|c| c.contains(needle)).unwrap();
    assert!(pos("-prune") < pos("sudo -n -l"));
    // Der Safety-Probe läuft nie mit sudo.
    assert!(!log[pos("-prune")].contains("sudo"));
}

#[tokio::test]
async fn test_each_prerequisite_failure_is_named() {
    for (probe, check, expected) in [
        (
            output(3, "", ""),
            output(0, "", ""),
            ElevationFailureKind::SftpServerNotFound,
        ),
        (
            output(0, "/usr/lib/openssh/sftp-server\n", ""),
            output(1, "", "sudo: sorry, you must have a tty to run sudo\n"),
            ElevationFailureKind::RequireTty,
        ),
        (
            output(0, "/usr/lib/openssh/sftp-server\n", ""),
            output(127, "", "sh: 1: sudo: not found\n"),
            ElevationFailureKind::SudoMissing,
        ),
    ] {
        let (t, _log) = transport(probe, check);
        let f = fixture(Box::new(t));
        let result = enable(&f.ctx(), "deploy", None, None, false, &access())
            .await
            .expect("ein gescheiterter Versuch ist ein Ergebnis, kein Err");
        assert_eq!(result.failure.map(|f| f.kind), Some(expected));
    }
}

#[tokio::test]
async fn test_override_path_skips_probe_and_invalid_override_runs_nothing() {
    let (t, log) = transport(output(3, "", ""), output(0, "", ""));
    let f = fixture(Box::new(t));
    let result = enable(
        &f.ctx(),
        "deploy",
        Some("/opt/ssh/sftp-server"),
        Some("www-data"),
        false,
        &access(),
    )
    .await
    .expect("Sitzung steht, das Aktivieren darf nicht mit Err enden");
    assert!(result.active, "{result:?}");
    let log = log.lock().unwrap().clone();
    assert!(
        !log.iter().any(|c| c.contains("sshd_config")),
        "keine Probe bei Override"
    );
    assert_eq!(
        log.last().unwrap(),
        "sftp-exec:sudo -n -u www-data /opt/ssh/sftp-server"
    );

    let (t, log) = transport(output(0, "", ""), output(0, "", ""));
    let f = fixture(Box::new(t));
    let result = enable(
        &f.ctx(),
        "deploy",
        Some("/opt/x; reboot"),
        None,
        false,
        &access(),
    )
    .await
    .expect("ein gescheiterter Versuch ist ein Ergebnis, kein Err");
    assert_eq!(
        result.failure.map(|f| f.kind),
        Some(ElevationFailureKind::InvalidPath)
    );
    assert!(
        log.lock().unwrap().is_empty(),
        "nichts darf ausgeführt werden"
    );
}

#[tokio::test]
async fn test_start_failure_is_reported_and_leaves_no_channel() {
    let (probe, check) = working_probes();
    let (mut t, _log) = transport(probe, check);
    t.start_fails = true;
    let f = fixture(Box::new(t));

    let result = enable(&f.ctx(), "deploy", None, None, false, &access())
        .await
        .expect("ein gescheiterter Versuch ist ein Ergebnis, kein Err");

    assert_eq!(
        result.failure.map(|f| f.kind),
        Some(ElevationFailureKind::StartFailed)
    );
    assert!(!f.is_active().await);
}

// --- Spec 0084, A2: kein erhöhter Kanal überlebt seine Sitzung ---------

/// Spec 0084, T6: Ist ein Kanal aktiv und wird die Sitzung über die eine
/// Funktion aus A2.1 entfernt, bleibt kein Eintrag in der Zuordnung
/// zurück. Scheitert, wenn diese Funktion den Eintrag nicht löscht.
#[tokio::test]
async fn test_t6_removing_a_session_also_removes_its_elevated_channel() {
    let (probe, check) = working_probes();
    let (t, _log) = transport(probe, check);
    let f = fixture(Box::new(t));
    enable(&f.ctx(), "deploy", None, None, false, &access())
        .await
        .expect("Vorbedingung: Aktivieren gelingt");
    assert!(f.is_active().await, "Vorbedingung: Kanal ist aktiv");

    let removed = f.registry.remove_session(&f.sessions, f.session_id);

    assert!(removed.is_some(), "die Sitzung selbst muss entfernt sein");
    assert!(
        f.registry.slot(f.session_id, &access()).is_none(),
        "A2.1: der erhöhte Kanal darf seine Sitzung nicht überleben"
    );
}

/// Issue #657: Several sessions to one server are independent — an active
/// elevated channel belongs to its session only, and revoking another
/// session leaves it alone.
#[tokio::test]
async fn test_elevated_channel_of_one_session_is_invisible_to_another() {
    let (probe, check) = working_probes();
    let (t, _log) = transport(probe, check);
    let f = fixture(Box::new(t));
    enable(&f.ctx(), "deploy", None, None, false, &access())
        .await
        .expect("precondition: enabling succeeds");
    let other = SessionId::new_v4();

    assert!(f.registry.slot(other, &access()).is_none());
    f.registry.revoke_session(other);
    assert!(
        f.registry.slot(f.session_id, &access()).is_some(),
        "revoking another session must not touch this session's channel"
    );
}

/// Spec 0084, T8: Das Aktivieren steht nach den Proben, unmittelbar vor
/// dem Eintrag, still; in diesem Moment wird die Sitzung über A2.1
/// entfernt. Danach läuft das Aktivieren weiter und muss mit dem
/// bestehenden Fehler enden, ohne einen Eintrag zu hinterlassen.
/// Scheitert, wenn die Prüfung aus A2.2 fehlt.
#[tokio::test]
async fn test_t8_activation_finishing_after_the_session_was_removed_is_rejected() {
    let reached = Arc::new(tokio::sync::Notify::new());
    let gate = Arc::new(tokio::sync::Notify::new());
    let (probe, check) = working_probes();
    let (mut t, _log) = transport(probe, check);
    t.open_reached = Some(reached.clone());
    t.open_gate = Some(gate.clone());
    let f = Arc::new(fixture(Box::new(t)));

    let f_for_enable = f.clone();
    let activation = tokio::spawn(async move {
        enable(&f_for_enable.ctx(), "deploy", None, None, false, &access()).await
    });

    // Warten, bis das Aktivieren wirklich vor dem Eintrag steht — sonst
    // prüfte der Test eine andere Verschränkung als gemeint.
    expect_within(
        "das Aktivieren erreicht den Öffnen-Schritt",
        || format!("Aktivierungs-Task beendet: {}", activation.is_finished()),
        reached.notified(),
    )
    .await;

    f.registry.remove_session(&f.sessions, f.session_id);
    gate.notify_one();

    let result = expect_within(
        "das Aktivieren endet nach dem Entfernen der Sitzung",
        || "Aktivierung hängt nach der Freigabe am Öffnen-Schritt".to_string(),
        activation,
    )
    .await
    .expect("der Aktivierungs-Task darf nicht panisch enden");

    let err = result.expect_err("A2.2: ohne Sitzung darf nicht aktiviert werden");
    assert_eq!(err.message, "session not found");
    assert_eq!(err.code, Some(app_logic::error::SESSION_NOT_FOUND));
    assert!(
        f.registry.slot(f.session_id, &access()).is_none(),
        "A2.2: kein Eintrag für eine bereits entfernte Sitzung"
    );
}

/// Spec 0084, T8b: dieselbe Verschränkung, aber das Aktivieren läuft
/// vollständig durch, während A2.1 zwischen `SessionManager::remove` und
/// dem Löschen des Zuordnungs-Eintrags steht (Haltepunkt
/// `interleave_hook`). Scheitert bei falscher Reihenfolge in A2.1:
/// löschte A2.1 erst den Eintrag und entfernte danach die Sitzung, sähe
/// das Aktivieren die Sitzung noch, trüge den Kanal ein — und der
/// Eintrag überlebte die Sitzung.
#[tokio::test]
async fn test_t8b_activation_between_the_two_removal_steps_leaves_no_entry() {
    let reached = Arc::new(tokio::sync::Notify::new());
    let gate = Arc::new(tokio::sync::Notify::new());
    let (probe, check) = working_probes();
    let (mut t, _log) = transport(probe, check);
    t.open_reached = Some(reached.clone());
    t.open_gate = Some(gate.clone());
    let f = Arc::new(fixture(Box::new(t)));

    let f_for_enable = f.clone();
    let activation = tokio::spawn(async move {
        enable(&f_for_enable.ctx(), "deploy", None, None, false, &access()).await
    });
    expect_within(
        "das Aktivieren erreicht den Öffnen-Schritt",
        || format!("Aktivierungs-Task beendet: {}", activation.is_finished()),
        reached.notified(),
    )
    .await;

    // Der Haltepunkt läuft mitten in A2.1: er gibt das Aktivieren frei
    // und wartet, bis es ganz durch ist, bevor A2.1 seinen zweiten
    // Schritt macht.
    let (done_tx, done_rx) = std::sync::mpsc::channel::<()>();
    let gate_for_hook = gate.clone();
    f.registry.set_interleave_hook(Box::new(move || {
        gate_for_hook.notify_one();
        // Synchroner Haltepunkt: `expect_within` ist hier nicht nutzbar,
        // daher dieselbe Obergrenze direkt.
        if let Err(err) = done_rx.recv_timeout(HANG_GUARD) {
            panic!(
                "Haltepunkt in A2.1: Warten auf erwartetes Ereignis \
                 „das Aktivieren endet innerhalb des Haltepunkts“ \
                 erfolglos nach {} s ({err})",
                HANG_GUARD.as_secs()
            );
        }
    }));

    // `spawn_blocking`: Der Haltepunkt blockiert seinen Thread, bis das
    // Aktivieren durch ist — auf einem Scheduler-Thread käme das
    // Aktivieren dann nie zum Zug.
    let f_for_removal = f.clone();
    let removal = tokio::task::spawn_blocking(move || {
        f_for_removal
            .registry
            .remove_session(&f_for_removal.sessions, f_for_removal.session_id)
    });

    let result = expect_within(
        "das Aktivieren endet, während A2.1 im Haltepunkt steht",
        || format!("Entfernen-Task beendet: {}", removal.is_finished()),
        activation,
    )
    .await
    .expect("der Aktivierungs-Task darf nicht panisch enden");
    let _ = done_tx.send(());
    expect_within(
        "das Entfernen der Sitzung (A2.1) endet",
        || "A2.1 hat den Haltepunkt nicht verlassen".to_string(),
        removal,
    )
    .await
    .expect("der Entfernen-Task darf nicht panisch enden");

    assert!(
        result.is_err(),
        "A2.1 entfernt die Sitzung zuerst — danach darf nicht mehr aktiviert werden"
    );
    assert!(
        f.registry.slot(f.session_id, &access()).is_none(),
        "A2.1/A2.2: kein Eintrag darf die Sitzung überleben"
    );
}

// --- Spec 0085, A2.2: Ausschalten verwirft den Kanal wirklich ----------

/// Meldet sein eigenes Verworfenwerden. Alle Trait-Methoden sind
/// `unreachable!` — T15 benutzt den Kanal nie, ein versehentlicher
/// Zugriff soll sofort auffallen statt still zu gelingen.
struct DropReportingSftp(Arc<std::sync::atomic::AtomicBool>);

impl Drop for DropReportingSftp {
    fn drop(&mut self) {
        self.0.store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

#[async_trait::async_trait]
impl SftpSession for DropReportingSftp {
    async fn list_dir(
        &mut self,
        _path: &str,
    ) -> Result<Vec<ssh_manager_core::ssh::RemoteEntry>, ssh_manager_core::ssh::SshError> {
        unreachable!("T15 benutzt den Kanal nie")
    }
    async fn read_file(&mut self, _path: &str) -> Result<Vec<u8>, ssh_manager_core::ssh::SshError> {
        unreachable!()
    }
    async fn read_file_capped(
        &mut self,
        _path: &str,
        _max_bytes: u64,
        _progress: &mut (dyn FnMut(u64) + Send),
    ) -> Result<Vec<u8>, ssh_manager_core::ssh::SshError> {
        unreachable!()
    }
    async fn write_file(
        &mut self,
        _path: &str,
        _content: &[u8],
    ) -> Result<(), ssh_manager_core::ssh::SshError> {
        unreachable!()
    }
    async fn read_file_chunked(
        &mut self,
        _path: &str,
        _chunk_size: usize,
        _on_chunk: &mut (dyn for<'a> FnMut(&'a [u8]) -> ssh_manager_core::ssh::ChunkControl + Send),
    ) -> Result<ssh_manager_core::ssh::TransferEnd, ssh_manager_core::ssh::SshError> {
        unreachable!("dieser Test lädt nichts herunter")
    }

    async fn write_file_chunked(
        &mut self,
        _path: &str,
        _chunk_size: usize,
        _next_chunk: &mut (dyn for<'a> FnMut(&'a mut [u8]) -> ssh_manager_core::ssh::ChunkSource
                  + Send),
    ) -> Result<ssh_manager_core::ssh::TransferEnd, ssh_manager_core::ssh::SshError> {
        unreachable!("dieser Test lädt nichts hoch")
    }

    async fn read_range(
        &mut self,
        _path: &str,
        _offset: u64,
        _len: usize,
    ) -> Result<Vec<u8>, ssh_manager_core::ssh::SshError> {
        unreachable!("this test does not transfer files")
    }

    async fn write_range(
        &mut self,
        _path: &str,
        _offset: u64,
        _data: &[u8],
        _truncate: bool,
    ) -> Result<(), ssh_manager_core::ssh::SshError> {
        unreachable!("this test does not transfer files")
    }

    async fn stat(
        &mut self,
        _path: &str,
    ) -> Result<ssh_manager_core::ssh::RemoteEntry, ssh_manager_core::ssh::SshError> {
        unreachable!()
    }
    async fn lstat(
        &mut self,
        _path: &str,
    ) -> Result<ssh_manager_core::ssh::RemoteEntry, ssh_manager_core::ssh::SshError> {
        unreachable!()
    }
    async fn remove(&mut self, _path: &str) -> Result<(), ssh_manager_core::ssh::SshError> {
        unreachable!()
    }
    async fn rename(
        &mut self,
        _from: &str,
        _to: &str,
    ) -> Result<(), ssh_manager_core::ssh::SshError> {
        unreachable!()
    }
    async fn posix_rename(
        &mut self,
        _from: &str,
        _to: &str,
    ) -> Result<bool, ssh_manager_core::ssh::SshError> {
        unreachable!()
    }
    async fn create_dir(&mut self, _path: &str) -> Result<(), ssh_manager_core::ssh::SshError> {
        unreachable!()
    }
    async fn create_file_exclusive(
        &mut self,
        _path: &str,
    ) -> Result<(), ssh_manager_core::ssh::SshError> {
        unreachable!()
    }
    async fn remove_dir(&mut self, _path: &str) -> Result<(), ssh_manager_core::ssh::SshError> {
        unreachable!()
    }
    async fn set_permissions(
        &mut self,
        _path: &str,
        _mode: u32,
    ) -> Result<(), ssh_manager_core::ssh::SshError> {
        unreachable!()
    }
    async fn set_owner(
        &mut self,
        _path: &str,
        _uid: u32,
        _gid: u32,
    ) -> Result<(), ssh_manager_core::ssh::SshError> {
        unreachable!()
    }
}

/// Spec 0085, T15 (A2.2): Nach dem Ausschalten hält **kein** Zustand den
/// Kanalwert mehr — auch dann nicht, wenn noch eine Kopie des Slots lebt,
/// wie sie ein wartender Browser-Befehl mitführt
/// (`BrowserChannel::from_request` klont den Slot).
///
/// Der Merker allein genügt dafür nicht: Solange der Kanalwert irgendwo
/// weiterlebt, bleibt auf dem Server `sudo -n <sftp-server>` am Leben
/// (T14 zeigt, dass erst das Verwerfen ihn beendet). „Ausgeschaltet" wäre
/// dann nur eine Behauptung der App.
///
/// Gegenbeweis (belegt): Verwirft `revoke` den Kanalwert nicht, sondern
/// setzt nur den Merker, scheitert dieser Test.
#[tokio::test]
async fn test_t15_disabling_drops_the_channel_even_while_a_command_holds_the_slot() {
    let dropped = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (probe, check) = working_probes();
    let (t, _log) = transport(probe, check);
    let f = fixture(Box::new(t));
    f.registry.insert_for_tests(
        f.session_id,
        ElevatedSftp {
            target_user: "root".to_string(),
            sftp: Box::new(DropReportingSftp(dropped.clone())),
        },
    );

    // Die Kopie, die ein laufender oder wartender Befehl festhält.
    let held = f
        .registry
        .slot(f.session_id, &access())
        .expect("Vorbedingung: Kanal aktiv");

    disable(&f.sessions, &f.registry, f.session_id, false, &access())
        .await
        .expect("Ausschalten gelingt");

    assert!(
        dropped.load(std::sync::atomic::Ordering::SeqCst),
        "A2.2: der Kanalwert muss beim Ausschalten verworfen werden, nicht nur als \
         widerrufen markiert — sonst lebt der sudo-Prozess auf dem Server weiter"
    );
    assert!(
        held.is_revoked(),
        "und die festgehaltene Kopie ist widerrufen"
    );
    assert!(
        held.lock_owned(&access()).await.is_none(),
        "der Slot ist leer, obwohl diese Kopie noch lebt"
    );
}

/// Spec 0084, T9 (mit der Klarstellung vom 2026-09-28): Ausschalten ohne
/// aktiven Kanal ist ein harmloses `Ok`; für eine **unbekannte**
/// Sitzungskennung bleibt es wie bisher bei `Err("Session nicht
/// gefunden")`. In keinem Fall ein Absturz.
#[tokio::test]
async fn test_t9_disabling_an_inactive_or_unknown_session_channel_is_harmless() {
    let (probe, check) = working_probes();
    let (t, _log) = transport(probe, check);
    let f = fixture(Box::new(t));

    disable(&f.sessions, &f.registry, f.session_id, false, &access())
        .await
        .expect("bekannte Sitzung ohne Kanal: „nicht aktiv“ ist kein Fehler");
    assert!(!f.is_active().await);

    let err = disable(
        &f.sessions,
        &f.registry,
        SessionId::new_v4(),
        false,
        &access(),
    )
    .await
    .expect_err("unbekannte Sitzungskennung meldet wie bisher einen Fehler");
    assert_eq!(err.message, "session not found");
    assert_eq!(err.code, Some(app_logic::error::SESSION_NOT_FOUND));
    assert!(!f.is_active().await);
}
