//! Issue #625: `posix-rename@openssh.com` neben `russh-sftp`.
//!
//! `russh-sftp` 2.4 wrappt die Erweiterung nicht und gibt weder die vom Server
//! gemeldeten Erweiterungen noch die rohe Sitzung heraus. Deshalb läuft der
//! Stream des SFTP-Kanals durch einen kleinen Vermittler, der paketweise
//! (u32-Länge + Rumpf) in beide Richtungen weiterreicht und dabei
//!
//! - in der `VERSION`-Antwort mitliest, ob der Server die Erweiterung anbietet,
//! - bei Bedarf selbst ein `EXTENDED`-Paket einschiebt (immer zwischen zwei
//!   ganzen Paketen) und dessen `STATUS`-Antwort abfängt, statt sie an
//!   `russh-sftp` weiterzugeben. Die Anfrage-IDs liegen ab
//!   [`TAP_ID_BASE`], `russh-sftp` zählt von 0 hoch — keine Überschneidung.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use tokio::io::{duplex, split, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, DuplexStream};
use tokio::sync::{oneshot, Mutex};

const EXTENSION: &str = "posix-rename@openssh.com";
const TAP_ID_BASE: u32 = 0x8000_0000;
const FXP_VERSION: u8 = 2;
const FXP_STATUS: u8 = 101;
const FXP_EXTENDED: u8 = 200;
/// Obergrenze für ein Paket, das der Vermittler puffert (wie `russh-sftp`s
/// Voreinstellung großzügig überschritten; größere Pakete sind kaputt).
const MAX_PACKET: u32 = 16 * 1024 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// Antwort auf eine eingeschobene Anfrage: Statuscode und Meldung.
pub(crate) type TapStatus = (u32, String);

type Writer = Arc<Mutex<Box<dyn AsyncWrite + Unpin + Send>>>;

pub(crate) struct SftpTap {
    posix_rename: AtomicBool,
    writer: Writer,
    pending: StdMutex<HashMap<u32, oneshot::Sender<TapStatus>>>,
    next_id: AtomicU32,
}

/// Fehler beim Einschieben der Anfrage (nicht: Fehler-Status des Servers).
#[derive(Debug)]
pub(crate) enum TapError {
    Closed,
    Timeout,
}

impl SftpTap {
    pub(crate) fn supports_posix_rename(&self) -> bool {
        self.posix_rename.load(Ordering::Acquire)
    }

    /// Sendet `posix-rename@openssh.com` und wartet auf den Status.
    pub(crate) async fn posix_rename(&self, from: &str, to: &str) -> Result<TapStatus, TapError> {
        let id = TAP_ID_BASE + (self.next_id.fetch_add(1, Ordering::Relaxed) & 0x3fff_ffff);
        let mut body = vec![FXP_EXTENDED];
        body.extend_from_slice(&id.to_be_bytes());
        for s in [EXTENSION, from, to] {
            body.extend_from_slice(&(s.len() as u32).to_be_bytes());
            body.extend_from_slice(s.as_bytes());
        }
        let (tx, rx) = oneshot::channel();
        self.pending.lock().unwrap().insert(id, tx);
        let written = {
            let mut w = self.writer.lock().await;
            let mut packet = (body.len() as u32).to_be_bytes().to_vec();
            packet.extend_from_slice(&body);
            match w.write_all(&packet).await {
                Ok(()) => w.flush().await,
                Err(e) => Err(e),
            }
        };
        if written.is_err() {
            self.pending.lock().unwrap().remove(&id);
            return Err(TapError::Closed);
        }
        match tokio::time::timeout(REQUEST_TIMEOUT, rx).await {
            Ok(Ok(status)) => Ok(status),
            Ok(Err(_)) => Err(TapError::Closed),
            Err(_) => {
                self.pending.lock().unwrap().remove(&id);
                Err(TapError::Timeout)
            }
        }
    }
}

async fn read_packet<R: AsyncRead + Unpin>(r: &mut R) -> std::io::Result<Vec<u8>> {
    let len = r.read_u32().await?;
    if len > MAX_PACKET {
        return Err(std::io::Error::other("SFTP-Paket zu groß"));
    }
    let mut body = vec![0u8; len as usize];
    r.read_exact(&mut body).await?;
    Ok(body)
}

fn be_u32(b: &[u8], at: usize) -> Option<u32> {
    b.get(at..at + 4)
        .map(|s| u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
}

fn read_string(b: &[u8], at: usize) -> Option<(String, usize)> {
    let len = be_u32(b, at)? as usize;
    let s = b.get(at + 4..at + 4 + len)?;
    Some((String::from_utf8_lossy(s).into_owned(), at + 4 + len))
}

/// `VERSION`: type, u32 version, dann (Name, Daten)-Paare.
fn advertises_posix_rename(body: &[u8]) -> bool {
    let mut at = 5;
    while let Some((name, next)) = read_string(body, at) {
        let Some((data, next)) = read_string(body, next) else {
            return false;
        };
        if name == EXTENSION && data == "1" {
            return true;
        }
        at = next;
    }
    false
}

/// `STATUS`: type, u32 id, u32 code, string message.
fn parse_status(body: &[u8]) -> Option<(u32, TapStatus)> {
    let id = be_u32(body, 1)?;
    let code = be_u32(body, 5)?;
    let message = read_string(body, 9).map(|(m, _)| m).unwrap_or_default();
    Some((id, (code, message)))
}

/// Schaltet den Vermittler vor `stream`. Der Rückgabe-Stream gehört
/// `russh-sftp`, das Handle dem Aufrufer.
pub(crate) fn tap<S>(stream: S) -> (DuplexStream, Arc<SftpTap>)
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (client_side, pump_side) = duplex(256 * 1024);
    let (mut remote_rd, remote_wr) = split(stream);
    let (mut client_rd, mut client_wr) = split(pump_side);
    let writer: Writer = Arc::new(Mutex::new(Box::new(remote_wr)));
    let shared = Arc::new(SftpTap {
        posix_rename: AtomicBool::new(false),
        writer: writer.clone(),
        pending: StdMutex::new(HashMap::new()),
        next_id: AtomicU32::new(0),
    });

    // russh-sftp -> Server
    let up_writer = writer.clone();
    tokio::spawn(async move {
        while let Ok(body) = read_packet(&mut client_rd).await {
            let mut w = up_writer.lock().await;
            let mut packet = (body.len() as u32).to_be_bytes().to_vec();
            packet.extend_from_slice(&body);
            if w.write_all(&packet).await.is_err() || w.flush().await.is_err() {
                return;
            }
        }
        let _ = up_writer.lock().await.shutdown().await;
    });

    // Server -> russh-sftp
    let down = shared.clone();
    tokio::spawn(async move {
        while let Ok(body) = read_packet(&mut remote_rd).await {
            match body.first().copied() {
                Some(FXP_VERSION) if advertises_posix_rename(&body) => {
                    down.posix_rename.store(true, Ordering::Release);
                }
                Some(FXP_STATUS) => {
                    if let Some((id, status)) = parse_status(&body) {
                        if id >= TAP_ID_BASE {
                            if let Some(tx) = down.pending.lock().unwrap().remove(&id) {
                                let _ = tx.send(status);
                            }
                            continue;
                        }
                    }
                }
                _ => {}
            }
            let mut packet = (body.len() as u32).to_be_bytes().to_vec();
            packet.extend_from_slice(&body);
            if client_wr.write_all(&packet).await.is_err() {
                break;
            }
        }
        // Wartende Anfragen scheitern (Sender fallen weg), russh-sftp sieht EOF.
        down.pending.lock().unwrap().clear();
        let _ = client_wr.shutdown().await;
    });

    (client_side, shared)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put_str(out: &mut Vec<u8>, s: &str) {
        out.extend_from_slice(&(s.len() as u32).to_be_bytes());
        out.extend_from_slice(s.as_bytes());
    }

    async fn send(w: &mut (impl AsyncWrite + Unpin), body: &[u8]) {
        w.write_u32(body.len() as u32).await.unwrap();
        w.write_all(body).await.unwrap();
    }

    fn version_packet(with_ext: bool) -> Vec<u8> {
        let mut v = vec![FXP_VERSION];
        v.extend_from_slice(&3u32.to_be_bytes());
        if with_ext {
            put_str(&mut v, EXTENSION);
            put_str(&mut v, "1");
        }
        v
    }

    #[tokio::test]
    async fn test_tap_detects_extension_and_round_trips_the_request() {
        let (client, mut server) = duplex(4096);
        let (mut app, tap) = tap(client);
        send(&mut server, &version_packet(true)).await;
        // Das VERSION-Paket kommt unverändert bei der Anwendung an.
        let body = read_packet(&mut app).await.unwrap();
        assert_eq!(body, version_packet(true));
        assert!(tap.supports_posix_rename());

        let t = tap.clone();
        let req = tokio::spawn(async move { t.posix_rename("/a", "/b").await });
        let got = read_packet(&mut server).await.unwrap();
        assert_eq!(got[0], FXP_EXTENDED);
        let id = be_u32(&got, 1).unwrap();
        let (name, next) = read_string(&got, 5).unwrap();
        assert_eq!(name, EXTENSION);
        let (from, next) = read_string(&got, next).unwrap();
        let (to, _) = read_string(&got, next).unwrap();
        assert_eq!((from.as_str(), to.as_str()), ("/a", "/b"));

        let mut status = vec![FXP_STATUS];
        status.extend_from_slice(&id.to_be_bytes());
        status.extend_from_slice(&3u32.to_be_bytes());
        put_str(&mut status, "Permission denied");
        put_str(&mut status, "");
        send(&mut server, &status).await;
        let (code, msg) = req.await.unwrap().unwrap();
        assert_eq!((code, msg.as_str()), (3, "Permission denied"));

        // Die eigene Antwort wird nicht an die Anwendung weitergegeben,
        // fremde STATUS-Pakete schon.
        let mut other = vec![FXP_STATUS];
        other.extend_from_slice(&1u32.to_be_bytes());
        other.extend_from_slice(&0u32.to_be_bytes());
        send(&mut server, &other).await;
        assert_eq!(read_packet(&mut app).await.unwrap(), other);
    }

    #[tokio::test]
    async fn test_tap_reports_no_support_without_extension() {
        let (client, mut server) = duplex(4096);
        let (mut app, tap) = tap(client);
        send(&mut server, &version_packet(false)).await;
        read_packet(&mut app).await.unwrap();
        assert!(!tap.supports_posix_rename());
    }

    #[tokio::test]
    async fn test_tap_request_fails_when_stream_ends() {
        let (client, server) = duplex(4096);
        let (_app, tap) = tap(client);
        let t = tap.clone();
        let req = tokio::spawn(async move { t.posix_rename("/a", "/b").await });
        tokio::task::yield_now().await;
        drop(server);
        let res = tokio::time::timeout(Duration::from_secs(5), req)
            .await
            .unwrap()
            .unwrap();
        assert!(res.is_err());
    }
}
