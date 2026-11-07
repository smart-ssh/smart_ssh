//! End-to-end exec, PTY and SFTP edge cases against real sshd (issue #1180):
//! exit codes and signals, binary output, `PermitTTY no`, `ForceCommand`, a
//! disabled SFTP subsystem, a full disk, missing write permission, symlinks
//! and odd names in listings, and a Dropbear smoke test. Scenarios `io-*`.
//! Run through `tests/sshd/run.sh`.

mod sshd_support;

use ssh_manager_core::ssh::{PtySize, SshError};
use sshd_support::{connect_scenario, step};

const SFTP_SERVER: &str = "/usr/lib/ssh/sftp-server";
const HOME: &str = "/home/testuser";

// ------------------------------------------------------------- exec

#[ignore = "needs sshd containers, see tests/sshd/README.md"]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn exit_codes_0_1_255_are_reported() {
    let mut t = connect_scenario("io-basic").await;
    for code in [0, 1, 255] {
        let out = step("execute(exit)", t.execute(&format!("exit {code}")))
            .await
            .unwrap();
        assert_eq!(out.exit_code, Some(code), "exit {code}");
    }
}

#[ignore = "needs sshd containers, see tests/sshd/README.md"]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn command_killed_by_signal_reports_it_and_does_not_hang() {
    let mut t = connect_scenario("io-basic").await;
    // `step` fails the test after 30 s instead of hanging.
    let out = step("execute(kill -9 $$)", t.execute("kill -9 $$"))
        .await
        .unwrap();
    // Shell convention: 128 + SIGKILL (9).
    assert_eq!(out.exit_code, Some(137));
    // The connection stays usable.
    let out = step("execute(echo ok)", t.execute("echo ok"))
        .await
        .unwrap();
    assert_eq!(out.stdout, b"ok\n");
}

#[ignore = "needs sshd containers, see tests/sshd/README.md"]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn binary_output_arrives_byte_identical() {
    const LEN: usize = 1024 * 1024;
    let mut t = connect_scenario("io-basic").await;
    let out = step(
        "execute(head urandom)",
        t.execute("head -c 1048576 /dev/urandom > /tmp/io.bin && cat /tmp/io.bin"),
    )
    .await
    .unwrap();
    assert_eq!(out.exit_code, Some(0));
    assert!(!out.truncated);
    assert_eq!(out.stdout.len(), LEN);
    // Checksum-equivalent: send the received bytes back and let the server
    // compare them with its copy.
    let cmp = step(
        "execute_with_stdin(cmp)",
        t.execute_with_stdin(
            "cat > /tmp/io2.bin && cmp /tmp/io.bin /tmp/io2.bin && echo identical",
            &out.stdout,
        ),
    )
    .await
    .unwrap();
    assert_eq!(cmp.stdout, b"identical\n", "stderr: {:?}", cmp.stderr);
}

#[ignore = "needs sshd containers, see tests/sshd/README.md"]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn permit_tty_no_fails_pty_shell_but_exec_works() {
    let mut t = connect_scenario("io-nopty").await;
    let shell = step("open_shell", t.open_shell(PtySize { cols: 80, rows: 24 })).await;
    match shell {
        Ok(_) => panic!("PTY shell must fail with PermitTTY no"),
        Err(e) => assert!(
            !e.to_string().is_empty(),
            "error must have a message: {e:?}"
        ),
    }
    let out = step("execute(echo ok)", t.execute("echo ok"))
        .await
        .unwrap();
    assert_eq!(out.stdout, b"ok\n");
    assert_eq!(out.exit_code, Some(0));
}

#[ignore = "needs sshd containers, see tests/sshd/README.md"]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn force_command_output_is_returned_for_exec() {
    let mut t = connect_scenario("io-forced").await;
    let out = step("execute(echo other)", t.execute("echo other"))
        .await
        .unwrap();
    assert_eq!(out.stdout, b"forced-output\n");
    assert_eq!(out.exit_code, Some(0));
}

// ------------------------------------------------------------- SFTP

#[ignore = "needs sshd containers, see tests/sshd/README.md"]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn disabled_sftp_subsystem_fails_clearly_and_exec_fallback_works() {
    let mut t = connect_scenario("io-nosftp").await;
    let started = std::time::Instant::now();
    match step("open_sftp", t.open_sftp()).await {
        Ok(_) => panic!("open_sftp must fail without the sftp subsystem"),
        Err(e) => {
            assert!(
                started.elapsed() < std::time::Duration::from_secs(3),
                "open_sftp took {:?}",
                started.elapsed()
            );
            assert!(
                matches!(e, SshError::ChannelError(_)),
                "unexpected error: {e:?}"
            );
            assert!(
                e.to_string().contains("SFTP-Subsystem"),
                "unexpected message: {e}"
            );
        }
    }
    // The connection survives, and SFTP over an exec channel (the fallback)
    // reaches the same server.
    let out = step("execute(echo ok)", t.execute("echo ok"))
        .await
        .unwrap();
    assert_eq!(out.stdout, b"ok\n");
    let mut sftp = step("open_sftp_via_exec", t.open_sftp_via_exec(SFTP_SERVER))
        .await
        .unwrap();
    let path = format!("{HOME}/fallback.txt");
    step("write", sftp.write_file(&path, b"via exec"))
        .await
        .unwrap();
    assert_eq!(
        step("read", sftp.read_file(&path)).await.unwrap(),
        b"via exec"
    );
}

#[ignore = "needs sshd containers, see tests/sshd/README.md"]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn upload_to_full_disk_is_an_error() {
    let mut t = connect_scenario("io-basic").await;
    let mut sftp = step("open_sftp", t.open_sftp()).await.unwrap();
    // `full` is a symlink to /dev/full: every write fails with ENOSPC.
    let res = step(
        "write_file(full)",
        sftp.write_file(&format!("{HOME}/full"), &vec![b'x'; 256 * 1024]),
    )
    .await;
    assert!(
        res.is_err(),
        "a failed write must not be reported as success"
    );
}

#[ignore = "needs sshd containers, see tests/sshd/README.md"]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn upload_into_unwritable_directory_is_permission_denied() {
    let mut t = connect_scenario("io-basic").await;
    let mut sftp = step("open_sftp", t.open_sftp()).await.unwrap();
    let path = format!("{HOME}/nowrite/file.txt");
    let res = step("write_file", sftp.write_file(&path, b"data")).await;
    assert!(
        matches!(res, Err(SshError::SftpPermissionDenied(_))),
        "got {res:?}"
    );
    assert!(step("lstat", sftp.lstat(&path)).await.is_err());
}

#[ignore = "needs sshd containers, see tests/sshd/README.md"]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn listing_with_symlinks_unicode_and_spaces() {
    let mut t = connect_scenario("io-basic").await;
    let mut sftp = step("open_sftp", t.open_sftp()).await.unwrap();
    let dir = format!("{HOME}/ls");
    let entries = step("list_dir", sftp.list_dir(&dir)).await.unwrap();
    let find = |name: &str| {
        entries
            .iter()
            .find(|e| e.name == name)
            .unwrap_or_else(|| panic!("entry {name:?} missing in {:?}", names(&entries)))
    };
    fn names(entries: &[ssh_manager_core::ssh::RemoteEntry]) -> Vec<&str> {
        entries.iter().map(|e| e.name.as_str()).collect()
    }

    let d = find("dir with space");
    assert!(d.is_dir && !d.is_symlink);
    for plain in ["plain.txt", "ünï cödé 日本.txt", "file with space.txt"] {
        let e = find(plain);
        assert!(!e.is_dir && !e.is_symlink, "{plain}");
        assert_eq!(e.path, format!("{dir}/{plain}"));
    }
    for link in ["link-file", "link-dir", "dangling"] {
        assert!(find(link).is_symlink, "{link} must be marked as a symlink");
    }
    // Targets resolve as expected through stat; the dangling one does not.
    assert!(
        !step("stat", sftp.stat(&format!("{dir}/link-file")))
            .await
            .unwrap()
            .is_dir
    );
    assert!(
        step("stat", sftp.stat(&format!("{dir}/link-dir")))
            .await
            .unwrap()
            .is_dir
    );
    assert!(step("stat", sftp.stat(&format!("{dir}/dangling")))
        .await
        .is_err());
    assert!(
        step("lstat", sftp.lstat(&format!("{dir}/dangling")))
            .await
            .unwrap()
            .is_symlink
    );
    // Odd names round-trip through read_file.
    let bytes = step("read", sftp.read_file(&format!("{dir}/ünï cödé 日本.txt")))
        .await
        .unwrap();
    assert_eq!(bytes, b"uni\n");
}

// ------------------------------------------------------------- Dropbear

#[ignore = "needs sshd containers, see tests/sshd/README.md"]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dropbear_exec_and_sftp_smoke() {
    let mut t = connect_scenario("io-dropbear").await;
    let out = step("execute(echo ok)", t.execute("echo ok"))
        .await
        .unwrap();
    assert_eq!(out.stdout, b"ok\n");
    let out = step("execute(exit 7)", t.execute("exit 7")).await.unwrap();
    assert_eq!(out.exit_code, Some(7));

    // Prefer the real subsystem; if this Dropbear build has none, the exec
    // channel with the same server binary must work.
    let mut sftp = match step("open_sftp", t.open_sftp()).await {
        Ok(s) => s,
        Err(e) => {
            eprintln!("dropbear: sftp subsystem unavailable ({e}); using exec channel");
            step("open_sftp_via_exec", t.open_sftp_via_exec(SFTP_SERVER))
                .await
                .unwrap()
        }
    };
    let path = format!("{HOME}/smoke.txt");
    step("write", sftp.write_file(&path, b"dropbear"))
        .await
        .unwrap();
    assert_eq!(
        step("read", sftp.read_file(&path)).await.unwrap(),
        b"dropbear"
    );
    let entries = step("list_dir", sftp.list_dir(HOME)).await.unwrap();
    assert!(entries.iter().any(|e| e.name == "smoke.txt"));
}
