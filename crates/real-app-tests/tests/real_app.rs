//! Startup paths of the real app (issue #235, Spec 0090 §2a).
//!
//! All tests are `#[ignore]`d: they need the built `smart-ssh` binary,
//! `tauri-driver`, a display (Xvfb) and an unlocked Secret Service session.
//! The CI job runs them with `cargo test -p real-app-tests -- --ignored
//! --test-threads=1`. The cases share one keychain and one single-instance
//! name, so they run one after the other (the mutex below enforces it even
//! without `--test-threads=1`).

mod support;

use serde_json::json;
use support::*;
use tokio::sync::Mutex;

static ONE_AT_A_TIME: Mutex<()> = Mutex::const_new(());

/// Path 1: fresh data directory, keychain mode. The app creates the key and
/// the database, hands its state to Tauri, and answers commands.
#[tokio::test]
#[ignore = "needs a built app, tauri-driver, a display and a Secret Service session"]
async fn fresh_install_in_keychain_mode_lists_servers() -> TestResult {
    let _serial = ONE_AT_A_TIME.lock().await;
    clear_test_keychain()?;
    let data_dir = tempfile::tempdir()?;

    let app = RealApp::start(data_dir.path()).await?;
    let result = async {
        let names = app.wait_for_server_names().await?;
        // A fresh installation has no servers besides the local one.
        assert!(names.is_empty(), "unexpected servers: {names:?}");
        app.assert_no_state_error_in_window().await?;
        assert!(
            data_dir.path().join("smart-ssh.db").is_file(),
            "the app did not create its database in the data directory"
        );
        // A second read through the state, a different command.
        let groups = app.invoke("list_groups", json!({})).await?;
        assert!(groups.is_ok(), "list_groups was rejected: {groups:?}");
        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    app.quit().await?;
    clear_test_keychain()?;
    result
}

/// Path 2: a plaintext database written by release 0.5.2 with credentials in
/// the keychain. The app converts the file and moves the credentials.
#[tokio::test]
#[ignore = "needs a built app, tauri-driver, a display and a Secret Service session"]
async fn upgraded_plaintext_database_lists_its_servers() -> TestResult {
    let _serial = ONE_AT_A_TIME.lock().await;
    clear_test_keychain()?;
    let data_dir = tempfile::tempdir()?;
    seed_plaintext_upgrade(data_dir.path())?;

    let app = RealApp::start(data_dir.path()).await?;
    let result = async {
        let mut names = app.wait_for_server_names().await?;
        names.sort();
        let mut expected: Vec<String> =
            UPGRADE_SERVER_NAMES.iter().map(|s| s.to_string()).collect();
        expected.sort();
        assert_eq!(names, expected);
        app.wait_for_names_in_window(&UPGRADE_SERVER_NAMES).await?;
        app.assert_no_state_error_in_window().await?;
        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    app.quit().await?;
    clear_test_keychain()?;
    result
}

/// Path 3: password mode. Locked start, commands rejected, unlock through
/// the window, then the servers are listed and nothing is rejected with a
/// state or lock error.
#[tokio::test]
#[ignore = "needs a built app, tauri-driver, a display and a Secret Service session"]
async fn password_mode_starts_locked_then_unlocks_and_lists() -> TestResult {
    use thirtyfour::prelude::*;

    let _serial = ONE_AT_A_TIME.lock().await;
    clear_test_keychain()?;
    let data_dir = tempfile::tempdir()?;
    seed_password_mode(data_dir.path()).await?;

    let app = RealApp::start(data_dir.path()).await?;
    let result = async {
        let field = app
            .driver
            .query(By::Css("input[type=password]"))
            .wait(
                std::time::Duration::from_secs(90),
                std::time::Duration::from_millis(250),
            )
            .first()
            .await?;

        // Before unlocking, the gate holds commands back.
        let locked = app
            .invoke("list_servers", json!({ "groupId": null }))
            .await?;
        assert!(
            locked.is_err(),
            "list_servers was answered while locked: {locked:?}"
        );

        field.send_keys(FIXTURE_MASTER_PASSWORD).await?;
        app.driver
            .find(By::Css("button[type=submit]"))
            .await?
            .click()
            .await?;

        let mut names = app.wait_for_server_names().await?;
        names.sort();
        let mut expected: Vec<String> = PASSWORD_MODE_SERVER_NAMES
            .iter()
            .map(|s| s.to_string())
            .collect();
        expected.sort();
        assert_eq!(names, expected);
        app.wait_for_names_in_window(&PASSWORD_MODE_SERVER_NAMES)
            .await?;
        app.assert_no_state_error_in_window().await?;
        let listed = app
            .invoke("list_servers", json!({ "groupId": null }))
            .await?;
        if let Err(rejection) = &listed {
            assert!(
                !is_state_or_lock_error(rejection),
                "rejected with a state or lock error: {rejection}"
            );
        }
        assert!(
            listed.is_ok(),
            "list_servers was rejected after unlock: {listed:?}"
        );
        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    app.quit().await?;
    clear_test_keychain()?;
    result
}
