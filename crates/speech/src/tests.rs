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
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/ggml-base.bin", listener.local_addr().unwrap());
    let hits = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&hits);
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            counter.fetch_add(1, Ordering::SeqCst);
            let body = body.clone();
            tokio::spawn(async move {
                let mut buf = [0u8; 2048];
                let _ = sock.read(&mut buf).await;
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

fn manager(dir: &Path, url: String, body: &[u8], hash: String) -> ModelManager {
    let src = ModelSource {
        url,
        size: body.len() as u64,
        sha256: hash,
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
        assert!(s
            .url
            .starts_with("https://huggingface.co/ggerganov/whisper.cpp/resolve/"));
        assert!(!s.url.contains("/resolve/main/"));
        assert!(s.url.contains(REVISION));
        assert_eq!(s.sha256.len(), 64);
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
        std::fs::read(m.model_path(SpeechModel::Base)).unwrap(),
        body
    );
    assert!(!part_path(dir.path(), SpeechModel::Base).exists());
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
    assert!(!part_path(dir.path(), SpeechModel::Base).exists());
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
    assert!(part_path(dir.path(), SpeechModel::Base).exists());
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
    assert!(!part_path(dir.path(), SpeechModel::Base).exists());
    assert!(!m.model_path(SpeechModel::Base).exists());
    assert_eq!(m.status(SpeechModel::Base), ModelStatus::NotDownloaded);
}

#[tokio::test]
async fn oversized_body_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let body = vec![3u8; 5_000];
    let (url, _) = serve(body.clone(), 200, false).await;
    let src = ModelSource {
        url,
        size: 1_000,
        sha256: sha_hex(&body),
    };
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
    std::fs::write(m.model_path(SpeechModel::Base), b"x").unwrap();
    assert_eq!(m.status(SpeechModel::Base), ModelStatus::Ready);
    m.delete(SpeechModel::Base).unwrap();
    assert_eq!(m.status(SpeechModel::Base), ModelStatus::NotDownloaded);
    assert!(!m.model_path(SpeechModel::Base).exists());
    // Deleting again is fine.
    m.delete(SpeechModel::Base).unwrap();
}
