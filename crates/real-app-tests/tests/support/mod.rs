//! Real-app startup tests (issue #235, Spec 0090 §2a).
//!
//! The Playwright suite (issue #166) runs the frontend against a fake
//! backend, and the Rust tests build `AppState` without Tauri. Neither starts
//! the real desktop app, so a startup that built its state and then never
//! handed it to Tauri (issue #234) passed everything. This crate starts the
//! actual `smart-ssh` binary through `tauri-driver` (WebDriver) on a seeded
//! data directory and asks the window and the IPC layer what they see.
//!
//! Nothing here is linked into the shipped application. Fixtures contain
//! obviously fake values only.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use secrecy::SecretString;
use serde_json::{json, Value};
use ssh_manager_core::profiles::{CredentialRef, CredentialStore};
use thirtyfour::prelude::*;
use tokio::process::{Child, Command};

/// Environment variable naming the built app binary. Default:
/// `target/debug/smart-ssh` of the workspace.
pub const APP_BINARY_ENV: &str = "SMART_SSH_REAL_APP_BINARY";
/// Environment variable naming the `tauri-driver` executable. Default:
/// `tauri-driver` from `PATH`.
pub const DRIVER_BINARY_ENV: &str = "TAURI_DRIVER";

/// Master password of the password-mode fixture. Fake, public on purpose.
pub const FIXTURE_MASTER_PASSWORD: &str = "fixture-master-password-0235";

/// The two servers of the checked-in 0.5.2 release fixture
/// (`crates/persistence-sqlite/tests/fixtures/releases/v0.5.2.sqlite3`).
pub const UPGRADE_SERVER_NAMES: [&str; 2] = ["Release fixture A", "Release fixture B"];

/// Servers created for the password-mode fixture.
pub const PASSWORD_MODE_SERVER_NAMES: [&str; 2] = ["Locked fixture one", "Locked fixture two"];

const POLL: Duration = Duration::from_millis(250);
const STARTUP_TIMEOUT: Duration = Duration::from_secs(90);

pub type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

pub fn app_binary() -> PathBuf {
    std::env::var_os(APP_BINARY_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace_root().join("target/debug/smart-ssh"))
}

fn release_fixture_path() -> PathBuf {
    workspace_root().join("crates/persistence-sqlite/tests/fixtures/releases/v0.5.2.sqlite3")
}

/// The database file name inside a data directory.
fn db_path_in(data_dir: &Path) -> PathBuf {
    data_dir.join("smart-ssh.db")
}

// ---------------------------------------------------------------- seeding

/// Removes everything the fixtures put into the test keychain, including the
/// root key the app generates, so each case starts from the same keychain.
/// Missing entries are fine.
pub fn clear_test_keychain() -> TestResult {
    let keychain = credentials_keyring::KeyringCredentialStore::new();
    let a = uuid::Uuid::from_u128(0x0011);
    let b = uuid::Uuid::from_u128(0x0012);
    for name in [
        ssh_manager_core::crypto::CHAT_CONTENT_ENCRYPTION_KEY_REF.to_string(),
        format!("server:{a}:password"),
        format!("server:{b}:private_key"),
        format!("server:{b}:passphrase"),
    ] {
        match keychain.delete(&CredentialRef::new(name)) {
            Ok(()) | Err(ssh_manager_core::profiles::CredentialError::NotFound(_)) => {}
            Err(err) => return Err(format!("could not clear the test keychain: {err}").into()),
        }
    }
    Ok(())
}

/// Upgrade path: a plaintext database written by release 0.5.2, with the
/// credentials its servers refer to in the test keychain. The root key is
/// absent, so the app generates one and converts the file (Spec 0101, A3/A6).
pub fn seed_plaintext_upgrade(data_dir: &Path) -> TestResult {
    std::fs::copy(release_fixture_path(), db_path_in(data_dir))?;
    // The fixture's servers use fixed ids (see its generator).
    let keychain = credentials_keyring::KeyringCredentialStore::new();
    let a = uuid::Uuid::from_u128(0x0011);
    let b = uuid::Uuid::from_u128(0x0012);
    for (name, value) in [
        (format!("server:{a}:password"), "fake-password-0235"),
        (format!("server:{b}:private_key"), "fake-private-key-0235"),
        (format!("server:{b}:passphrase"), "fake-passphrase-0235"),
    ] {
        keychain.set(&CredentialRef::new(name), SecretString::from(value))?;
    }
    Ok(())
}

/// Password mode: an encrypted database with two servers and a master
/// password wrapping its root key, set up through the same functions the app
/// uses. There is no root key in the keychain.
pub async fn seed_password_mode(data_dir: &Path) -> TestResult {
    use persistence_sqlite::SqliteProfileStore;
    use ssh_manager_core::crypto::{generate_root_key, DatabaseKey};
    use ssh_manager_core::profiles::{AuthMethod, PostIngestPolicy, ProfileStore, Server};
    use ssh_manager_core::shared::ServerId;

    let db_path = db_path_in(data_dir);
    let root_key = generate_root_key();
    let store =
        SqliteProfileStore::connect_encrypted(&db_path, &DatabaseKey::from_root_key(&root_key))
            .await?;
    let now = chrono::Utc::now();
    for name in PASSWORD_MODE_SERVER_NAMES {
        store
            .create_server(&Server {
                id: ServerId::new(),
                name: name.to_string(),
                host: "host-0235.example".into(),
                port: 22,
                username: "user-0235".into(),
                group_id: None,
                tags: Vec::new(),
                auth: AuthMethod::Agent,
                notes: String::new(),
                jump_host: None,
                post_ingest_policy: PostIngestPolicy::default(),
                ai_injection_check_enabled: false,
                sftp_server_path: None,
                start_directory: None,
                created_at: now,
                updated_at: now,
            })
            .await?;
    }
    store.close().await;

    let password = SecretString::from(FIXTURE_MASTER_PASSWORD);
    app_logic::master_password::set_up_master_password(
        &db_path,
        &root_key,
        &password,
        &password,
        app_logic::master_password::LossWarning::ConfirmedByTheUser,
        None,
    )
    .map_err(|err| format!("could not set up the fixture master password: {err}"))?;
    Ok(())
}

// ---------------------------------------------------------------- the app

/// A running app instance behind `tauri-driver`. Dropping it kills the driver
/// (and with it the app).
pub struct RealApp {
    pub driver: WebDriver,
    child: Child,
}

fn free_port() -> TestResult<u16> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    Ok(listener.local_addr()?.port())
}

impl RealApp {
    /// Starts `tauri-driver` with `data_dir` as `SMART_SSH_DATA_DIR`, then
    /// the app through it. Must run inside the Secret Service session the
    /// seeding used.
    pub async fn start(data_dir: &Path) -> TestResult<Self> {
        let binary = app_binary();
        if !binary.exists() {
            return Err(format!(
                "app binary not found at {} (build it first, or set {APP_BINARY_ENV})",
                binary.display()
            )
            .into());
        }
        let port = free_port()?;
        let native_port = free_port()?;
        let driver_bin =
            std::env::var_os(DRIVER_BINARY_ENV).unwrap_or_else(|| "tauri-driver".into());
        let child = Command::new(driver_bin)
            .arg("--port")
            .arg(port.to_string())
            .arg("--native-port")
            .arg(native_port.to_string())
            .env("SMART_SSH_DATA_DIR", data_dir)
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|err| format!("could not start tauri-driver: {err}"))?;

        let deadline = Instant::now() + Duration::from_secs(30);
        while tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .is_err()
        {
            if Instant::now() > deadline {
                return Err("tauri-driver did not start listening".into());
            }
            tokio::time::sleep(POLL).await;
        }

        let mut caps = Capabilities::new();
        caps.set("browserName", "wry")?;
        caps.set(
            "tauri:options",
            json!({ "application": binary.to_string_lossy() }),
        )?;
        let driver = WebDriver::new(format!("http://127.0.0.1:{port}"), caps).await?;
        Ok(Self { driver, child })
    }

    /// Calls a Tauri command through the real IPC layer. `Ok` is the
    /// resolved value, `Err` the rejection (stringified).
    pub async fn invoke(&self, command: &str, args: Value) -> TestResult<Result<Value, String>> {
        let script = r#"
            const done = arguments[arguments.length - 1];
            const [command, args] = arguments;
            window.__TAURI_INTERNALS__.invoke(command, args).then(
              (value) => done({ ok: true, value }),
              (error) => done({ ok: false, error: typeof error === "string" ? error : JSON.stringify(error) }),
            );
        "#;
        let ret = self
            .driver
            .execute_async(script, vec![json!(command), args])
            .await?;
        let value: Value = ret.json().clone();
        if value["ok"] == json!(true) {
            Ok(Ok(value["value"].clone()))
        } else {
            Ok(Err(value["error"].as_str().unwrap_or("").to_string()))
        }
    }

    /// Text of the whole page.
    pub async fn page_text(&self) -> TestResult<String> {
        let ret = self
            .driver
            .execute(
                "return document.body ? document.body.innerText : '';",
                vec![],
            )
            .await?;
        Ok(ret.json().as_str().unwrap_or("").to_string())
    }

    /// Waits until `list_servers` resolves and returns the server names.
    /// The app needs a moment after the window exists (state build,
    /// webview load), so a rejection is retried until the timeout and then
    /// reported with its text.
    pub async fn wait_for_server_names(&self) -> TestResult<Vec<String>> {
        let deadline = Instant::now() + STARTUP_TIMEOUT;
        let mut last;
        loop {
            match self
                .invoke("list_servers", json!({ "groupId": null }))
                .await
            {
                Ok(Ok(Value::Array(servers))) => {
                    return Ok(servers
                        .iter()
                        .filter(|s| s["isLocal"] != json!(true))
                        .filter_map(|s| s["name"].as_str().map(str::to_string))
                        .collect());
                }
                Ok(Ok(other)) => last = format!("unexpected result: {other}"),
                Ok(Err(rejection)) => last = format!("rejected: {rejection}"),
                // The page may not be ready to execute scripts yet.
                Err(err) => last = format!("driver error: {err}"),
            }
            if Instant::now() > deadline {
                return Err(format!("list_servers never resolved ({last})").into());
            }
            tokio::time::sleep(POLL).await;
        }
    }

    /// Waits until each name shows up in the window's text.
    pub async fn wait_for_names_in_window(&self, names: &[&str]) -> TestResult {
        let deadline = Instant::now() + STARTUP_TIMEOUT;
        loop {
            let text = self.page_text().await.unwrap_or_default();
            if names.iter().all(|name| text.contains(name)) {
                return Ok(());
            }
            if Instant::now() > deadline {
                return Err(format!("the window does not list {names:?}; it shows: {text}").into());
            }
            tokio::time::sleep(POLL).await;
        }
    }

    /// Fails when the window text carries a state or lock error.
    pub async fn assert_no_state_error_in_window(&self) -> TestResult {
        let text = self.page_text().await?.to_lowercase();
        for marker in [
            "not managed",
            "state not managed",
            "startup gate",
            "is locked",
        ] {
            if text.contains(marker) {
                return Err(format!("the window shows a state or lock error ({marker:?})").into());
            }
        }
        Ok(())
    }

    /// Ends the session and the driver.
    pub async fn quit(mut self) -> TestResult {
        let _ = self.driver.clone().quit().await;
        let _ = self.child.kill().await;
        Ok(())
    }
}

/// Message fragments by which a rejected command shows the bug class of
/// issue #234 (state never handed to Tauri) or a lock error.
pub fn is_state_or_lock_error(rejection: &str) -> bool {
    let lower = rejection.to_lowercase();
    lower.contains("not managed") || lower.contains("locked") || lower.contains("gesperrt")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_not_managed_is_recognised() {
        assert!(is_state_or_lock_error(
            "state not managed for field `state` on command `list_servers`"
        ));
        assert!(!is_state_or_lock_error("server not found"));
    }

    /// The password-mode seed is what the app takes for password mode, and
    /// the fixture password unlocks it. Runs without a display or keychain.
    #[tokio::test]
    async fn password_mode_seed_is_a_wrapped_database() {
        let dir = tempfile::tempdir().unwrap();
        seed_password_mode(dir.path()).await.unwrap();
        let db_path = db_path_in(dir.path());
        assert_eq!(
            app_logic::master_password::key_mode(&db_path),
            app_logic::master_password::KeyMode::Password
        );
        let password = SecretString::from(FIXTURE_MASTER_PASSWORD);
        assert!(app_logic::master_password::unlock(&db_path, &password).is_ok());
    }

    #[test]
    fn release_fixture_exists() {
        assert!(release_fixture_path().is_file());
    }
}
