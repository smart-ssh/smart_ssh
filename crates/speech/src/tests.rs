use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use super::*;

fn sha_hex(data: &[u8]) -> String {
    hex(&Sha256::digest(data))
}

/// Local HTTP fixture. `stall`: send only half of the body, then hang.
/// Returns the URL and a request counter.
async fn serve(body: Vec<u8>, status: u16, stall: bool) -> (String, Arc<AtomicUsize>) {
    let (base, hits) = serve_routes(vec![("/model.onnx".into(), body, status, stall)]).await;
    (format!("{base}/model.onnx"), hits)
}

/// Like `serve`, with one body per request path. Returns the base URL.
async fn serve_routes(routes: Vec<(String, Vec<u8>, u16, bool)>) -> (String, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let hits = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&hits);
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            counter.fetch_add(1, Ordering::SeqCst);
            let routes = routes.clone();
            tokio::spawn(async move {
                let mut buf = [0u8; 2048];
                let n = sock.read(&mut buf).await.unwrap_or(0);
                let request = String::from_utf8_lossy(&buf[..n]).into_owned();
                let path = request.split_whitespace().nth(1).unwrap_or("").to_owned();
                let Some((_, body, status, stall)) = routes.into_iter().find(|r| r.0 == path)
                else {
                    let _ = sock
                        .write_all(
                            b"HTTP/1.1 404 X\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                        )
                        .await;
                    return;
                };
                let head = format!(
                    "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = sock.write_all(head.as_bytes()).await;
                if stall {
                    let _ = sock.write_all(&body[..body.len() / 2]).await;
                    tokio::time::sleep(Duration::from_secs(60)).await;
                } else {
                    let _ = sock.write_all(&body).await;
                }
            });
        }
    });
    (url, hits)
}

fn part(dir: &Path) -> PathBuf {
    dir.join(SpeechModel::Base.file_name())
        .join("model.onnx.part")
}

fn file(name: &str, url: String, body: &[u8], hash: String) -> ModelFile {
    ModelFile {
        name: name.to_owned(),
        url,
        size: body.len() as u64,
        sha256: hash,
    }
}

fn manager(dir: &Path, url: String, body: &[u8], hash: String) -> ModelManager {
    let src = ModelSource {
        files: vec![file("model.onnx", url, body, hash)],
    };
    ModelManager::with_sources(dir.to_path_buf(), HashMap::from([(SpeechModel::Base, src)]))
}

type Seen = Arc<Mutex<Vec<ModelStatus>>>;

fn recorder() -> (StatusCallback, Seen) {
    let seen: Seen = Arc::default();
    let s = Arc::clone(&seen);
    (Arc::new(move |_, st| s.lock().unwrap().push(st)), seen)
}

async fn wait_final(seen: &Seen) -> ModelStatus {
    for _ in 0..500 {
        if let Some(last) = seen.lock().unwrap().last() {
            if !matches!(last, ModelStatus::Downloading { .. }) {
                return last.clone();
            }
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("download did not finish");
}

#[test]
fn constants_are_pinned() {
    for m in SpeechModel::ALL {
        let s = m.source();
        assert!(!s.files.is_empty());
        for f in &s.files {
            assert!(!f.url.contains("/resolve/main/"));
            assert_eq!(f.sha256.len(), 64);
            assert!(f.url.ends_with(&f.name));
            assert!(f.size > 0);
        }
        let (repo, rev) = match m.engine() {
            Engine::Whisper if m == SpeechModel::Small => {
                ("onnx-community/whisper-small", WHISPER_SMALL_REVISION)
            }
            Engine::Whisper => ("onnx-community/whisper-base", WHISPER_BASE_REVISION),
            Engine::Parakeet => ("istupakov/parakeet-tdt-0.6b-v3-onnx", PARAKEET_REVISION),
        };
        for f in &s.files {
            assert!(f
                .url
                .starts_with(&format!("https://huggingface.co/{repo}/resolve/{rev}/")));
        }
    }
    for m in [SpeechModel::Base, SpeechModel::Small] {
        let names: Vec<_> = m.source().files.iter().map(|f| f.name.clone()).collect();
        assert_eq!(
            names,
            [
                "encoder_model_int8.onnx",
                "decoder_model_merged_int8.onnx",
                "vocab.json",
                "added_tokens.json",
                "config.json"
            ]
        );
    }
    assert_eq!(SpeechModel::Base.source().total_size(), 77_968_043);
    assert_eq!(SpeechModel::Small.source().total_size(), 250_150_387);
    assert_eq!(SpeechModel::ParakeetV3.source().files.len(), 4);
    assert_eq!(SpeechModel::ParakeetV3.source().total_size(), 670_619_706);
    assert_eq!(
        SpeechModel::from_id("parakeet_v3"),
        Some(SpeechModel::ParakeetV3)
    );
    assert_eq!(SpeechModel::ParakeetV3.id(), "parakeet_v3");
    for m in SpeechModel::ALL {
        assert_eq!(SpeechModel::from_id(m.id()), Some(m));
    }
    assert_eq!(SpeechModel::DEFAULT, SpeechModel::Base);
    assert_eq!(SpeechModel::from_id("small"), Some(SpeechModel::Small));
    assert_eq!(SpeechModel::from_id("tiny"), None);
}

#[tokio::test]
async fn status_is_local_only_and_not_downloaded_without_file() {
    let dir = tempfile::tempdir().unwrap();
    let (url, hits) = serve(vec![], 200, false).await;
    let m = manager(dir.path(), url, b"", String::new());
    assert_eq!(m.status(SpeechModel::Base), ModelStatus::NotDownloaded);
    assert_eq!(m.status(SpeechModel::Small), ModelStatus::NotDownloaded);
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(hits.load(Ordering::SeqCst), 0, "no network request");
}

#[tokio::test]
async fn download_verifies_and_becomes_ready() {
    let dir = tempfile::tempdir().unwrap();
    let body: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
    let (url, _) = serve(body.clone(), 200, false).await;
    let m = manager(dir.path(), url, &body, sha_hex(&body));
    let (cb, seen) = recorder();
    m.start_download(SpeechModel::Base, cb).unwrap();
    assert_eq!(wait_final(&seen).await, ModelStatus::Ready);
    assert!(seen
        .lock()
        .unwrap()
        .iter()
        .any(|s| matches!(s, ModelStatus::Downloading { done, total } if *done > 0 && *total == body.len() as u64)));
    assert_eq!(m.status(SpeechModel::Base), ModelStatus::Ready);
    assert_eq!(
        std::fs::read(m.model_path(SpeechModel::Base).join("model.onnx")).unwrap(),
        body
    );
    assert!(!part(dir.path()).exists());
    // The other model is unaffected.
    assert_eq!(m.status(SpeechModel::Small), ModelStatus::NotDownloaded);
}

#[tokio::test]
async fn hash_mismatch_deletes_file_and_reports_error() {
    let dir = tempfile::tempdir().unwrap();
    let body = vec![7u8; 10_000];
    let (url, _) = serve(body.clone(), 200, false).await;
    let m = manager(dir.path(), url, &body, sha_hex(b"something else"));
    let (cb, seen) = recorder();
    m.start_download(SpeechModel::Base, cb).unwrap();
    assert_eq!(
        wait_final(&seen).await,
        ModelStatus::Error {
            key: "speech_model_hash_mismatch"
        }
    );
    assert!(!m.model_path(SpeechModel::Base).exists());
    assert!(!part(dir.path()).exists());
    assert_eq!(
        m.status(SpeechModel::Base),
        ModelStatus::Error {
            key: "speech_model_hash_mismatch"
        }
    );
    // Deleting clears the error.
    m.delete(SpeechModel::Base).unwrap();
    assert_eq!(m.status(SpeechModel::Base), ModelStatus::NotDownloaded);
}

#[tokio::test]
async fn http_error_status_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let (url, _) = serve(b"nope".to_vec(), 404, false).await;
    let m = manager(dir.path(), url, b"nope", sha_hex(b"nope"));
    let (cb, seen) = recorder();
    m.start_download(SpeechModel::Base, cb).unwrap();
    assert_eq!(
        wait_final(&seen).await,
        ModelStatus::Error {
            key: "speech_model_http_status"
        }
    );
    assert!(!m.model_path(SpeechModel::Base).exists());
}

#[tokio::test]
async fn cancel_stops_transfer_and_removes_temp_file() {
    let dir = tempfile::tempdir().unwrap();
    let body = vec![1u8; 100_000];
    let (url, _) = serve(body.clone(), 200, true).await;
    let m = manager(dir.path(), url, &body, sha_hex(&body));
    let (cb, seen) = recorder();
    m.start_download(SpeechModel::Base, cb).unwrap();
    // Wait until bytes flow (temp file exists with data).
    for _ in 0..500 {
        if matches!(m.status(SpeechModel::Base), ModelStatus::Downloading { done, .. } if done > 0)
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(part(dir.path()).exists());
    assert_eq!(
        m.start_download(SpeechModel::Base, recorder().0),
        Err(ModelManagerError::AlreadyDownloading)
    );
    assert_eq!(
        m.delete(SpeechModel::Base),
        Err(ModelManagerError::Downloading)
    );
    assert!(m.cancel_download(SpeechModel::Base));
    assert_eq!(wait_final(&seen).await, ModelStatus::NotDownloaded);
    assert!(!part(dir.path()).exists());
    assert!(!m.model_path(SpeechModel::Base).exists());
    assert_eq!(m.status(SpeechModel::Base), ModelStatus::NotDownloaded);
}

#[tokio::test]
async fn oversized_body_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let body = vec![3u8; 5_000];
    let (url, _) = serve(body.clone(), 200, false).await;
    let mut f = file("model.onnx", url, &body, sha_hex(&body));
    f.size = 1_000;
    let src = ModelSource { files: vec![f] };
    let m = ModelManager::with_sources(
        dir.path().to_path_buf(),
        HashMap::from([(SpeechModel::Base, src)]),
    );
    let (cb, seen) = recorder();
    m.start_download(SpeechModel::Base, cb).unwrap();
    assert_eq!(
        wait_final(&seen).await,
        ModelStatus::Error {
            key: "speech_model_hash_mismatch"
        }
    );
    assert!(!m.model_path(SpeechModel::Base).exists());
}

#[tokio::test]
async fn delete_removes_file() {
    let dir = tempfile::tempdir().unwrap();
    let m = manager(dir.path(), String::new(), b"", String::new());
    std::fs::create_dir_all(m.model_path(SpeechModel::Base)).unwrap();
    std::fs::write(m.model_path(SpeechModel::Base).join("model.onnx"), b"x").unwrap();
    assert_eq!(m.status(SpeechModel::Base), ModelStatus::Ready);
    m.delete(SpeechModel::Base).unwrap();
    assert_eq!(m.status(SpeechModel::Base), ModelStatus::NotDownloaded);
    assert!(!m.model_path(SpeechModel::Base).exists());
    // Deleting again is fine.
    m.delete(SpeechModel::Base).unwrap();
}

/// Issue #1332: discarding leftovers never removes an installed model.
#[tokio::test]
async fn delete_partial_keeps_the_installed_model_and_removes_part_files() {
    let dir = tempfile::tempdir().unwrap();
    let m = manager(dir.path(), String::new(), b"", String::new());
    let d = m.model_path(SpeechModel::Base);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("model.onnx"), b"x").unwrap();
    std::fs::write(d.join("model.onnx.part"), b"y").unwrap();
    m.delete_partial(SpeechModel::Base).unwrap();
    assert_eq!(m.status(SpeechModel::Base), ModelStatus::Ready);
    assert!(!d.join("model.onnx.part").exists());
    // Nothing to remove is fine.
    m.delete_partial(SpeechModel::Base).unwrap();
}

/// Parakeet-shaped fixture: several files in the model's own directory.
fn parakeet_manager(
    dir: &Path,
    base: &str,
    bodies: &[(&str, Vec<u8>)],
    bad: Option<&str>,
) -> ModelManager {
    let files = bodies
        .iter()
        .map(|(name, body)| {
            let hash = if Some(*name) == bad {
                sha_hex(b"other")
            } else {
                sha_hex(body)
            };
            file(name, format!("{base}/{name}"), body, hash)
        })
        .collect();
    ModelManager::with_sources(
        dir.to_path_buf(),
        HashMap::from([(SpeechModel::ParakeetV3, ModelSource { files })]),
    )
}

fn bodies() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("a.onnx", vec![1u8; 40_000]),
        ("b.onnx", vec![2u8; 30_000]),
        ("vocab.txt", vec![3u8; 10_000]),
    ]
}

async fn serve_bodies(bodies: &[(&str, Vec<u8>)], stall_on: Option<&str>) -> String {
    let routes = bodies
        .iter()
        .map(|(n, b)| (format!("/{n}"), b.clone(), 200, Some(*n) == stall_on))
        .collect();
    serve_routes(routes).await.0
}

fn leftovers(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir.join(SpeechModel::ParakeetV3.file_name())) {
        out.extend(
            rd.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned()),
        );
    }
    out
}

#[tokio::test]
async fn multi_file_download_reports_combined_progress_and_becomes_ready() {
    let dir = tempfile::tempdir().unwrap();
    let bodies = bodies();
    let base = serve_bodies(&bodies, None).await;
    let m = parakeet_manager(dir.path(), &base, &bodies, None);
    let total = 80_000u64;
    assert_eq!(
        m.status(SpeechModel::ParakeetV3),
        ModelStatus::NotDownloaded
    );
    let (cb, seen) = recorder();
    m.start_download(SpeechModel::ParakeetV3, cb).unwrap();
    assert_eq!(wait_final(&seen).await, ModelStatus::Ready);
    let seen = seen.lock().unwrap();
    let dones: Vec<u64> = seen
        .iter()
        .filter_map(|s| match s {
            ModelStatus::Downloading { done, total: t } => {
                assert_eq!(*t, total);
                Some(*done)
            }
            _ => None,
        })
        .collect();
    assert!(
        dones.windows(2).all(|w| w[0] <= w[1]),
        "monotonic: {dones:?}"
    );
    assert_eq!(dones.last(), Some(&total));
    drop(seen);
    assert_eq!(m.status(SpeechModel::ParakeetV3), ModelStatus::Ready);
    assert!(m.model_path(SpeechModel::ParakeetV3).is_dir());
    let mut names = leftovers(dir.path());
    names.sort();
    assert_eq!(names, ["a.onnx", "b.onnx", "vocab.txt"]);
}

#[tokio::test]
async fn hash_mismatch_in_any_file_leaves_no_file_of_the_model() {
    for bad in ["a.onnx", "b.onnx", "vocab.txt"] {
        let dir = tempfile::tempdir().unwrap();
        let bodies = bodies();
        let base = serve_bodies(&bodies, None).await;
        let m = parakeet_manager(dir.path(), &base, &bodies, Some(bad));
        let (cb, seen) = recorder();
        m.start_download(SpeechModel::ParakeetV3, cb).unwrap();
        assert_eq!(
            wait_final(&seen).await,
            ModelStatus::Error {
                key: "speech_model_hash_mismatch"
            },
            "{bad}"
        );
        assert_eq!(leftovers(dir.path()), Vec::<String>::new(), "{bad}");
        m.delete(SpeechModel::ParakeetV3).unwrap();
        assert_eq!(
            m.status(SpeechModel::ParakeetV3),
            ModelStatus::NotDownloaded
        );
    }
}

#[tokio::test]
async fn model_with_a_missing_file_is_not_ready() {
    let dir = tempfile::tempdir().unwrap();
    let bodies = bodies();
    let base = serve_bodies(&bodies, None).await;
    let m = parakeet_manager(dir.path(), &base, &bodies, None);
    let model_dir = m.model_path(SpeechModel::ParakeetV3);
    std::fs::create_dir_all(&model_dir).unwrap();
    std::fs::write(model_dir.join("a.onnx"), b"x").unwrap();
    std::fs::write(model_dir.join("b.onnx"), b"x").unwrap();
    assert_eq!(
        m.status(SpeechModel::ParakeetV3),
        ModelStatus::NotDownloaded
    );
    std::fs::write(model_dir.join("vocab.txt"), b"x").unwrap();
    assert_eq!(m.status(SpeechModel::ParakeetV3), ModelStatus::Ready);
}

#[tokio::test]
async fn cancel_during_a_later_file_removes_everything_of_the_model() {
    let dir = tempfile::tempdir().unwrap();
    let bodies = bodies();
    let base = serve_bodies(&bodies, Some("b.onnx")).await;
    let m = parakeet_manager(dir.path(), &base, &bodies, None);
    let (cb, seen) = recorder();
    m.start_download(SpeechModel::ParakeetV3, cb).unwrap();
    // The first file is done (40 000 bytes) and the second one is flowing.
    for _ in 0..1000 {
        if matches!(m.status(SpeechModel::ParakeetV3), ModelStatus::Downloading { done, .. } if done > 40_000)
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(leftovers(dir.path()).contains(&"a.onnx.part".to_owned()));
    assert!(m.cancel_download(SpeechModel::ParakeetV3));
    assert_eq!(wait_final(&seen).await, ModelStatus::NotDownloaded);
    assert_eq!(leftovers(dir.path()), Vec::<String>::new());
    assert_eq!(
        m.status(SpeechModel::ParakeetV3),
        ModelStatus::NotDownloaded
    );
}

#[tokio::test]
async fn delete_removes_all_files_of_the_model_and_keeps_foreign_files() {
    let dir = tempfile::tempdir().unwrap();
    let bodies = bodies();
    let base = serve_bodies(&bodies, None).await;
    let m = parakeet_manager(dir.path(), &base, &bodies, None);
    let (cb, seen) = recorder();
    m.start_download(SpeechModel::ParakeetV3, cb).unwrap();
    assert_eq!(wait_final(&seen).await, ModelStatus::Ready);
    std::fs::write(dir.path().join("foreign.bin"), b"whisper").unwrap();
    m.delete(SpeechModel::ParakeetV3).unwrap();
    assert!(!m.model_path(SpeechModel::ParakeetV3).exists());
    assert_eq!(
        m.status(SpeechModel::ParakeetV3),
        ModelStatus::NotDownloaded
    );
    assert_eq!(
        std::fs::read(dir.path().join("foreign.bin")).unwrap(),
        b"whisper"
    );
}

#[test]
fn availability_matches_the_platform() {
    let ids: Vec<&str> = SpeechModel::available().iter().map(|m| m.id()).collect();
    // Parakeet is offered on every platform, Android included (issue #1300).
    assert_eq!(ids, ["base", "small", "parakeet_v3"]);
    assert!(SpeechModel::ParakeetV3.is_available());
}

#[test]
fn model_info_lists_engine_size_and_source() {
    let infos = SpeechModel::available_info();
    let base = &infos[0];
    assert_eq!(base.id, "base");
    assert_eq!(base.engine, Engine::Whisper);
    assert_eq!(base.total_size, 77_968_043);
    assert_eq!(base.source_host, "huggingface.co");
    assert_eq!(base.source_repo, "onnx-community/whisper-base");
    let json = serde_json::to_value(base).unwrap();
    assert_eq!(json["engine"], "whisper");
    assert_eq!(json["total_size"], 77_968_043);
    if let Some(p) = infos.iter().find(|i| i.id == "parakeet_v3") {
        assert_eq!(p.engine, Engine::Parakeet);
        assert_eq!(p.source_repo, "istupakov/parakeet-tdt-0.6b-v3-onnx");
        assert_eq!(p.total_size, 670_619_706);
    }
}

#[test]
fn legacy_ggml_files_are_removed_and_nothing_else() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    for name in [
        "ggml-base.bin",
        "ggml-small.bin",
        "ggml-base.bin.part",
        "ggml-small.bin.part",
        "other.bin",
    ] {
        std::fs::write(p.join(name), b"x").unwrap();
    }
    std::fs::create_dir(p.join("whisper-base-int8")).unwrap();
    std::fs::write(p.join("whisper-base-int8").join("config.json"), b"x").unwrap();
    remove_legacy_files(p);
    let mut left: Vec<String> = std::fs::read_dir(p)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    left.sort();
    assert_eq!(left, ["other.bin", "whisper-base-int8"]);
    // A directory that does not exist is fine, too.
    remove_legacy_files(&p.join("missing"));
    let m = ModelManager::new(p.to_path_buf());
    assert_eq!(m.status(SpeechModel::Base), ModelStatus::NotDownloaded);
    assert_eq!(m.status(SpeechModel::Small), ModelStatus::NotDownloaded);
}

/// Fixture for resume tests: answers `Range: bytes=N-` with 206 when
/// `honor`, else always with the full body (200). Returns the URL and the
/// `Range` header values seen (`None` = no header).
async fn serve_range(body: Vec<u8>, honor: bool) -> (String, Arc<Mutex<Vec<Option<String>>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/model.onnx", listener.local_addr().unwrap());
    let seen: Arc<Mutex<Vec<Option<String>>>> = Arc::default();
    let log = Arc::clone(&seen);
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            let body = body.clone();
            let log = Arc::clone(&log);
            tokio::spawn(async move {
                let mut buf = [0u8; 2048];
                let n = sock.read(&mut buf).await.unwrap_or(0);
                let request = String::from_utf8_lossy(&buf[..n]).into_owned();
                let range = request.lines().find_map(|l| {
                    l.to_ascii_lowercase()
                        .strip_prefix("range:")
                        .map(|v| v.trim().to_owned())
                });
                log.lock().unwrap().push(range.clone());
                let start = range
                    .as_deref()
                    .and_then(|r| r.strip_prefix("bytes="))
                    .and_then(|r| r.strip_suffix('-'))
                    .and_then(|r| r.parse::<usize>().ok());
                let (head, payload) = match start {
                    Some(s) if honor => (
                        format!(
                            "HTTP/1.1 206 X\r\nContent-Length: {}\r\nContent-Range: bytes {s}-{}/{}\r\nConnection: close\r\n\r\n",
                            body.len() - s,
                            body.len() - 1,
                            body.len()
                        ),
                        body[s..].to_vec(),
                    ),
                    _ => (
                        format!(
                            "HTTP/1.1 200 X\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            body.len()
                        ),
                        body.clone(),
                    ),
                };
                let _ = sock.write_all(head.as_bytes()).await;
                let _ = sock.write_all(&payload).await;
            });
        }
    });
    (url, seen)
}

fn leave_part(dir: &Path, bytes: &[u8]) {
    let p = part(dir);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, bytes).unwrap();
}

#[tokio::test]
async fn resume_continues_from_the_part_file_and_hashes_the_whole_file() {
    let dir = tempfile::tempdir().unwrap();
    let body: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
    let (url, seen) = serve_range(body.clone(), true).await;
    let m = manager(dir.path(), url, &body, sha_hex(&body));
    leave_part(dir.path(), &body[..70_000]);
    let (cb, st) = recorder();
    m.resume_download(SpeechModel::Base, cb).unwrap();
    assert_eq!(wait_final(&st).await, ModelStatus::Ready);
    assert_eq!(
        seen.lock().unwrap().as_slice(),
        [Some("bytes=70000-".to_owned())]
    );
    assert_eq!(
        std::fs::read(m.model_path(SpeechModel::Base).join("model.onnx")).unwrap(),
        body
    );
    // Progress started at the resumed offset, not at zero.
    assert!(st
        .lock()
        .unwrap()
        .iter()
        .all(|s| !matches!(s, ModelStatus::Downloading { done, .. } if *done < 70_000)));
}

#[tokio::test]
async fn resume_hash_check_covers_the_kept_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let body: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
    let (url, _) = serve_range(body.clone(), true).await;
    let m = manager(dir.path(), url, &body, sha_hex(&body));
    // Corrupt kept bytes: the tail is right, the whole file must not verify.
    leave_part(dir.path(), &vec![0xAB; 70_000]);
    let (cb, st) = recorder();
    m.resume_download(SpeechModel::Base, cb).unwrap();
    assert_eq!(
        wait_final(&st).await,
        ModelStatus::Error {
            key: ModelErrorKey::HashMismatch.key()
        }
    );
    assert!(!part(dir.path()).exists());
}

#[tokio::test]
async fn server_ignoring_range_restarts_from_zero_without_corruption() {
    let dir = tempfile::tempdir().unwrap();
    let body: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
    let (url, seen) = serve_range(body.clone(), false).await;
    let m = manager(dir.path(), url, &body, sha_hex(&body));
    leave_part(dir.path(), &body[..70_000]);
    let (cb, st) = recorder();
    m.resume_download(SpeechModel::Base, cb).unwrap();
    assert_eq!(wait_final(&st).await, ModelStatus::Ready);
    assert_eq!(seen.lock().unwrap().len(), 1);
    assert_eq!(
        std::fs::read(m.model_path(SpeechModel::Base).join("model.onnx")).unwrap(),
        body
    );
}

#[tokio::test]
async fn plain_start_ignores_a_leftover_part_file() {
    let dir = tempfile::tempdir().unwrap();
    let body: Vec<u8> = (0..50_000u32).map(|i| (i % 251) as u8).collect();
    let (url, seen) = serve_range(body.clone(), true).await;
    let m = manager(dir.path(), url, &body, sha_hex(&body));
    leave_part(dir.path(), b"stale");
    let (cb, st) = recorder();
    m.start_download(SpeechModel::Base, cb).unwrap();
    assert_eq!(wait_final(&st).await, ModelStatus::Ready);
    assert_eq!(seen.lock().unwrap().as_slice(), [None]);
}

#[tokio::test]
async fn resume_with_a_complete_part_file_only_verifies() {
    let dir = tempfile::tempdir().unwrap();
    let body: Vec<u8> = (0..50_000u32).map(|i| (i % 251) as u8).collect();
    let (url, seen) = serve_range(body.clone(), true).await;
    let m = manager(dir.path(), url, &body, sha_hex(&body));
    leave_part(dir.path(), &body);
    let (cb, st) = recorder();
    m.resume_download(SpeechModel::Base, cb).unwrap();
    assert_eq!(wait_final(&st).await, ModelStatus::Ready);
    assert!(seen.lock().unwrap().is_empty());
}

#[tokio::test]
async fn delete_removes_the_part_file_of_an_interrupted_download() {
    let dir = tempfile::tempdir().unwrap();
    let body = b"abcdef".to_vec();
    let m = manager(
        dir.path(),
        "http://127.0.0.1:1/x".into(),
        &body,
        sha_hex(&body),
    );
    leave_part(dir.path(), b"abc");
    assert!(part(dir.path()).exists());
    m.delete(SpeechModel::Base).unwrap();
    assert!(!part(dir.path()).exists());
}
