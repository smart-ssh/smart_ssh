//! Issue #622: chunks, progress, stop and confirmation before stopping (split from the parent file for size).
use super::*;

// --- Issue #622: Stücke, Fortschritt, Abbruch, Bestätigung vor dem Stoppen --

const CHUNK: usize = crate::file_ops::TRANSFER_CHUNK_BYTES;

/// Eine Datei aus drei Stücken (das letzte nur halb), mit stückabhängigem Muster.
fn three_chunk_content() -> Vec<u8> {
    (0..(CHUNK * 2 + CHUNK / 2))
        .map(|i| ((i / 1000) % 251) as u8)
        .collect()
}

fn events_named(emitter: &app_logic::events::TestEmitter, name: &str) -> Vec<serde_json::Value> {
    emitter
        .events
        .lock()
        .unwrap()
        .iter()
        .filter(|(n, _)| n == name)
        .map(|(_, p)| p.clone())
        .collect()
}

fn spawn_download(
    s: &Setup,
    emitter: &Arc<app_logic::events::TestEmitter>,
    remote: &'static str,
    local: std::path::PathBuf,
    total: Option<u64>,
) -> tokio::task::JoinHandle<CommandResult<()>> {
    let em = emitter.clone();
    let session_id = s.session_id;
    spawn_command(
        s.session.clone(),
        s.elevated_channel(),
        move |session, channel| async move {
            download_one_file(
                em.as_ref(),
                &session,
                &channel,
                session_id,
                remote,
                local,
                total,
            )
            .await
        },
    )
}

fn spawn_upload(
    s: &Setup,
    emitter: &Arc<app_logic::events::TestEmitter>,
    local: std::path::PathBuf,
    remote: &'static str,
) -> tokio::task::JoinHandle<CommandResult<()>> {
    let em = emitter.clone();
    let session_id = s.session_id;
    let granted = app_logic::local_path_grants::GrantedLocalPath::unchecked_for_tests(local);
    spawn_command(
        s.session.clone(),
        s.elevated_channel(),
        move |session, channel| async move {
            upload_impl(
                em.as_ref(),
                &session,
                &channel,
                session_id,
                &granted,
                remote,
            )
            .await
        },
    )
}

/// Ausschalten mit Bestätigung in eigenem Task; wartet, bis die Stopp-Marke
/// gesetzt ist (erst dann darf der Test den Mock freigeben).
async fn confirmed_disable(
    s: &Setup,
    elevated: &GatedSftp,
) -> tokio::task::JoinHandle<CommandResult<app_logic::dto::ElevationDisableDto>> {
    let (sessions, registry, session_id) = (s.sessions.clone(), s.registry.clone(), s.session_id);
    let task = tokio::spawn(async move {
        crate::elevated_sftp::disable(
            &sessions,
            &registry,
            session_id,
            true,
            &crate::test_support::elevation::access(),
        )
        .await
    });
    let registry = s.registry.clone();
    let session_id = s.session_id;
    expect_within(
        "die laufende Operation bekommt die Stopp-Marke",
        || elevated.describe(),
        async move {
            while !registry.ops().has_stopped_elevated(session_id) {
                tokio::task::yield_now().await;
            }
        },
    )
    .await;
    task
}

fn part_files(dir: &std::path::Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect()
}

/// Acceptance: > 100 MB laden, ohne die Datei im Speicher zu halten; die
/// Fortschrittsereignisse steigen und sind gedrosselt.
#[tokio::test]
async fn test_622_a_download_over_100_mb_reads_in_bounded_chunks() {
    let elevated = GatedSftp::small_tree(0);
    let size: u64 = 101 * 1024 * 1024;
    elevated.set_synthetic("/t/big.bin", size);
    let s = setup(Box::new(elevated.clone())).await;
    let emitter = Arc::new(app_logic::events::TestEmitter::default());
    let dir = tempfile::tempdir().unwrap();
    let local = dir.path().join("big.bin");

    let started = std::time::Instant::now();
    let result = spawn_download(&s, &emitter, "/t/big.bin", local.clone(), Some(size));
    join(result, &elevated).await.expect("Download gelingt");
    let elapsed = started.elapsed();

    assert_eq!(std::fs::metadata(&local).unwrap().len(), size);
    assert_eq!(elevated.chunks(), size.div_ceil(CHUNK as u64) as usize);
    assert!(
        elevated.max_chunk() <= CHUNK,
        "ein Stück war größer als die Obergrenze: {}",
        elevated.max_chunk()
    );
    assert!(
        !elevated.ops().iter().any(|o| o.starts_with("read_file ")),
        "kein Lesen der ganzen Datei, war: {:?}",
        elevated.ops()
    );
    assert_eq!(part_files(dir.path()), vec!["big.bin".to_string()]);

    let progress = events_named(&emitter, "sftp-transfer-progress");
    let done: Vec<u64> = progress
        .iter()
        .map(|p| p["bytesDone"].as_u64().unwrap())
        .collect();
    assert!(!done.is_empty(), "mindestens ein Fortschrittsereignis");
    assert!(
        done.windows(2).all(|w| w[0] < w[1]),
        "streng steigend: {done:?}"
    );
    assert!(*done.last().unwrap() <= size);
    assert!(
        done.len() as f64 <= elapsed.as_secs_f64() * 4.0 + 1.0,
        "höchstens ca. 4 Ereignisse pro Sekunde: {} in {:?}",
        done.len(),
        elapsed
    );
    assert_eq!(progress[0]["totalBytes"].as_u64(), Some(size));
}

/// Acceptance: ohne laufende Operation schaltet das Ausschalten sofort ab.
#[tokio::test]
async fn test_622_disabling_without_a_running_operation_switches_off_at_once() {
    let elevated = GatedSftp::small_tree(0);
    let s = setup(Box::new(elevated.clone())).await;
    let slot = s.slot();

    let result = crate::elevated_sftp::disable(
        &s.sessions,
        &s.registry,
        s.session_id,
        false,
        &crate::test_support::elevation::access(),
    )
    .await
    .expect("Ausschalten gelingt");

    assert!(result.disabled);
    assert!(result.running_operations.is_empty());
    assert!(slot.is_revoked());
}

/// Acceptance: Ausschalten mit laufenden Operationen listet sie; „Abbrechen“
/// (= ohne Bestätigung) lässt den Modus an und alles läuft zu Ende.
#[tokio::test]
async fn test_622_unconfirmed_disable_lists_the_operations_and_changes_nothing() {
    let elevated = GatedSftp::small_tree(0);
    elevated.put_file("/t/a.txt", three_chunk_content());
    elevated.pause_after_chunk(1);
    let s = setup(Box::new(elevated.clone())).await;
    let slot = s.slot();
    let emitter = Arc::new(app_logic::events::TestEmitter::default());
    let dir = tempfile::tempdir().unwrap();
    let local = dir.path().join("a.txt");
    let download = spawn_download(&s, &emitter, "/t/a.txt", local.clone(), Some(1));
    elevated.wait_until_chunk_paused().await;

    // Löschen und chmod laufen als eingetragene Operationen mit.
    let channel = s.elevated_channel();
    let _delete = channel.register_op(
        uuid::Uuid::new_v4(),
        crate::file_ops::OpKind::Delete,
        "/t/old",
    );
    let _chmod = channel.register_op(
        uuid::Uuid::new_v4(),
        crate::file_ops::OpKind::Chmod,
        "/t/sub",
    );

    let access = crate::test_support::elevation::access();
    let result =
        crate::elevated_sftp::disable(&s.sessions, &s.registry, s.session_id, false, &access)
            .await
            .unwrap();
    assert!(!result.disabled);
    let mut kinds: Vec<_> = result
        .running_operations
        .iter()
        .map(|o| (o.kind, o.path.as_str()))
        .collect();
    kinds.sort();
    assert_eq!(
        kinds,
        vec![
            ("chmod", "/t/sub"),
            ("delete", "/t/old"),
            ("download", "/t/a.txt")
        ]
    );
    assert!(!slot.is_revoked(), "Abbrechen lässt den Modus an");

    elevated.release_chunk();
    join(download, &elevated)
        .await
        .expect("Download läuft zu Ende");
    assert_eq!(std::fs::read(&local).unwrap(), three_chunk_content());
    assert_eq!(elevated.chunks(), 3);
    assert!(!slot.is_revoked());
}

/// Acceptance: bestätigtes Stoppen — der Download endet nach einem Stück, die
/// Temp-Datei ist weg, die Zeile zeigt „stopped“ ohne Kanalfehler, danach ist
/// der Kanal zu und eine neue Operation scheitert (Adversarial).
#[tokio::test]
async fn test_622_confirmed_stop_ends_a_download_within_one_chunk_and_closes_the_channel() {
    let elevated = GatedSftp::small_tree(0);
    elevated.put_file("/t/a.txt", three_chunk_content());
    elevated.pause_after_chunk(1);
    let s = setup(Box::new(elevated.clone())).await;
    let slot = s.slot();
    let emitter = Arc::new(app_logic::events::TestEmitter::default());
    let dir = tempfile::tempdir().unwrap();
    let local = dir.path().join("a.txt");
    std::fs::write(&local, b"ALT").unwrap();
    let download = spawn_download(&s, &emitter, "/t/a.txt", local.clone(), None);
    elevated.wait_until_chunk_paused().await;
    assert_eq!(part_files(dir.path()).len(), 2, "Temp-Datei läuft mit");
    assert_eq!(std::fs::read(&local).unwrap(), b"ALT");

    let disable = confirmed_disable(&s, &elevated).await;
    elevated.release_chunk();

    let err = join(download, &elevated).await.expect_err("gestoppt");
    assert_eq!(
        err.code,
        Some(super::super::transfer::FILE_OPERATION_STOPPED)
    );
    let result = expect_within("das Ausschalten endet", || elevated.describe(), disable)
        .await
        .unwrap()
        .unwrap();
    assert!(result.disabled);
    assert!(slot.is_revoked());

    assert_eq!(elevated.chunks(), 1, "genau ein Stück, dann Schluss");
    assert_eq!(part_files(dir.path()), vec!["a.txt".to_string()]);
    assert_eq!(std::fs::read(&local).unwrap(), b"ALT", "alte Datei bleibt");
    let finished = events_named(&emitter, "sftp-transfer-finished");
    assert_eq!(finished.len(), 1);
    assert_eq!(finished[0]["stopped"], "stopped");
    assert!(finished[0]["error"].is_null(), "kein Kanalfehler");

    // Adversarial: nach dem Schließen läuft nichts mehr über den Kanal.
    let ops_before = elevated.op_count();
    let again = spawn_download(&s, &emitter, "/t/a.txt", dir.path().join("b.txt"), None);
    assert_inactive(join(again, &elevated).await);
    assert_eq!(elevated.op_count(), ops_before);
    assert_eq!(elevated.chunks(), 1);
}

/// Acceptance: bestätigtes Stoppen beendet ein rekursives Löschen am nächsten
/// Eintrag, dann ist der Kanal zu.
#[tokio::test]
async fn test_622_confirmed_stop_ends_a_recursive_delete_at_the_next_entry() {
    let elevated = GatedSftp::small_tree(5);
    let s = setup(Box::new(elevated.clone())).await;
    let slot = s.slot();
    let command = spawn_command(
        s.session.clone(),
        s.elevated_channel(),
        |session, channel| async move { delete_impl(&session, &channel, "/t").await },
    );
    elevated.wait_until_paused().await;
    let ops_before = elevated.op_count();

    let disable = confirmed_disable(&s, &elevated).await;
    elevated.release();

    let err = join(command, &elevated).await.expect_err("gestoppt");
    assert_eq!(
        err.code,
        Some(super::super::transfer::FILE_OPERATION_STOPPED)
    );
    expect_within("das Ausschalten endet", || elevated.describe(), disable)
        .await
        .unwrap()
        .unwrap();
    assert!(slot.is_revoked());
    assert_eq!(elevated.op_count(), ops_before, "kein weiterer Eintrag");
    assert!(elevated.exists("/t"));
}

/// Dasselbe für rekursives chmod.
#[tokio::test]
async fn test_622_confirmed_stop_ends_a_recursive_chmod_at_the_next_entry() {
    let elevated = GatedSftp::small_tree(5);
    let s = setup(Box::new(elevated.clone())).await;
    let slot = s.slot();
    let command = spawn_command(
        s.session.clone(),
        s.elevated_channel(),
        |session, channel| async move { chmod_impl(&session, &channel, "/t", 0o700, true, false).await },
    );
    elevated.wait_until_paused().await;
    let ops_before = elevated.op_count();

    let disable = confirmed_disable(&s, &elevated).await;
    elevated.release();

    let err = join(command, &elevated).await.expect_err("gestoppt");
    assert_eq!(
        err.code,
        Some(super::super::transfer::FILE_OPERATION_STOPPED)
    );
    expect_within("das Ausschalten endet", || elevated.describe(), disable)
        .await
        .unwrap()
        .unwrap();
    assert!(slot.is_revoked());
    assert_eq!(elevated.op_count(), ops_before);
}

/// Acceptance: ein Nutzerwechsel mit laufender Operation zeigt dieselbe Liste
/// und ändert nichts, solange nicht bestätigt wird.
#[tokio::test]
async fn test_622_changing_the_target_user_while_an_operation_runs_asks_first() {
    let elevated = GatedSftp::small_tree(0);
    let f = crate::test_support::elevation::fixture_with_channels(vec![Box::new(elevated.clone())]);
    f.session
        .set_sftp_for_tests(Box::new(MockSftpSession::new()))
        .await;
    crate::elevated_sftp::enable(
        &f.ctx(),
        "deploy",
        None,
        None,
        false,
        &crate::test_support::elevation::access(),
    )
    .await
    .expect("Vorbedingung: Aktivieren gelingt");
    let slot = f
        .registry
        .slot(f.session_id, &crate::test_support::elevation::access())
        .unwrap();
    let channel = BrowserChannel::for_tests(&f.registry, f.session_id, Some("root".to_string()));
    let _op = channel.register_op(uuid::Uuid::new_v4(), crate::file_ops::OpKind::Delete, "/t");

    let result = crate::elevated_sftp::enable(
        &f.ctx(),
        "deploy",
        None,
        Some("www-data"),
        false,
        &crate::test_support::elevation::access(),
    )
    .await
    .unwrap();
    assert!(result.active);
    assert_eq!(result.running_operations.len(), 1);
    assert_eq!(result.running_operations[0].kind, "delete");
    assert!(
        !slot.is_revoked(),
        "ohne Bestätigung bleibt alles wie es war"
    );
}

/// Acceptance: Trennen während eines erhöhten Transfers stoppt ihn ohne Dialog.
#[tokio::test]
async fn test_622_disconnect_stops_an_elevated_transfer_without_a_dialog() {
    let elevated = GatedSftp::small_tree(0);
    elevated.put_file("/t/a.txt", three_chunk_content());
    elevated.pause_after_chunk(1);
    let s = setup(Box::new(elevated.clone())).await;
    let emitter = Arc::new(app_logic::events::TestEmitter::default());
    let dir = tempfile::tempdir().unwrap();
    let download = spawn_download(&s, &emitter, "/t/a.txt", dir.path().join("a.txt"), None);
    elevated.wait_until_chunk_paused().await;

    s.registry.remove_session(&s.sessions, s.session_id);
    elevated.release_chunk();

    assert_inactive(join(download, &elevated).await);
    assert_eq!(elevated.chunks(), 1, "nach dem Trennen kein weiteres Stück");
    assert_eq!(
        part_files(dir.path()),
        Vec::<String>::new(),
        "Temp-Datei weg"
    );
}

/// Acceptance: Abbrechen-Knopf — ein Stück, Teildatei weg, Zeile „cancelled“,
/// kein Fehler, Kanal bleibt offen.
#[tokio::test]
async fn test_622_cancel_stops_a_download_and_removes_the_partial_file() {
    let elevated = GatedSftp::small_tree(0);
    elevated.put_file("/t/a.txt", three_chunk_content());
    elevated.pause_after_chunk(1);
    let s = setup(Box::new(elevated.clone())).await;
    let slot = s.slot();
    let emitter = Arc::new(app_logic::events::TestEmitter::default());
    let dir = tempfile::tempdir().unwrap();
    let download = spawn_download(&s, &emitter, "/t/a.txt", dir.path().join("a.txt"), None);
    elevated.wait_until_chunk_paused().await;

    let running = s.registry.ops().running_elevated(s.session_id);
    let id: uuid::Uuid = running[0].id.parse().unwrap();
    assert!(s.registry.ops().cancel(s.session_id, id));
    elevated.release_chunk();

    let err = join(download, &elevated).await.expect_err("abgebrochen");
    assert_eq!(
        err.code,
        Some(super::super::transfer::FILE_OPERATION_STOPPED)
    );
    assert_eq!(elevated.chunks(), 1);
    assert!(part_files(dir.path()).is_empty());
    let finished = events_named(&emitter, "sftp-transfer-finished");
    assert_eq!(finished[0]["stopped"], "cancelled");
    assert!(finished[0]["error"].is_null());
    assert!(!slot.is_revoked());
}

/// Upload-Abbruch entfernt die Teildatei auf dem Server.
#[tokio::test]
async fn test_622_cancel_removes_the_partial_remote_upload() {
    let elevated = GatedSftp::small_tree(0);
    elevated.pause_after_chunk(1);
    let s = setup(Box::new(elevated.clone())).await;
    let emitter = Arc::new(app_logic::events::TestEmitter::default());
    let dir = tempfile::tempdir().unwrap();
    let local = dir.path().join("up.bin");
    std::fs::write(&local, three_chunk_content()).unwrap();
    let upload = spawn_upload(&s, &emitter, local, "/t/up.bin");
    elevated.wait_until_chunk_paused().await;

    let id: uuid::Uuid = s.registry.ops().running_elevated(s.session_id)[0]
        .id
        .parse()
        .unwrap();
    assert!(s.registry.ops().cancel(s.session_id, id));
    elevated.release_chunk();

    let err = join(upload, &elevated).await.expect_err("abgebrochen");
    assert_eq!(
        err.code,
        Some(super::super::transfer::FILE_OPERATION_STOPPED)
    );
    assert!(!elevated.exists("/t/up.bin"), "Teildatei entfernt");
    let finished = events_named(&emitter, "sftp-transfer-finished");
    assert_eq!(finished[0]["stopped"], "cancelled");
    assert!(finished[0]["incompleteRemotePath"].is_null());
}

/// Lässt sich die Teildatei nicht entfernen, nennt die Zeile ihren Pfad.
#[tokio::test]
async fn test_622_cancel_names_the_incomplete_remote_file_when_removal_fails() {
    let elevated = GatedSftp::small_tree(0);
    elevated.pause_after_chunk(1);
    elevated.fail_remove();
    let s = setup(Box::new(elevated.clone())).await;
    let emitter = Arc::new(app_logic::events::TestEmitter::default());
    let dir = tempfile::tempdir().unwrap();
    let local = dir.path().join("up.bin");
    std::fs::write(&local, three_chunk_content()).unwrap();
    let upload = spawn_upload(&s, &emitter, local, "/t/up.bin");
    elevated.wait_until_chunk_paused().await;

    let id: uuid::Uuid = s.registry.ops().running_elevated(s.session_id)[0]
        .id
        .parse()
        .unwrap();
    s.registry.ops().cancel(s.session_id, id);
    elevated.release_chunk();

    join(upload, &elevated).await.expect_err("abgebrochen");
    let finished = events_named(&emitter, "sftp-transfer-finished");
    assert_eq!(finished[0]["stopped"], "cancelled");
    assert_eq!(finished[0]["incompleteRemotePath"], "/t/up.bin");
}

/// Ein erfolgreicher Download ist byte-identisch und ersetzt eine vorhandene
/// Datei erst nach dem Erfolg.
#[tokio::test]
async fn test_622_download_is_identical_and_replaces_the_old_file_only_on_success() {
    let elevated = GatedSftp::small_tree(0);
    elevated.put_file("/t/a.txt", three_chunk_content());
    elevated.pause_after_chunk(2);
    let s = setup(Box::new(elevated.clone())).await;
    let emitter = Arc::new(app_logic::events::TestEmitter::default());
    let dir = tempfile::tempdir().unwrap();
    let local = dir.path().join("a.txt");
    std::fs::write(&local, b"ALT").unwrap();
    let download = spawn_download(&s, &emitter, "/t/a.txt", local.clone(), None);
    elevated.wait_until_chunk_paused().await;
    assert_eq!(
        std::fs::read(&local).unwrap(),
        b"ALT",
        "noch die alte Datei"
    );

    elevated.release_chunk();
    join(download, &elevated).await.unwrap();
    assert_eq!(std::fs::read(&local).unwrap(), three_chunk_content());
    assert_eq!(part_files(dir.path()), vec!["a.txt".to_string()]);
}

/// Ordner-Download: Gesamtfortschritt (Dateien/Bytes) in den Ereignissen.
#[tokio::test]
async fn test_622_folder_download_reports_aggregate_progress() {
    let elevated = GatedSftp::small_tree(0);
    elevated.put_file("/t/a.txt", three_chunk_content());
    elevated.put_file("/t/b.txt", vec![7u8; 1000]);
    let s = setup(Box::new(elevated.clone())).await;
    let emitter = Arc::new(app_logic::events::TestEmitter::default());
    let dir = tempfile::tempdir().unwrap();
    let local_root = dir.path().join("t");
    let em = emitter.clone();
    let session_id = s.session_id;
    let result = spawn_command(
        s.session.clone(),
        s.elevated_channel(),
        move |session, channel| async move {
            download_recursive(
                em.as_ref(),
                &session,
                &channel,
                session_id,
                "/t",
                &local_root,
            )
            .await
        },
    );
    join(result, &elevated)
        .await
        .expect("Ordner-Download gelingt");

    let progress = events_named(&emitter, "sftp-transfer-progress");
    assert!(!progress.is_empty());
    let total_files = 3u64; // a.txt, b.txt, sub/c.txt
    for p in &progress {
        let folder = &p["folder"];
        assert_eq!(folder["filesTotal"].as_u64(), Some(total_files), "{p}");
        assert!(folder["bytesTotal"].as_u64().unwrap() >= three_chunk_content().len() as u64);
        assert!(folder["bytesDone"].as_u64().unwrap() <= folder["bytesTotal"].as_u64().unwrap());
    }
}

/// Issue #656: an action that still carries the identity of an edit session
/// opened as another user is refused with the stable `ELEVATED_USER_CHANGED`
/// code and never touches the channel (no silent switch); the same action with
/// the current identity passes the check.
#[tokio::test]
async fn test_656_stale_expected_identity_is_refused_and_explicit_current_identity_passes() {
    let elevated = MockSftpSession::new().with_file("/t/a.txt", b"x".to_vec());
    let s = setup(Box::new(elevated.clone())).await;

    let stale = BrowserChannel::for_tests(&s.registry, s.session_id, Some("www-data".to_string()));
    let err = delete_impl(&s.session, &stale, "/t/a.txt")
        .await
        .expect_err("an old expected identity must be refused");
    assert_eq!(err.code, Some("ELEVATED_USER_CHANGED"));
    assert!(err.message.contains("root") && err.message.contains("www-data"));
    assert!(
        elevated.file_content("/t/a.txt").is_some(),
        "a refused action must not touch the elevated channel"
    );

    delete_impl(&s.session, &s.elevated_channel(), "/t/a.txt")
        .await
        .expect("the current identity, sent explicitly, passes");
    assert!(!elevated.file_content("/t/a.txt").is_some());
}

// --- Issue #654: chown ---------------------------------------------------------

/// Issue #654: disabling the elevated mode in the middle of a recursive
/// chown stops it at once, like chmod (T2): no further `set_owner`.
#[tokio::test]
async fn test_654_disabling_during_a_recursive_chown_stops_it_immediately() {
    let elevated = GatedSftp::small_tree(5);
    let s = setup(Box::new(elevated.clone())).await;
    let slot = s.slot();

    let command = spawn_command(
        s.session.clone(),
        s.elevated_channel(),
        |session, channel| async move {
            chown_impl(&session, &channel, "/t", Some("1000"), Some("100"), true).await
        },
    );
    elevated.wait_until_paused().await;
    let ops_before = elevated.op_count();
    assert!(
        !elevated.owner_ops().is_empty(),
        "precondition: at least one entry was already changed"
    );

    let (sessions, registry) = (s.sessions.clone(), s.registry.clone());
    let session_id = s.session_id;
    let disable = tokio::spawn(async move {
        crate::elevated_sftp::disable_now(
            &sessions,
            &registry,
            session_id,
            &crate::test_support::elevation::access(),
        )
        .await
    });
    s.wait_until_revoked(&slot).await;
    elevated.release();

    assert_inactive(join(command, &elevated).await);
    expect_within("disabling ends", || elevated.describe(), disable)
        .await
        .expect("disable task must not panic")
        .expect("disabling succeeds");
    assert_eq!(elevated.op_count(), ops_before, "{:?}", elevated.ops());
    assert!(
        s.normal.calls().is_empty(),
        "no fallback to the normal channel, was: {:?}",
        s.normal.calls()
    );
}

/// Issue #654: elevated mode switched off before the command starts → the
/// inactive message, nothing changed, no fallback to the normal channel.
#[tokio::test]
async fn test_654_chown_after_the_elevated_mode_was_switched_off_fails_as_inactive() {
    let elevated = GatedSftp::small_tree(0);
    let s = setup(Box::new(elevated.clone())).await;
    let slot = s.slot();
    let channel = s.elevated_channel();
    s.registry.remove_session(&s.sessions, s.session_id);
    assert!(slot.is_revoked());

    let result = with_browser_channel(s.session.clone(), channel, |session, channel| async move {
        chown_impl(&session, &channel, "/t/a.txt", Some("1"), Some("2"), false).await
    })
    .await;
    assert_inactive(result);
    assert!(elevated.owner_ops().is_empty(), "{:?}", elevated.ops());
    assert!(s.normal.calls().is_empty(), "{:?}", s.normal.calls());
}

/// Issue #654: recursive chown over the normal channel changes every entry
/// of the tree (root last), reports the count, and does not touch symlinks.
#[tokio::test]
async fn test_654_recursive_chown_walks_the_tree_and_skips_symlinks() {
    let tree = GatedSftp::small_tree(0);
    tree.mark_symlink("/t/b.txt");
    let s = setup(Box::new(GatedSftp::small_tree(0))).await;
    s.session.set_sftp_for_tests(Box::new(tree.clone())).await;

    let changed = with_browser_channel(
        s.session.clone(),
        s.normal_channel(),
        |session, channel| async move {
            chown_impl(&session, &channel, "/t", Some("1000"), Some("100"), true).await
        },
    )
    .await
    .expect("recursive chown succeeds");
    assert_eq!(
        changed, 4,
        "root + a.txt + sub + sub/c.txt, not the symlink"
    );
    let ops = tree.owner_ops();
    assert_eq!(ops.len(), 4, "{ops:?}");
    assert!(ops.iter().all(|o| o.ends_with(" 1000 100")), "{ops:?}");
    assert!(!ops.iter().any(|o| o.contains("/t/b.txt")), "{ops:?}");
    assert_eq!(ops.last().unwrap(), "set_owner /t 1000 100", "root last");
}

/// Issue #654: names are resolved through passwd/group of the server; an
/// unknown name fails with its own code and **nothing** is changed.
#[tokio::test]
async fn test_654_chown_resolves_names_and_unknown_name_changes_nothing() {
    let tree = GatedSftp::new(
        &[
            ("/etc", true),
            ("/etc/passwd", false),
            ("/etc/group", false),
            ("/t", true),
            ("/t/a.txt", false),
        ],
        0,
        false,
    );
    tree.set_content(
        "/etc/passwd",
        "root:x:0:0::/root:/bin/sh\nalice:x:1000:1000::/h:/bin/sh\n",
    );
    tree.set_content("/etc/group", "staff:x:50:\n");
    let s = setup(Box::new(GatedSftp::small_tree(0))).await;
    s.session.set_sftp_for_tests(Box::new(tree.clone())).await;

    let changed = with_browser_channel(
        s.session.clone(),
        s.normal_channel(),
        |session, channel| async move {
            chown_impl(
                &session,
                &channel,
                "/t/a.txt",
                Some("alice"),
                Some("staff"),
                false,
            )
            .await
        },
    )
    .await
    .expect("names resolve");
    assert_eq!(changed, 1);
    assert_eq!(tree.owner_ops(), vec!["set_owner /t/a.txt 1000 50"]);

    for (owner, group, code) in [
        (Some("mallory"), Some("staff"), "CHOWN_UNKNOWN_USER"),
        (Some("alice"), Some("nogroup"), "CHOWN_UNKNOWN_GROUP"),
    ] {
        let err = with_browser_channel(
            s.session.clone(),
            s.normal_channel(),
            move |session, channel| async move {
                chown_impl(&session, &channel, "/t", owner, group, true).await
            },
        )
        .await
        .expect_err("unknown name must fail");
        assert_eq!(err.code, Some(code));
    }
    assert_eq!(
        tree.owner_ops().len(),
        1,
        "nothing changed after the errors"
    );
}

// --- Issue #921: the SFTP lock is held per chunk, not per transfer ----------

fn spawn_normal_download(
    s: &Setup,
    emitter: &Arc<app_logic::events::TestEmitter>,
    remote: &'static str,
    local: std::path::PathBuf,
) -> tokio::task::JoinHandle<CommandResult<()>> {
    let em = emitter.clone();
    let session_id = s.session_id;
    spawn_command(
        s.session.clone(),
        s.normal_channel(),
        move |session, channel| async move {
            download_one_file(
                em.as_ref(),
                &session,
                &channel,
                session_id,
                remote,
                local,
                None,
            )
            .await
        },
    )
}

/// Index of the first op that starts with `prefix`.
fn op_index(ops: &[String], prefix: &str) -> usize {
    ops.iter()
        .position(|o| o.starts_with(prefix))
        .unwrap_or_else(|| panic!("no op {prefix:?} in {ops:?}"))
}

/// A listing and an AI-style file read (same `lock_sftp` as
/// `execute_read_remote_file`) of the same session run *between* two chunks of
/// a running download — before the last chunk is read, not after the transfer.
#[tokio::test]
async fn test_921_list_and_ai_read_complete_while_a_download_is_running() {
    let normal = GatedSftp::small_tree(0);
    let size = (CHUNK * 3) as u64;
    normal.set_synthetic("/t/big.bin", size);
    normal.pause_after_chunk(2);
    let s = setup(Box::new(GatedSftp::small_tree(0))).await;
    s.session.set_sftp_for_tests(Box::new(normal.clone())).await;
    let emitter = Arc::new(app_logic::events::TestEmitter::default());
    let dir = tempfile::tempdir().unwrap();
    let local = dir.path().join("big.bin");

    let download = spawn_normal_download(&s, &emitter, "/t/big.bin", local.clone());
    // The mock holds the lock while it serves chunk 2.
    normal.wait_until_chunk_paused().await;

    let list = {
        let (session, channel) = (s.session.clone(), s.normal_channel());
        tokio::spawn(async move { list_impl(&session, &channel, "/t").await })
    };
    let ai_read = {
        let session = s.session.clone();
        tokio::spawn(async move {
            let mut guard = session.lock_sftp().await;
            guard.sftp_or_err()?.read_file("/t/a.txt").await
        })
    };
    // Let both tasks queue on the session's lock.
    for _ in 0..5 {
        tokio::task::yield_now().await;
    }
    assert!(!list.is_finished() && !ai_read.is_finished());
    normal.release_chunk();

    expect_within("the listing completes", || normal.describe(), list)
        .await
        .unwrap()
        .expect("listing works");
    expect_within("the AI read completes", || normal.describe(), ai_read)
        .await
        .unwrap()
        .expect("read works");
    join_plain(download, &normal).await.expect("download works");

    let ops = normal.ops();
    let last_chunk = ops
        .iter()
        .rposition(|o| o.starts_with("read_range /t/big.bin"))
        .expect("third chunk read");
    assert!(op_index(&ops, "list_dir /t") < last_chunk, "{ops:?}");
    assert!(op_index(&ops, "read_file /t/a.txt") < last_chunk, "{ops:?}");
    assert_eq!(std::fs::metadata(&local).unwrap().len(), size);
}

async fn join_plain<T>(
    handle: tokio::task::JoinHandle<CommandResult<T>>,
    gate: &GatedSftp,
) -> CommandResult<T> {
    join(handle, gate).await
}

/// Revoking the elevated channel between two chunks ends the transfer with the
/// revoked error, reads no further chunk and leaves no temp file.
#[tokio::test]
async fn test_921_revocation_between_chunks_reads_no_further_chunk() {
    let elevated = GatedSftp::small_tree(0);
    elevated.put_file("/t/a.txt", three_chunk_content());
    elevated.pause_after_chunk(1);
    let s = setup(Box::new(elevated.clone())).await;
    let slot = s.slot();
    let emitter = Arc::new(app_logic::events::TestEmitter::default());
    let dir = tempfile::tempdir().unwrap();
    let download = spawn_download(&s, &emitter, "/t/a.txt", dir.path().join("a.txt"), None);
    elevated.wait_until_chunk_paused().await;

    s.registry.remove_session(&s.sessions, s.session_id);
    s.wait_until_revoked(&slot).await;
    elevated.release_chunk();

    assert_inactive(join(download, &elevated).await);
    assert_eq!(elevated.chunks(), 1, "no chunk after the revocation");
    assert!(part_files(dir.path()).is_empty(), "no temp file is left");
}

/// A stopped upload with several chunks writes in ranges and an empty file is
/// still created.
#[tokio::test]
async fn test_921_upload_writes_chunk_by_chunk_and_creates_empty_files() {
    let normal = GatedSftp::small_tree(0);
    let s = setup(Box::new(GatedSftp::small_tree(0))).await;
    s.session.set_sftp_for_tests(Box::new(normal.clone())).await;
    let emitter = Arc::new(app_logic::events::TestEmitter::default());
    let dir = tempfile::tempdir().unwrap();
    let big = dir.path().join("big.bin");
    let content = three_chunk_content();
    std::fs::write(&big, &content).unwrap();
    let empty = dir.path().join("empty.bin");
    std::fs::write(&empty, b"").unwrap();

    for (local, remote) in [(big, "/t/up.bin"), (empty, "/t/empty.bin")] {
        let em = emitter.clone();
        let session_id = s.session_id;
        let granted = app_logic::local_path_grants::GrantedLocalPath::unchecked_for_tests(local);
        let task = spawn_command(
            s.session.clone(),
            s.normal_channel(),
            move |session, channel| async move {
                upload_impl(
                    em.as_ref(),
                    &session,
                    &channel,
                    session_id,
                    &granted,
                    remote,
                )
                .await
            },
        );
        join(task, &normal).await.expect("upload works");
    }
    assert_eq!(normal.content("/t/up.bin").unwrap(), content);
    assert_eq!(normal.content("/t/empty.bin").unwrap(), Vec::<u8>::new());
    assert_eq!(
        normal
            .ops()
            .iter()
            .filter(|o| o.starts_with("write_range /t/up.bin"))
            .count(),
        2,
        "chunks 2 and 3 are range writes after the creating first write"
    );
}

/// Issue #921 guard: local file I/O of transfers runs in `spawn_blocking`
/// (directly or through `blocking_io`); every `std::fs::` use in the transfer
/// modules sits inside such a closure or in `create_temp`, which only those
/// closures call.
#[test]
fn test_921_no_blocking_std_fs_outside_spawn_blocking() {
    for (name, source) in [
        ("transfer.rs", include_str!("../transfer.rs")),
        ("folder_upload.rs", include_str!("../folder_upload.rs")),
        ("download.rs", include_str!("../download.rs")),
    ] {
        let lines: Vec<&str> = source.lines().collect();
        let end = lines
            .iter()
            .position(|l| l.contains("#[cfg(test)]"))
            .unwrap_or(lines.len());
        for (i, line) in lines[..end].iter().enumerate() {
            if !line.contains("std::fs::") || line.trim_start().starts_with("//") {
                continue;
            }
            let from = i.saturating_sub(12);
            let context = lines[from..=i].join("\n");
            assert!(
                context.contains("blocking_io(")
                    || context.contains("spawn_blocking(")
                    || context.contains("fn create_temp"),
                "{name}:{}: std::fs outside spawn_blocking: {line}",
                i + 1
            );
        }
    }
}
