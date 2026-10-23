//! Spec 0085, A1 (T1–T9): Ein Widerruf des erhöhten Modus beendet auch
//! Browser-Befehle, die schon laufen.
//!
//! Angriffsbild: Ein rekursives Löschen oder chmod holte den erhöhten Kanal
//! einmal zu Befehlsbeginn und lief dann durch. Schaltete der Nutzer den
//! erhöhten Modus währenddessen aus, entfernte die Sitzung oder aktivierte
//! sie für einen anderen Nutzer neu, liefen die restlichen Schreibvorgänge
//! weiter mit den alten Rechten — und das Ausschalten wartete, bis die ganze
//! Rekursion durch war.
//!
//! Spec 0086, A2 (T8/T9, am Ende der Datei): derselbe Aufbau für die Frage,
//! welchen Text die **Transferliste** bei einem Widerruf zeigt — sie zeigte
//! den Fehler mit dem Präfix `Channel-Fehler: `, während der Befehl denselben
//! Grund ohne Präfix meldete.
//!
//! Alle Tests hier fahren den **echten** Befehlsrumpf (`delete_impl`,
//! `chmod_impl`, …) durch den **echten** gemeinsamen Rahmen
//! (`with_browser_channel`, dieselbe Funktion, die `run_browser_command` im
//! Produktivpfad benutzt) — nur das Auflösen von Sitzung und Kanal aus dem
//! `AppState` fehlt, weil ein Tauri-`State` im Unit-Test nicht baubar ist.

use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex as StdMutex};

use async_trait::async_trait;
use tokio::sync::Notify;

use ssh_manager_core::ssh::mock::MockSftpSession;
use ssh_manager_core::ssh::{
    CommandOutput, InteractiveShell, PtySize, RemoteEntry, SftpSession, SshError, SshTransport,
};

use app_logic::session::{Session, SessionManager};
use app_logic::state::SessionId;

use super::*;
use crate::commands::elevation::{with_browser_channel, BrowserChannel};
use crate::elevated_sftp::{ElevatedSftp, ElevatedSftpRegistry, ElevatedSftpSlot};

// Jedes Warten hier wartet auf ein Ereignis (Haltepunkt erreicht, Widerruf
// gesetzt, Task beendet). Ein Test, der hängen bliebe, scheitert sichtbar an
// der gemeinsamen Obergrenze `waiting::HANG_GUARD` statt den Testlauf
// anzuhalten, und nennt dabei das erwartete Ereignis und die bis dahin
// gelaufenen Operationen (Issue #114: vorher feste 5 s).
use crate::test_support::waiting::expect_within;

// --- Test-Double für den erhöhten Kanal --------------------------------------

/// Ein Eintrag des In-Memory-Baums hinter [`GatedSftp`].
#[derive(Clone)]
struct Entry {
    is_dir: bool,
    permissions: u32,
    content: Vec<u8>,
}

/// Der erhöhte Kanal als Test-Double: deterministischer Verzeichnisbaum,
/// Protokoll **jeder** Operation, und ein Haltepunkt nach der k-ten
/// Operation, an dem der Test widerrufen kann.
///
/// Deterministisch statt gegen `ssh_transport::LocalFileSession`: die
/// Reihenfolge von `read_dir` auf einem echten Dateisystem ist nicht
/// festgelegt, dann wäre „nach der k-ten Operation" keine reproduzierbare
/// Stelle im Ablauf.
#[derive(Clone)]
struct GatedSftp(Arc<Gate>);

struct Gate {
    entries: StdMutex<BTreeMap<String, Entry>>,
    ops: StdMutex<Vec<String>>,
    /// Nach der so vielten Operation hält der Mock an; `0` = nie.
    pause_after: usize,
    paused: AtomicBool,
    reached: Notify,
    released: AtomicBool,
    release: Notify,
    /// T5: Nach der Freigabe hält **jede** weitere Operation für immer an.
    /// Im behobenen Stand kommt keine weitere mehr an (der Widerruf greift
    /// vorher); in der kaputten Variante wartet das Deaktivieren hier
    /// sichtbar auf den Rest des Befehls und der Test scheitert mit Frist.
    stall_after_release: bool,
    stall: Notify,
    /// Issue #622: Haltepunkt nach dem k-ten gelieferten bzw. geschriebenen
    /// Stück (`0` = nie), Zähler und größtes gesehenes Stück.
    chunk_pause_at: AtomicUsize,
    chunks: AtomicUsize,
    max_chunk: AtomicUsize,
    chunk_paused: AtomicBool,
    chunk_reached: Notify,
    chunk_released: AtomicBool,
    chunk_release: Notify,
    /// Eine „große“ Datei, die nicht im Speicher liegt: Pfad und Größe; jedes
    /// Stück wird beim Lesen erzeugt.
    synthetic: StdMutex<Option<(String, u64)>>,
    /// `remove` scheitert (Teil-Upload lässt sich nicht entfernen).
    fail_remove: AtomicBool,
}

impl GatedSftp {
    fn new(entries: &[(&str, bool)], pause_after: usize, stall_after_release: bool) -> Self {
        let entries = entries
            .iter()
            .map(|(path, is_dir)| {
                (
                    (*path).to_string(),
                    Entry {
                        is_dir: *is_dir,
                        permissions: if *is_dir { 0o755 } else { 0o644 },
                        content: b"ROOT-INHALT".to_vec(),
                    },
                )
            })
            .collect();
        Self(Arc::new(Gate {
            entries: StdMutex::new(entries),
            ops: StdMutex::new(Vec::new()),
            pause_after,
            paused: AtomicBool::new(false),
            reached: Notify::new(),
            released: AtomicBool::new(false),
            release: Notify::new(),
            stall_after_release,
            stall: Notify::new(),
            chunk_pause_at: AtomicUsize::new(0),
            chunks: AtomicUsize::new(0),
            max_chunk: AtomicUsize::new(0),
            chunk_paused: AtomicBool::new(false),
            chunk_reached: Notify::new(),
            chunk_released: AtomicBool::new(false),
            chunk_release: Notify::new(),
            synthetic: StdMutex::new(None),
            fail_remove: AtomicBool::new(false),
        }))
    }

    /// Ein Baum mit zwei Dateien, einem Unterordner und einer Datei darin.
    fn small_tree(pause_after: usize) -> Self {
        Self::new(
            &[
                ("/t", true),
                ("/t/a.txt", false),
                ("/t/b.txt", false),
                ("/t/sub", true),
                ("/t/sub/c.txt", false),
            ],
            pause_after,
            false,
        )
    }

    /// Derselbe Aufbau, aber mit vielen Dateien — damit sichtbar wird, dass
    /// nach dem Widerruf wirklich **nichts** mehr läuft und das Deaktivieren
    /// nicht auf den Rest wartet (T5).
    fn large_tree(pause_after: usize, stall_after_release: bool) -> Self {
        let mut paths: Vec<String> = vec!["/t".to_string()];
        for i in 0..30 {
            paths.push(format!("/t/f{i:02}.txt"));
        }
        let entries: Vec<(&str, bool)> = paths
            .iter()
            .map(|p| (p.as_str(), p == "/t"))
            .collect::<Vec<_>>();
        Self::new(&entries, pause_after, stall_after_release)
    }

    fn ops(&self) -> Vec<String> {
        self.0.ops.lock().unwrap().clone()
    }

    /// Haltepunkt nach dem `k`-ten Stück (Lesen wie Schreiben).
    fn pause_after_chunk(&self, k: usize) {
        self.0.chunk_pause_at.store(k, Ordering::SeqCst);
    }

    fn chunks(&self) -> usize {
        self.0.chunks.load(Ordering::SeqCst)
    }

    fn max_chunk(&self) -> usize {
        self.0.max_chunk.load(Ordering::SeqCst)
    }

    fn put_file(&self, path: &str, content: Vec<u8>) {
        self.0.entries.lock().unwrap().insert(
            path.to_string(),
            Entry {
                is_dir: false,
                permissions: 0o644,
                content,
            },
        );
    }

    fn content(&self, path: &str) -> Option<Vec<u8>> {
        self.0
            .entries
            .lock()
            .unwrap()
            .get(path)
            .map(|e| e.content.clone())
    }

    fn set_synthetic(&self, path: &str, size: u64) {
        *self.0.synthetic.lock().unwrap() = Some((path.to_string(), size));
    }

    fn fail_remove(&self) {
        self.0.fail_remove.store(true, Ordering::SeqCst);
    }

    async fn wait_until_chunk_paused(&self) {
        expect_within(
            "der Mock erreicht den Stück-Haltepunkt",
            || self.describe(),
            async {
                loop {
                    let notified = self.0.chunk_reached.notified();
                    if self.0.chunk_paused.load(Ordering::SeqCst) {
                        return;
                    }
                    notified.await;
                }
            },
        )
        .await;
    }

    fn release_chunk(&self) {
        self.0.chunk_released.store(true, Ordering::SeqCst);
        self.0.chunk_release.notify_waiters();
    }

    /// Zählt ein Stück und hält am Haltepunkt an.
    async fn after_chunk(&self, len: usize) {
        self.0.max_chunk.fetch_max(len, Ordering::SeqCst);
        let k = self.0.chunks.fetch_add(1, Ordering::SeqCst) + 1;
        if k == self.0.chunk_pause_at.load(Ordering::SeqCst)
            && !self.0.chunk_paused.swap(true, Ordering::SeqCst)
        {
            self.0.chunk_reached.notify_waiters();
            loop {
                let notified = self.0.chunk_release.notified();
                if self.0.chunk_released.load(Ordering::SeqCst) {
                    break;
                }
                notified.await;
            }
        }
    }

    fn op_count(&self) -> usize {
        self.0.ops.lock().unwrap().len()
    }

    fn exists(&self, path: &str) -> bool {
        self.0.entries.lock().unwrap().contains_key(path)
    }

    fn permissions(&self, path: &str) -> Option<u32> {
        self.0
            .entries
            .lock()
            .unwrap()
            .get(path)
            .map(|e| e.permissions)
    }

    /// Wartet, bis der Mock am Haltepunkt steht. `notified()` wird **vor**
    /// der Flag-Prüfung angelegt, sonst ginge ein Signal zwischen Prüfung
    /// und `await` verloren.
    async fn wait_until_paused(&self) {
        expect_within(
            "der Mock erreicht den Haltepunkt",
            || self.describe(),
            async {
                loop {
                    let notified = self.0.reached.notified();
                    if self.0.paused.load(Ordering::SeqCst) {
                        return;
                    }
                    notified.await;
                }
            },
        )
        .await;
    }

    /// Was der Mock bis jetzt gesehen hat — für die Meldung, wenn ein
    /// erwartetes Ereignis ausbleibt.
    fn describe(&self) -> String {
        format!(
            "am Haltepunkt: {}, freigegeben: {}, Operationen: {:?}",
            self.0.paused.load(Ordering::SeqCst),
            self.0.released.load(Ordering::SeqCst),
            self.ops()
        )
    }

    fn release(&self) {
        self.0.released.store(true, Ordering::SeqCst);
        self.0.release.notify_waiters();
    }
}

impl Gate {
    async fn wait_for_release(&self) {
        loop {
            let notified = self.release.notified();
            if self.released.load(Ordering::SeqCst) {
                return;
            }
            notified.await;
        }
    }

    /// Hält für immer an — nur erreichbar, wenn nach dem Widerruf noch eine
    /// Operation durchkommt (s. [`Gate::stall_after_release`]).
    async fn stall_forever(&self) {
        loop {
            self.stall.notified().await;
        }
    }
}

impl GatedSftp {
    /// Protokolliert die Operation und bedient Haltepunkt bzw. Dauerhalt.
    async fn record(&self, op: String) {
        let index = {
            let mut ops = self.0.ops.lock().unwrap();
            ops.push(op);
            ops.len()
        };
        if self.0.stall_after_release && self.0.released.load(Ordering::SeqCst) {
            self.0.stall_forever().await;
        }
        if index == self.0.pause_after && !self.0.paused.swap(true, Ordering::SeqCst) {
            self.0.reached.notify_waiters();
            self.0.wait_for_release().await;
        }
    }

    fn entry(&self, path: &str) -> Result<RemoteEntry, SshError> {
        let entries = self.0.entries.lock().unwrap();
        let entry = entries
            .get(path)
            .ok_or_else(|| SshError::ChannelError(format!("Datei nicht gefunden: {path}")))?;
        Ok(remote_entry(path, entry))
    }
}

fn remote_entry(path: &str, entry: &Entry) -> RemoteEntry {
    RemoteEntry {
        name: path.rsplit('/').next().unwrap_or(path).to_string(),
        path: path.to_string(),
        is_dir: entry.is_dir,
        size: entry.content.len() as u64,
        permissions: entry.permissions,
        modified: None,
        uid: None,
        gid: None,
        owner: None,
        group: None,
    }
}

#[async_trait]
impl SftpSession for GatedSftp {
    async fn list_dir(&mut self, path: &str) -> Result<Vec<RemoteEntry>, SshError> {
        self.record(format!("list_dir {path}")).await;
        let prefix = format!("{}/", path.trim_end_matches('/'));
        let entries = self.0.entries.lock().unwrap();
        Ok(entries
            .iter()
            .filter(|(p, _)| {
                p.strip_prefix(prefix.as_str())
                    .is_some_and(|rest| !rest.is_empty() && !rest.contains('/'))
            })
            .map(|(p, e)| remote_entry(p, e))
            .collect())
    }

    async fn read_file(&mut self, path: &str) -> Result<Vec<u8>, SshError> {
        self.record(format!("read_file {path}")).await;
        let entries = self.0.entries.lock().unwrap();
        entries
            .get(path)
            .map(|e| e.content.clone())
            .ok_or_else(|| SshError::ChannelError(format!("Datei nicht gefunden: {path}")))
    }

    async fn read_file_capped(
        &mut self,
        path: &str,
        max_bytes: u64,
        progress: &mut (dyn FnMut(u64) + Send),
    ) -> Result<Vec<u8>, SshError> {
        self.record(format!("read_file_capped {path}")).await;
        let entries = self.0.entries.lock().unwrap();
        let content = entries
            .get(path)
            .map(|e| e.content.clone())
            .ok_or_else(|| SshError::ChannelError(format!("Datei nicht gefunden: {path}")))?;
        let len = content.len().min(max_bytes as usize);
        progress(len as u64);
        Ok(content[..len].to_vec())
    }

    async fn write_file(&mut self, path: &str, content: &[u8]) -> Result<(), SshError> {
        self.record(format!("write_file {path}")).await;
        self.0.entries.lock().unwrap().insert(
            path.to_string(),
            Entry {
                is_dir: false,
                permissions: 0o644,
                content: content.to_vec(),
            },
        );
        Ok(())
    }

    async fn read_file_chunked(
        &mut self,
        path: &str,
        chunk_size: usize,
        on_chunk: &mut (dyn for<'a> FnMut(&'a [u8]) -> ssh_manager_core::ssh::ChunkControl + Send),
    ) -> Result<ssh_manager_core::ssh::TransferEnd, SshError> {
        self.record(format!("read_file_chunked {path}")).await;
        let synthetic = self.0.synthetic.lock().unwrap().clone();
        if let Some((p, size)) = synthetic.filter(|(p, _)| p == path) {
            let _ = p;
            let mut offset = 0u64;
            let mut index = 0u8;
            while offset < size {
                let n = (size - offset).min(chunk_size.max(1) as u64) as usize;
                // Nur dieses eine Stück liegt im Speicher.
                let chunk = vec![index; n];
                if on_chunk(&chunk) == ssh_manager_core::ssh::ChunkControl::Stop {
                    return Ok(ssh_manager_core::ssh::TransferEnd::Stopped);
                }
                offset += n as u64;
                index = index.wrapping_add(1);
                self.after_chunk(n).await;
            }
            return Ok(ssh_manager_core::ssh::TransferEnd::Completed);
        }
        let content = self
            .content(path)
            .ok_or_else(|| SshError::ChannelError(format!("Datei nicht gefunden: {path}")))?;
        for chunk in content.chunks(chunk_size.max(1)) {
            if on_chunk(chunk) == ssh_manager_core::ssh::ChunkControl::Stop {
                return Ok(ssh_manager_core::ssh::TransferEnd::Stopped);
            }
            self.after_chunk(chunk.len()).await;
        }
        Ok(ssh_manager_core::ssh::TransferEnd::Completed)
    }

    async fn write_file_chunked(
        &mut self,
        path: &str,
        chunk_size: usize,
        next_chunk: &mut (dyn for<'a> FnMut(&'a mut [u8]) -> ssh_manager_core::ssh::ChunkSource
                  + Send),
    ) -> Result<ssh_manager_core::ssh::TransferEnd, SshError> {
        use ssh_manager_core::ssh::{ChunkSource, TransferEnd};
        self.record(format!("write_file_chunked {path}")).await;
        let mut content = Vec::new();
        let mut buf = vec![0u8; chunk_size.max(1)];
        let end = loop {
            match next_chunk(&mut buf) {
                ChunkSource::Data(n) => {
                    content.extend_from_slice(&buf[..n]);
                    self.after_chunk(n).await;
                }
                ChunkSource::Eof => break TransferEnd::Completed,
                ChunkSource::Stop => break TransferEnd::Stopped,
            }
        };
        self.0.entries.lock().unwrap().insert(
            path.to_string(),
            Entry {
                is_dir: false,
                permissions: 0o644,
                content,
            },
        );
        Ok(end)
    }

    async fn stat(&mut self, path: &str) -> Result<RemoteEntry, SshError> {
        self.record(format!("stat {path}")).await;
        self.entry(path)
    }

    async fn lstat(&mut self, path: &str) -> Result<RemoteEntry, SshError> {
        self.record(format!("lstat {path}")).await;
        self.entry(path)
    }

    async fn remove(&mut self, path: &str) -> Result<(), SshError> {
        self.record(format!("remove {path}")).await;
        if self.0.fail_remove.load(Ordering::SeqCst) {
            return Err(SshError::ChannelError("remove verweigert".to_string()));
        }
        let mut entries = self.0.entries.lock().unwrap();
        match entries.get(path) {
            None => Err(SshError::ChannelError(format!(
                "Datei nicht gefunden: {path}"
            ))),
            Some(entry) if entry.is_dir => Err(SshError::ChannelError(format!(
                "{path} ist ein Verzeichnis"
            ))),
            Some(_) => {
                entries.remove(path);
                Ok(())
            }
        }
    }

    async fn rename(&mut self, from: &str, to: &str) -> Result<(), SshError> {
        self.record(format!("rename {from} -> {to}")).await;
        let mut entries = self.0.entries.lock().unwrap();
        let Some(entry) = entries.remove(from) else {
            return Err(SshError::ChannelError(format!(
                "Datei nicht gefunden: {from}"
            )));
        };
        entries.insert(to.to_string(), entry);
        Ok(())
    }

    async fn create_dir(&mut self, path: &str) -> Result<(), SshError> {
        self.record(format!("create_dir {path}")).await;
        self.0.entries.lock().unwrap().insert(
            path.to_string(),
            Entry {
                is_dir: true,
                permissions: 0o755,
                content: Vec::new(),
            },
        );
        Ok(())
    }

    async fn create_file_exclusive(&mut self, path: &str) -> Result<(), SshError> {
        self.record(format!("create_file_exclusive {path}")).await;
        let mut entries = self.0.entries.lock().unwrap();
        if entries.contains_key(path) {
            return Err(SshError::SftpAlreadyExists(path.to_string()));
        }
        entries.insert(
            path.to_string(),
            Entry {
                is_dir: false,
                permissions: 0o644,
                content: Vec::new(),
            },
        );
        Ok(())
    }

    async fn remove_dir(&mut self, path: &str) -> Result<(), SshError> {
        self.record(format!("remove_dir {path}")).await;
        let mut entries = self.0.entries.lock().unwrap();
        let prefix = format!("{}/", path.trim_end_matches('/'));
        if entries.keys().any(|p| p.starts_with(prefix.as_str())) {
            // Wie SFTP `RMDIR`: nur ein leeres Verzeichnis lässt sich
            // entfernen. So merkt T9, wenn die Reihenfolge kippt.
            return Err(SshError::ChannelError(format!(
                "Verzeichnis nicht leer: {path}"
            )));
        }
        entries.remove(path);
        Ok(())
    }

    async fn set_permissions(&mut self, path: &str, mode: u32) -> Result<(), SshError> {
        self.record(format!("set_permissions {path} {mode:o}"))
            .await;
        let mut entries = self.0.entries.lock().unwrap();
        match entries.get_mut(path) {
            None => Err(SshError::ChannelError(format!(
                "Datei nicht gefunden: {path}"
            ))),
            Some(entry) => {
                entry.permissions = mode;
                Ok(())
            }
        }
    }
}

/// Meldet sein eigenes Verworfenwerden — für T15 (Spec 0085, A2.2) und, hier,
/// als *neuer* Kanal in T4: er darf nie berührt werden.
#[derive(Clone)]
struct CountingSftp(Arc<StdMutex<Vec<String>>>);

#[async_trait]
impl SftpSession for CountingSftp {
    async fn list_dir(&mut self, path: &str) -> Result<Vec<RemoteEntry>, SshError> {
        self.0.lock().unwrap().push(format!("list_dir {path}"));
        Ok(Vec::new())
    }
    async fn read_file(&mut self, path: &str) -> Result<Vec<u8>, SshError> {
        self.0.lock().unwrap().push(format!("read_file {path}"));
        Ok(Vec::new())
    }
    async fn read_file_capped(
        &mut self,
        path: &str,
        _max_bytes: u64,
        _progress: &mut (dyn FnMut(u64) + Send),
    ) -> Result<Vec<u8>, SshError> {
        self.0
            .lock()
            .unwrap()
            .push(format!("read_file_capped {path}"));
        Ok(Vec::new())
    }
    async fn write_file(&mut self, path: &str, _content: &[u8]) -> Result<(), SshError> {
        self.0.lock().unwrap().push(format!("write_file {path}"));
        Ok(())
    }
    async fn read_file_chunked(
        &mut self,
        _path: &str,
        _chunk_size: usize,
        _on_chunk: &mut (dyn for<'a> FnMut(&'a [u8]) -> ssh_manager_core::ssh::ChunkControl + Send),
    ) -> Result<ssh_manager_core::ssh::TransferEnd, SshError> {
        self.0
            .lock()
            .unwrap()
            .push(format!("read_file_chunked {_path}"));
        Ok(ssh_manager_core::ssh::TransferEnd::Completed)
    }

    async fn write_file_chunked(
        &mut self,
        _path: &str,
        _chunk_size: usize,
        _next_chunk: &mut (dyn for<'a> FnMut(&'a mut [u8]) -> ssh_manager_core::ssh::ChunkSource
                  + Send),
    ) -> Result<ssh_manager_core::ssh::TransferEnd, SshError> {
        self.0
            .lock()
            .unwrap()
            .push(format!("write_file_chunked {_path}"));
        Ok(ssh_manager_core::ssh::TransferEnd::Completed)
    }

    async fn stat(&mut self, path: &str) -> Result<RemoteEntry, SshError> {
        self.0.lock().unwrap().push(format!("stat {path}"));
        Err(SshError::ChannelError("kein Eintrag".to_string()))
    }
    async fn lstat(&mut self, path: &str) -> Result<RemoteEntry, SshError> {
        self.0.lock().unwrap().push(format!("lstat {path}"));
        Err(SshError::ChannelError("kein Eintrag".to_string()))
    }
    async fn remove(&mut self, path: &str) -> Result<(), SshError> {
        self.0.lock().unwrap().push(format!("remove {path}"));
        Ok(())
    }
    async fn rename(&mut self, from: &str, to: &str) -> Result<(), SshError> {
        self.0.lock().unwrap().push(format!("rename {from} {to}"));
        Ok(())
    }
    async fn create_dir(&mut self, path: &str) -> Result<(), SshError> {
        self.0.lock().unwrap().push(format!("create_dir {path}"));
        Ok(())
    }
    async fn create_file_exclusive(&mut self, path: &str) -> Result<(), SshError> {
        self.0
            .lock()
            .unwrap()
            .push(format!("create_file_exclusive {path}"));
        Ok(())
    }
    async fn remove_dir(&mut self, path: &str) -> Result<(), SshError> {
        self.0.lock().unwrap().push(format!("remove_dir {path}"));
        Ok(())
    }
    async fn set_permissions(&mut self, path: &str, mode: u32) -> Result<(), SshError> {
        self.0
            .lock()
            .unwrap()
            .push(format!("set_permissions {path} {mode:o}"));
        Ok(())
    }
}

struct NoTransport;
#[async_trait]
impl SshTransport for NoTransport {
    async fn execute(&mut self, _command: &str) -> Result<CommandOutput, SshError> {
        unreachable!("diese Tests führen kein Kommando aus")
    }
    async fn open_shell(&mut self, _size: PtySize) -> Result<Box<dyn InteractiveShell>, SshError> {
        unreachable!()
    }
    async fn disconnect(&mut self) -> Result<(), SshError> {
        Ok(())
    }
}

// --- Aufbau ------------------------------------------------------------------

/// Sitzung, Zuordnung und beide Kanäle, wie die `sftp_*`-Befehle sie zur
/// Laufzeit vorfinden.
struct Setup {
    sessions: Arc<SessionManager>,
    registry: Arc<ElevatedSftpRegistry>,
    session_id: SessionId,
    session: Arc<Session>,
    /// Prüf-Handle auf den **normalen** Kanal der Sitzung (T8): er darf in
    /// keinem Widerrufs-Test eine Operation sehen.
    normal: MockSftpSession,
}

async fn setup(elevated: Box<dyn SftpSession>) -> Setup {
    let session = Arc::new(app_logic::test_support::session_with_transport(Box::new(
        NoTransport,
    )));
    let normal = MockSftpSession::new().with_file("/t/a.txt", b"USER-INHALT".to_vec());
    session.set_sftp_for_tests(Box::new(normal.clone())).await;

    let sessions = Arc::new(SessionManager::new());
    let session_id = SessionId::new_v4();
    sessions.insert(session_id, session.clone());

    let registry = Arc::new(ElevatedSftpRegistry::default());
    registry.insert_for_tests(
        session_id,
        ElevatedSftp {
            target_user: "root".to_string(),
            sftp: elevated,
        },
    );

    Setup {
        sessions,
        registry,
        session_id,
        session,
        normal,
    }
}

impl Setup {
    fn elevated_channel(&self) -> BrowserChannel {
        BrowserChannel::for_tests(&self.registry, self.session_id, Some("root".to_string()))
    }

    fn normal_channel(&self) -> BrowserChannel {
        BrowserChannel::for_tests(&self.registry, self.session_id, None)
    }

    fn slot(&self) -> ElevatedSftpSlot {
        self.registry
            .slot(self.session_id, &crate::test_support::elevation::access())
            .expect("Vorbedingung: erhöhter Kanal aktiv")
    }

    /// Wartet, bis der Widerruf wirkt — er ist der sofort wirksame Teil und
    /// geht dem Herausnehmen des Kanalwerts voraus (Spec 0084, §9).
    ///
    /// Der Widerruf meldet sich nicht von selbst; die Abfrage gibt nach jedem
    /// Blick ab, damit der widerrufende Task auf derselben
    /// Ein-Thread-Laufzeit weiterkommt.
    async fn wait_until_revoked(&self, slot: &ElevatedSftpSlot) {
        expect_within(
            "der Widerruf wirkt sofort, nicht erst nach dem Befehl",
            || "Kanal noch nicht widerrufen".to_string(),
            async {
                while !slot.is_revoked() {
                    tokio::task::yield_now().await;
                }
            },
        )
        .await;
    }
}

/// Fährt einen Befehlsrumpf durch **denselben** gemeinsamen Rahmen wie der
/// Produktivpfad (`run_browser_command` ruft dieselbe Funktion) und gibt den
/// laufenden Task zurück, damit der Test währenddessen widerrufen kann.
fn spawn_command<T, F, Fut>(
    session: Arc<Session>,
    channel: BrowserChannel,
    body: F,
) -> tokio::task::JoinHandle<CommandResult<T>>
where
    T: Send + 'static,
    F: FnOnce(Arc<Session>, BrowserChannel) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = CommandResult<T>> + Send,
{
    tokio::spawn(async move { with_browser_channel(session, channel, body).await })
}

async fn join<T>(
    handle: tokio::task::JoinHandle<CommandResult<T>>,
    elevated: &GatedSftp,
) -> CommandResult<T> {
    expect_within(
        "der Befehl endet, statt auf den widerrufenen Kanal zu warten",
        || elevated.describe(),
        handle,
    )
    .await
    .expect("der Befehls-Task darf nicht panisch enden")
}

/// Der Wortlaut von `ELEVATED_CHANNEL_INACTIVE` (dort `pub(super)`-los, also
/// hier nachgeschrieben statt importiert — genau deshalb prüft
/// [`assert_inactive`] auf Gleichheit: eine Abweichung fällt auf).
const ELEVATED_CHANNEL_INACTIVE_TEXT: &str =
    "Der erhöhte Modus ist nicht mehr aktiv (Verbindung getrennt oder ausgeschaltet) — \
     die Aktion wurde nicht ausgeführt. Bitte den erhöhten Modus erneut einschalten.";

/// Spec 0085, A1.2: **gleich**, nicht „enthält". Ein Abbruch wegen Widerrufs
/// darf nicht als SFTP-Fehler oder Channel-Fehler beim Aufrufer ankommen.
fn assert_inactive<T: std::fmt::Debug>(result: CommandResult<T>) {
    let err = result.expect_err("nach dem Widerruf darf der Befehl nicht gelingen");
    assert_eq!(
        err.message, ELEVATED_CHANNEL_INACTIVE_TEXT,
        "der Abbruchgrund muss wörtlich ELEVATED_CHANNEL_INACTIVE sein"
    );
}

// --- Audit-Mitschnitt (T7) ---------------------------------------------------

/// Eingefangene `tracing`-Ereignisse, Feldname → Wert.
#[derive(Clone, Default)]
struct CapturedLogs(Arc<StdMutex<Vec<HashMap<String, String>>>>);

impl CapturedLogs {
    /// Die Audit-Zeilen zu `action` auf `path` (s.
    /// `elevation::audit_elevated_change`).
    fn audit_lines(&self, action: &str, path: &str) -> Vec<HashMap<String, String>> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .filter(|fields| {
                fields.get("action").map(String::as_str) == Some(action)
                    && fields.get("path").map(String::as_str) == Some(path)
                    && fields.get("elevated").map(String::as_str) == Some("true")
            })
            .cloned()
            .collect()
    }
}

/// Minimaler `tracing::Subscriber`, der nur Ereignisfelder sammelt.
///
/// Von Hand statt über `tracing-subscriber`: diese Kiste hat die Abhängigkeit
/// nicht, und für „welche Felder hatte das Ereignis" braucht es nichts
/// weiter.
///
/// Issue #114: **prozessweit** gesetzt (einmal je Testlauf), nicht als
/// Thread-Standard. `tracing` merkt sich je Aufrufstelle prozessweit, ob sie
/// jemand hören will, und fragt dafür den Thread, der sie zuerst erreicht.
/// Mit einem Thread-Standard konnte das ein paralleler Test ohne Mitschnitt
/// sein, dann galt die Audit-Zeile für immer als „nie" und T7 sah sie nicht
/// (s. `test_capture_sees_a_line_whose_callsite_another_thread_reached_first`).
/// Der prozessweite Subscriber hört jede Aufrufstelle, egal von welchem
/// Thread, und legt eine Zeile nur ab, wenn auf **ihrem** Thread gerade ein
/// Mitschnitt läuft. `#[tokio::test]` fährt eine Ein-Thread-Laufzeit, die
/// auch die per `tokio::spawn` gestarteten Tasks auf genau diesem Thread
/// abarbeitet, der Mitschnitt erfasst sie also mit.
struct RoutingSubscriber;

thread_local! {
    /// Mitschnitt des Tests, der auf diesem Thread läuft, falls einer läuft.
    static THREAD_CAPTURE: std::cell::RefCell<Option<CapturedLogs>> =
        const { std::cell::RefCell::new(None) };
}

impl tracing::Subscriber for RoutingSubscriber {
    fn enabled(&self, _metadata: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, _span: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _span: &tracing::span::Id, _values: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _span: &tracing::span::Id, _follows: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        THREAD_CAPTURE.with(|capture| {
            if let Some(logs) = capture.borrow().as_ref() {
                let mut fields = HashMap::new();
                event.record(&mut FieldCollector(&mut fields));
                logs.0.lock().unwrap().push(fields);
            }
        });
    }
    fn enter(&self, _span: &tracing::span::Id) {}
    fn exit(&self, _span: &tracing::span::Id) {}
}

struct FieldCollector<'a>(&'a mut HashMap<String, String>);

impl tracing::field::Visit for FieldCollector<'_> {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        self.0.insert(field.name().to_string(), value.to_string());
    }
    fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
        self.0.insert(field.name().to_string(), value.to_string());
    }
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.0
            .insert(field.name().to_string(), format!("{value:?}"));
    }
}

/// Beendet den Mitschnitt dieses Threads beim Verlassen des Tests (auch bei
/// einer Panik) und stellt den vorherigen Zustand wieder her.
struct CaptureGuard(Option<CapturedLogs>);

impl Drop for CaptureGuard {
    fn drop(&mut self) {
        let previous = self.0.take();
        THREAD_CAPTURE.with(|capture| *capture.borrow_mut() = previous);
    }
}

fn capture_logs() -> (CapturedLogs, CaptureGuard) {
    static INSTALL: std::sync::Once = std::sync::Once::new();
    INSTALL.call_once(|| {
        tracing::subscriber::set_global_default(RoutingSubscriber)
            .expect("in diesem Testlauf darf niemand sonst einen prozessweiten Subscriber setzen");
    });
    let logs = CapturedLogs::default();
    let previous = THREAD_CAPTURE.with(|capture| capture.borrow_mut().replace(logs.clone()));
    (logs, CaptureGuard(previous))
}

/// Eine eigene Aufrufstelle nur für den Test unten: Welcher Thread sie zuerst
/// erreicht, bestimmt der Test selbst, nicht die Reihenfolge anderer Tests.
fn emit_capture_probe_line() {
    tracing::info!(probe = "capture", "capture probe");
}

/// Issue #114 (Ursache des Wacklers in T7): `tracing` merkt sich je
/// Aufrufstelle prozessweit, ob sie überhaupt jemand hören will. Erreicht ein
/// anderer Thread ohne Mitschnitt die Aufrufstelle zuerst, während der
/// Mitschnitt dieses Tests der einzige eingetragene ist, fragt `tracing` nur
/// den Standard **jenes** Threads und merkt sich „nie". Danach geht die Zeile
/// auch auf dem Thread mit Mitschnitt verloren.
///
/// Genau so lief es unter voller Last mit der Audit-Zeile: Ein paralleler
/// Test ohne Mitschnitt erreichte `audit_elevated_change` zuerst, und T7 sah
/// keine einzige Zeile. Der Mitschnitt muss deshalb unabhängig davon sein,
/// welcher Thread eine Aufrufstelle zuerst erreicht.
#[test]
fn test_capture_sees_a_line_whose_callsite_another_thread_reached_first() {
    let (logs, _capture) = capture_logs();
    std::thread::spawn(emit_capture_probe_line)
        .join()
        .expect("der fremde Thread darf nicht panisch enden");
    emit_capture_probe_line();

    let probe_lines = logs
        .0
        .lock()
        .unwrap()
        .iter()
        .filter(|fields| fields.get("probe").map(String::as_str) == Some("capture"))
        .count();
    assert_eq!(
        probe_lines, 1,
        "genau die Zeile dieses Threads muss im Mitschnitt stehen, nicht die des fremden"
    );
}

// --- T1–T5: Widerruf mitten in einer Rekursion ------------------------------

/// Spec 0085, T1: Rekursives Löschen über den erhöhten Kanal, der Mock hält
/// nach der 5. Operation an (die erste Datei ist da schon gelöscht). Genau
/// dort schaltet der Nutzer die erhöhten Rechte aus.
///
/// Scheitert gegen den Stand vor diesem Schritt (Widerruf nur bei
/// Befehlsbeginn geprüft): dann laufen die restlichen `remove`/`remove_dir`
/// über den widerrufenen Kanal weiter und der Befehl endet mit `Ok`.
#[tokio::test]
async fn test_t1_disabling_during_a_recursive_delete_stops_it_immediately() {
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
    assert!(
        elevated.ops().iter().any(|op| op.starts_with("remove /t/")),
        "Vorbedingung: mindestens ein Eintrag war schon gelöscht, war: {:?}",
        elevated.ops()
    );

    // Ausschalten: der Merker wirkt sofort, das Herausnehmen des Kanalwerts
    // wartet auf die gerade laufende Operation — deshalb in einem eigenen
    // Task.
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
    expect_within("das Ausschalten endet", || elevated.describe(), disable)
        .await
        .expect("der Ausschalt-Task darf nicht panisch enden")
        .expect("Ausschalten gelingt");

    assert_eq!(
        elevated.op_count(),
        ops_before,
        "nach dem Widerruf darf keine Operation mehr über den erhöhten Kanal laufen, war: {:?}",
        elevated.ops()
    );
    assert!(
        elevated.exists("/t") && elevated.exists("/t/sub/c.txt"),
        "der Rest des Baums bleibt stehen — der Befehl wurde abgebrochen"
    );
    // A1.6: kein Rückfall auf den normalen Kanal (T8).
    assert!(
        s.normal.calls().is_empty(),
        "der normale Kanal darf keine Operation dieses Befehls sehen, war: {:?}",
        s.normal.calls()
    );
}

/// Spec 0085, T2: dasselbe für rekursives chmod. Der Mock hält nach der 5.
/// Operation an — da hat der erste Eintrag die neuen Rechte schon.
#[tokio::test]
async fn test_t2_disabling_during_a_recursive_chmod_stops_it_immediately() {
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
    assert_eq!(
        elevated.permissions("/t/sub/c.txt"),
        Some(0o700),
        "Vorbedingung: mindestens ein Eintrag hatte die neuen Rechte schon"
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
    expect_within("das Ausschalten endet", || elevated.describe(), disable)
        .await
        .expect("der Ausschalt-Task darf nicht panisch enden")
        .expect("Ausschalten gelingt");

    assert_eq!(
        elevated.op_count(),
        ops_before,
        "nach dem Widerruf darf kein weiteres set_permissions laufen, war: {:?}",
        elevated.ops()
    );
    assert_eq!(
        elevated.permissions("/t"),
        Some(0o755),
        "die Wurzel behält ihre alten Rechte — sie kam nach dem Widerruf dran"
    );
    assert!(s.normal.calls().is_empty());
}

/// Spec 0085, T3: wie T1, aber die Sitzung wird entfernt (Spec 0084, A2.1)
/// statt ausgeschaltet. Das Entfernen kehrt **sofort** zurück, ohne auf den
/// laufenden Befehl zu warten — es steht im `disconnect`-Pfad.
#[tokio::test]
async fn test_t3_removing_the_session_during_a_recursive_delete_stops_it_immediately() {
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

    // Direkt, nicht in einem Task: das Entfernen darf gar nicht warten.
    let removed = s.registry.remove_session(&s.sessions, s.session_id);
    assert!(removed.is_some(), "die Sitzung war eingetragen");
    assert!(
        slot.is_revoked(),
        "A2.1: das Entfernen widerruft sofort, während der Befehl die Sperre noch hält"
    );

    elevated.release();
    assert_inactive(join(command, &elevated).await);
    assert_eq!(
        elevated.op_count(),
        ops_before,
        "nach dem Trennen darf keine Operation mehr laufen, war: {:?}",
        elevated.ops()
    );
    assert!(s.normal.calls().is_empty());
}

/// Spec 0085, T4: wie T1, aber der Nutzer aktiviert den erhöhten Modus
/// mitten im Befehl für einen **anderen** Nutzer neu. Danach läuft keine
/// Operation dieses Befehls mehr — weder über den alten noch über den neuen
/// Kanal.
#[tokio::test]
async fn test_t4_reactivating_for_another_user_during_a_recursive_delete_stops_it() {
    let elevated = GatedSftp::small_tree(5);
    let new_channel_calls = Arc::new(StdMutex::new(Vec::new()));

    // Aufbau über den echten Aktivierungsweg, damit das Neu-Aktivieren
    // wirklich `insert_if_session_alive`/`revoke` durchläuft: der Transport
    // gibt erst den überwachten, dann den neuen Kanal heraus.
    let f = crate::test_support::elevation::fixture_with_channels(vec![
        Box::new(elevated.clone()),
        Box::new(CountingSftp(new_channel_calls.clone())),
    ]);
    let normal = MockSftpSession::new();
    f.session.set_sftp_for_tests(Box::new(normal.clone())).await;
    crate::elevated_sftp::enable(
        &f.ctx(),
        "deploy",
        None,
        None,
        false,
        &crate::test_support::elevation::access(),
    )
    .await
    .expect("Vorbedingung: Aktivieren als root gelingt");
    let slot = f
        .registry
        .slot(f.session_id, &crate::test_support::elevation::access())
        .expect("Vorbedingung: erhöhter Kanal aktiv");

    let channel = BrowserChannel::for_tests(&f.registry, f.session_id, Some("root".to_string()));
    let command = spawn_command(f.session.clone(), channel, |session, channel| async move {
        delete_impl(&session, &channel, "/t").await
    });
    elevated.wait_until_paused().await;
    let ops_before = elevated.op_count();

    let f = Arc::new(f);
    let f_for_switch = f.clone();
    let switch = tokio::spawn(async move {
        crate::elevated_sftp::enable_after_confirmation(
            &f_for_switch.ctx(),
            "deploy",
            None,
            Some("www-data"),
            &crate::test_support::elevation::access(),
        )
        .await
    });
    expect_within(
        "das Neu-Aktivieren widerruft den alten Kanal sofort",
        || {
            format!(
                "Kanal nicht widerrufen, Umschalten beendet: {}",
                switch.is_finished()
            )
        },
        async {
            while !slot.is_revoked() {
                tokio::task::yield_now().await;
            }
        },
    )
    .await;
    elevated.release();

    assert_inactive(join(command, &elevated).await);
    expect_within("das Umschalten endet", || elevated.describe(), switch)
        .await
        .expect("der Umschalt-Task darf nicht panisch enden")
        .expect("das Umschalten selbst gelingt");

    assert_eq!(
        elevated.op_count(),
        ops_before,
        "keine Operation mehr über den ALTEN Kanal, war: {:?}",
        elevated.ops()
    );
    assert!(
        new_channel_calls.lock().unwrap().is_empty(),
        "und keine über den NEUEN Kanal, war: {:?}",
        new_channel_calls.lock().unwrap()
    );
    assert!(normal.calls().is_empty(), "und keine über den normalen");
}

/// Spec 0085, T5: Das Ausschalten wartet höchstens auf die gerade laufende
/// einzelne SFTP-Operation, nicht auf den Rest des Befehls (A1.3).
///
/// Der Mock hält nach der 5. Operation an und lässt danach **jede** weitere
/// Operation für immer hängen. Im behobenen Stand kommt keine weitere mehr
/// an: der Befehl bricht ab, die Sperre wird frei, das Ausschalten kehrt
/// zurück. Gegen den Stand von vorher (Merker nur bei Befehlsbeginn, Sperre
/// bis zum Ende gehalten) läuft der Befehl in den Dauerhalt und das
/// Ausschalten scheitert mit Frist.
#[tokio::test]
async fn test_t5_disabling_waits_only_for_the_running_operation_not_for_the_rest() {
    let elevated = GatedSftp::large_tree(5, true);
    let s = setup(Box::new(elevated.clone())).await;
    let slot = s.slot();

    let command = spawn_command(
        s.session.clone(),
        s.elevated_channel(),
        |session, channel| async move { delete_impl(&session, &channel, "/t").await },
    );
    elevated.wait_until_paused().await;
    let ops_before = elevated.op_count();

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

    // Gegenbeweis bleibt gültig: Wartet das Ausschalten auf den Rest des
    // Befehls, hängt es im Dauerhalt des Mocks und scheitert an der
    // Obergrenze.
    expect_within(
        "das Ausschalten endet, ohne auf den Rest des Befehls zu warten",
        || elevated.describe(),
        disable,
    )
    .await
    .expect("der Ausschalt-Task darf nicht panisch enden")
    .expect("Ausschalten gelingt");

    assert_inactive(join(command, &elevated).await);
    assert_eq!(
        elevated.op_count(),
        ops_before,
        "Zähler der nach dem Widerruf ausgeführten Operationen muss 0 sein, war: {:?}",
        elevated.ops()
    );
    assert!(
        elevated.exists("/t/f29.txt"),
        "der große Rest des Baums ist unberührt"
    );
}

// --- T6/T6b/T6c: Befehle, die den Fehler selbst abfangen oder nur lesen -----

/// Spec 0085, T6: Die Lösch-Vorschau bricht ebenso ab — kein weiterer
/// Lesezugriff nach dem Widerruf, und ein Fehler statt eines Zählergebnisses.
#[tokio::test]
async fn test_t6_delete_preview_stops_at_a_revocation_instead_of_returning_counts() {
    let elevated = GatedSftp::small_tree(2);
    let s = setup(Box::new(elevated.clone())).await;
    let slot = s.slot();

    let command = spawn_command(
        s.session.clone(),
        s.elevated_channel(),
        |session, channel| async move { delete_preview_impl(&session, &channel, "/t").await },
    );
    elevated.wait_until_paused().await;
    let ops_before = elevated.op_count();

    s.registry.remove_session(&s.sessions, s.session_id);
    s.wait_until_revoked(&slot).await;
    elevated.release();

    assert_inactive(join(command, &elevated).await);
    assert_eq!(
        elevated.op_count(),
        ops_before,
        "kein weiterer Lesezugriff nach dem Widerruf, war: {:?}",
        elevated.ops()
    );
}

/// Spec 0085, T6b: `sftp_read_text` und `sftp_open_for_editing` führen zwei
/// Operationen aus (`stat`, dann `read_file`). Ein Widerruf dazwischen lässt
/// die Datei ungelesen.
///
/// Scheitert gegen eine Lösung, die nur die Rekursionen absichert: dort
/// prüfte der zweite Zugriff nichts und der Dateiinhalt ginge über den
/// widerrufenen Kanal noch heraus.
#[tokio::test]
async fn test_t6b_a_revocation_between_stat_and_read_leaves_the_file_unread() {
    for command_name in ["read_text", "open_for_editing"] {
        let elevated = GatedSftp::small_tree(1);
        let s = setup(Box::new(elevated.clone())).await;
        let slot = s.slot();
        let session_id = s.session_id;
        // Issue #134: never the real user cache, even if a regression lets
        // the read (and so the write of the edit copy) through.
        let edit_root = tempfile::tempdir().unwrap();
        let edit_root_path = edit_root.path().to_path_buf();

        let command = spawn_command(
            s.session.clone(),
            s.elevated_channel(),
            move |session, channel| async move {
                if command_name == "read_text" {
                    read_text_impl(&session, &channel, "/t/a.txt")
                        .await
                        .map(|_| ())
                } else {
                    // Spec 0086, A1.2: die Grenze ist hier ohne Belang (der
                    // Baum hält winzige Dateien) — die echte Konstante, wie
                    // im Produktivpfad.
                    open_for_editing_impl(
                        &session,
                        &channel,
                        Some(&edit_root_path),
                        session_id,
                        "/t/a.txt",
                        MAX_EDIT_OPEN_BYTES,
                    )
                    .await
                    .map(|_| ())
                }
            },
        );
        elevated.wait_until_paused().await;
        assert_eq!(
            elevated.ops(),
            vec!["stat /t/a.txt".to_string()],
            "{command_name}: Vorbedingung — der Mock steht direkt nach dem stat"
        );

        s.registry.remove_session(&s.sessions, session_id);
        s.wait_until_revoked(&slot).await;
        elevated.release();

        assert_inactive(join(command, &elevated).await);
        assert!(
            !elevated.ops().iter().any(|op| op.starts_with("read_file")),
            "{command_name}: die Datei darf nach dem Widerruf nicht gelesen werden, war: {:?}",
            elevated.ops()
        );
        assert!(s.normal.calls().is_empty(), "{command_name}: kein Rückfall");
    }
}

/// Spec 0085, T6c: `sftp_exists` beantwortet „gibt es den Pfad?" mit
/// `stat(..).is_ok()` und verschluckt damit jeden Fehler — auch einen
/// Widerruf. Der Abbruch-Vermerk des Befehls und die zentrale Übersetzung
/// machen daraus trotzdem `ELEVATED_CHANNEL_INACTIVE` statt `Ok(false)`.
///
/// Der Widerruf sitzt hier **vor** dem `stat`, zwischen Zugang und Operation
/// — dafür der Haltepunkt am Slot (`set_before_operation_hook`): `sftp_exists`
/// hat nur diese eine Operation, es gibt also keinen zweiten Zugriff, an dem
/// die Verschränkung von selbst entstünde.
///
/// Scheitert gegen eine Lösung mit Prüfung je Operation, aber **ohne**
/// zentrale Übersetzung: dann kommt beim Aufrufer `Ok(false)` an — die
/// Aussage „der Pfad existiert nicht", obwohl niemand nachgesehen hat.
#[tokio::test]
async fn test_t6c_exists_reports_the_revocation_instead_of_a_plain_false() {
    let elevated = GatedSftp::small_tree(0);
    let s = setup(Box::new(elevated.clone())).await;
    let slot = s.slot();

    let sessions = s.sessions.clone();
    let registry = s.registry.clone();
    let session_id = s.session_id;
    slot.set_before_operation_hook(Box::new(move || {
        registry.remove_session(&sessions, session_id);
    }));

    let result = with_browser_channel(
        s.session.clone(),
        s.elevated_channel(),
        |session, channel| async move { exists_impl(&session, &channel, "/t/a.txt").await },
    )
    .await;

    assert!(
        slot.is_revoked(),
        "Vorbedingung: der Haltepunkt hat vor der Operation widerrufen"
    );
    assert_inactive(result);
    assert!(
        elevated.ops().is_empty(),
        "das stat darf nach dem Widerruf nicht mehr laufen, war: {:?}",
        elevated.ops()
    );
}

/// spec-reviewer-Fund (Runde 1/2): Erreicht **die maßgebliche** Prüfung — die
/// unter der Kanal-Sperre in `elevated_access`.
///
/// Vor ihr sitzt ein Schnellabbruch, der einen schon widerrufenen Zugriff gar
/// nicht erst warten lässt. Der kann nur ablehnen, nie zulassen, und ersetzt
/// die Prüfung unter der Sperre deshalb nicht — er verdeckt sie aber in jedem
/// anderen Test: dort ist der Widerruf beim Start der nächsten Operation
/// schon gesetzt, und der Schnellabbruch greift zuerst.
///
/// Dieser Test führt den Widerruf deshalb über den Haltepunkt am Slot, der
/// **hinter** dem Schnellabbruch sitzt: der Zugriff ist an der Abkürzung
/// vorbei, wenn widerrufen wird. Über `remove_session`, weil das nur den
/// Merker setzt und den Kanalwert stehen lässt — sonst scheiterte der Zugriff
/// schon am fehlenden Wert und nicht an der Prüfung.
///
/// Gegenbeweis (belegt): Ohne diese Prüfung läuft die Operation über den
/// widerrufenen Kanal, und der ganze Baum wird gelöscht.
#[tokio::test]
async fn test_a_revocation_that_slipped_past_the_shortcut_still_stops_the_operation() {
    let elevated = GatedSftp::small_tree(0);
    let s = setup(Box::new(elevated.clone())).await;
    let slot = s.slot();

    let sessions = s.sessions.clone();
    let registry = s.registry.clone();
    let session_id = s.session_id;
    slot.set_before_operation_hook(Box::new(move || {
        registry.remove_session(&sessions, session_id);
    }));

    let result = with_browser_channel(
        s.session.clone(),
        s.elevated_channel(),
        |session, channel| async move { delete_impl(&session, &channel, "/t").await },
    )
    .await;

    assert!(
        slot.is_revoked(),
        "Vorbedingung: der Haltepunkt hat hinter dem Schnellabbruch widerrufen"
    );
    assert_inactive(result);
    assert!(
        elevated.ops().is_empty(),
        "keine einzige Operation darf über den widerrufenen Kanal laufen, war: {:?}",
        elevated.ops()
    );
    assert!(
        elevated.exists("/t") && elevated.exists("/t/sub/c.txt"),
        "und der Baum ist unberührt"
    );
    assert!(s.normal.calls().is_empty(), "kein Rückfall");
}

// --- T7: Audit --------------------------------------------------------------

/// Spec 0085, T7 (A1.4): Ein abgebrochener erhöhter `delete`/`chmod`, bei dem
/// mindestens eine Operation lief, hinterlässt **genau eine** Audit-Zeile
/// `ok = false` mit dem Ziel-Nutzer, unter dem die schon ausgeführten
/// Operationen liefen.
///
/// Scheitert gegen die Variante, die den Ziel-Nutzer erst nach dem Widerruf
/// abfragt: dann meldet der Kanal keinen Nutzer mehr und die Zeile fehlt
/// ganz — die erhöhte Änderung wäre nicht protokolliert.
#[tokio::test]
async fn test_t7_an_aborted_elevated_change_is_audited_once_with_its_target_user() {
    for action in ["delete", "chmod"] {
        let elevated = GatedSftp::small_tree(5);
        let s = setup(Box::new(elevated.clone())).await;
        let slot = s.slot();
        let (logs, _capture) = capture_logs();

        let command = spawn_command(
            s.session.clone(),
            s.elevated_channel(),
            move |session, channel| async move {
                if action == "delete" {
                    delete_impl(&session, &channel, "/t").await
                } else {
                    chmod_impl(&session, &channel, "/t", 0o700, true, false)
                        .await
                        .map(|_| ())
                }
            },
        );
        elevated.wait_until_paused().await;
        s.registry.remove_session(&s.sessions, s.session_id);
        s.wait_until_revoked(&slot).await;
        elevated.release();
        assert_inactive(join(command, &elevated).await);

        let lines = logs.audit_lines(action, "/t");
        assert_eq!(
            lines.len(),
            1,
            "{action}: genau eine Audit-Zeile erwartet, war: {lines:?}"
        );
        assert_eq!(
            lines[0].get("ok").map(String::as_str),
            Some("false"),
            "{action}: die Zeile darf keinen Erfolg behaupten"
        );
        assert_eq!(
            lines[0].get("target_user").map(String::as_str),
            Some("root"),
            "{action}: mit dem Ziel-Nutzer, unter dem die Operationen liefen"
        );
    }
}

/// Spec 0085, T7, zweiter Teil: Scheitert schon der **Zugriff** auf den
/// Kanal, gibt es wie bisher keine Zeile — es lief keine Operation, es ist
/// nichts zu protokollieren.
#[tokio::test]
async fn test_t7_a_revocation_before_the_first_access_writes_no_audit_line() {
    let elevated = GatedSftp::small_tree(0);
    let s = setup(Box::new(elevated.clone())).await;
    let (logs, _capture) = capture_logs();

    s.registry.remove_session(&s.sessions, s.session_id);

    let result = with_browser_channel(
        s.session.clone(),
        s.elevated_channel(),
        |session, channel| async move { delete_impl(&session, &channel, "/t").await },
    )
    .await;

    assert_inactive(result);
    assert!(
        logs.audit_lines("delete", "/t").is_empty(),
        "ohne ausgeführte Operation keine Audit-Zeile, war: {:?}",
        logs.audit_lines("delete", "/t")
    );
    assert!(elevated.ops().is_empty());
}

// --- T9: Regression ---------------------------------------------------------

/// Spec 0085, T9: Ohne Widerruf verhalten sich rekursives Löschen und chmod
/// wie bisher — über den nicht widerrufenen erhöhten Kanal bis `Ok`.
#[tokio::test]
async fn test_t9_without_a_revocation_recursive_delete_and_chmod_still_succeed() {
    let elevated = GatedSftp::small_tree(0);
    let s = setup(Box::new(elevated.clone())).await;

    let changed =
        with_browser_channel(
            s.session.clone(),
            s.elevated_channel(),
            |session, channel| async move {
                chmod_impl(&session, &channel, "/t", 0o700, true, false).await
            },
        )
        .await
        .expect("ohne Widerruf muss das rekursive chmod gelingen");
    assert_eq!(changed, 5, "Wurzel, zwei Dateien, Unterordner, Datei darin");
    assert_eq!(elevated.permissions("/t"), Some(0o700));
    assert_eq!(elevated.permissions("/t/sub/c.txt"), Some(0o700));

    with_browser_channel(
        s.session.clone(),
        s.elevated_channel(),
        |session, channel| async move { delete_impl(&session, &channel, "/t").await },
    )
    .await
    .expect("ohne Widerruf muss das rekursive Löschen gelingen");
    assert!(
        !elevated.exists("/t") && !elevated.exists("/t/sub/c.txt"),
        "der ganze Baum ist weg, war: {:?}",
        elevated.ops()
    );
    assert!(s.normal.calls().is_empty());
}

/// Spec 0085, T9/A1.5: Über den **normalen** Kanal ändert sich nichts — er
/// wird wie bisher für die Dauer des Befehls gesperrt, kennt keinen Widerruf,
/// und der erhöhte Kanal bleibt dabei unberührt.
#[tokio::test]
async fn test_t9_the_normal_channel_is_unaffected_by_the_revocation_machinery() {
    let elevated = GatedSftp::small_tree(0);
    let s = setup(Box::new(elevated.clone())).await;

    // Der erhöhte Kanal ist aktiv — und wird trotzdem nicht berührt.
    let text = with_browser_channel(
        s.session.clone(),
        s.normal_channel(),
        |session, channel| async move { read_text_impl(&session, &channel, "/t/a.txt").await },
    )
    .await
    .expect("über den normalen Kanal muss das Lesen gelingen");

    assert_eq!(text, "USER-INHALT");
    assert!(
        elevated.ops().is_empty(),
        "der erhöhte Kanal darf dabei nie berührt werden, war: {:?}",
        elevated.ops()
    );

    // Und ein Widerruf des erhöhten Kanals lässt den normalen Befehl
    // unbeeindruckt.
    s.registry.remove_session(&s.sessions, s.session_id);
    let text = with_browser_channel(
        s.session.clone(),
        s.normal_channel(),
        |session, channel| async move { read_text_impl(&session, &channel, "/t/a.txt").await },
    )
    .await
    .expect("der Widerruf des erhöhten Kanals betrifft den normalen nicht");
    assert_eq!(text, "USER-INHALT");
}

/// Spec 0085, T9 (Ergänzung nach dem spec-reviewer-Fund aus Runde 1):
/// **rekursives** Löschen und chmod über den normalen Kanal, und zwar durch
/// denselben neuen Wrapper-Pfad (`BrowserSftp::Normal`), den die Befehle zur
/// Laufzeit nehmen.
///
/// Die bestehenden `sftp_mutation_tests` rufen `delete_recursive`/
/// `chmod_recursive` direkt mit einem `&mut dyn SftpSession` auf und gehen am
/// Wrapper vorbei — für den normalen Kanal war er damit nirgends von Befehl
/// bis Kanal abgedeckt.
#[tokio::test]
async fn test_t9_recursive_delete_and_chmod_over_the_normal_channel_behave_as_before() {
    let normal_tree = GatedSftp::small_tree(0);
    let elevated = GatedSftp::small_tree(0);
    let s = setup(Box::new(elevated.clone())).await;
    // Der normale Kanal bekommt denselben Baum wie der erhöhte — so ist am
    // Protokoll ablesbar, dass wirklich der normale benutzt wurde.
    s.session
        .set_sftp_for_tests(Box::new(normal_tree.clone()))
        .await;

    let changed =
        with_browser_channel(
            s.session.clone(),
            s.normal_channel(),
            |session, channel| async move {
                chmod_impl(&session, &channel, "/t", 0o700, true, false).await
            },
        )
        .await
        .expect("rekursives chmod über den normalen Kanal muss gelingen");
    assert_eq!(changed, 5);
    assert_eq!(normal_tree.permissions("/t"), Some(0o700));
    assert_eq!(normal_tree.permissions("/t/sub/c.txt"), Some(0o700));

    with_browser_channel(
        s.session.clone(),
        s.normal_channel(),
        |session, channel| async move { delete_impl(&session, &channel, "/t").await },
    )
    .await
    .expect("rekursives Löschen über den normalen Kanal muss gelingen");
    assert!(
        !normal_tree.exists("/t") && !normal_tree.exists("/t/sub/c.txt"),
        "der ganze Baum ist weg, war: {:?}",
        normal_tree.ops()
    );

    assert!(
        elevated.ops().is_empty(),
        "der erhöhte Kanal darf dabei nie berührt werden, war: {:?}",
        elevated.ops()
    );
}

// --- Spec 0086, A2 (T8/T9): die Meldung in der Transferliste ----------------
//
// Ein Widerruf, der erst bei der einzelnen Operation greift, kam im Rumpf von
// `download_one_file`/`upload_impl` als `SshError::ChannelError(<Text>)` an,
// dessen `Display` das Präfix `Channel-Fehler: ` vorsetzt. Das
// `sftp-transfer-finished`-Ereignis trug diesen Text, das Befehlsergebnis
// danach den ohne Präfix (zentral in `with_browser_channel` übersetzt) — die
// Transferliste zeigte also etwas anderes als der Befehl.
//
// Beide Transfer-Kerne bekommen den Emitter seit Spec 0086 als Parameter
// (T8); vorher bauten sie ihn aus einem `AppHandle` selbst und kein Test
// erreichte sie.

/// Die `error`-Felder aller mitgeschnittenen `sftp-transfer-finished`-
/// Ereignisse, in Reihenfolge.
fn transfer_finished_errors(emitter: &app_logic::events::TestEmitter) -> Vec<Option<String>> {
    emitter
        .events
        .lock()
        .unwrap()
        .iter()
        .filter(|(name, _)| name == "sftp-transfer-finished")
        .map(|(_, payload)| {
            payload
                .get("error")
                .and_then(|value| value.as_str())
                .map(|text| text.to_string())
        })
        .collect()
}

/// Widerruft am Haltepunkt vor der ersten Operation dieses Befehls — also
/// nach dem Erstzugang (das `sftp-transfer-started`-Ereignis ist schon
/// draußen), aber vor dem Lesen bzw. Schreiben. Der Haltepunkt wird beim
/// ersten Durchlauf verbraucht (s. `set_before_operation_hook`), er trifft
/// also genau diese eine Operation.
fn revoke_before_next_operation(s: &Setup, slot: &ElevatedSftpSlot) {
    let sessions = s.sessions.clone();
    let registry = s.registry.clone();
    let session_id = s.session_id;
    slot.set_before_operation_hook(Box::new(move || {
        registry.remove_session(&sessions, session_id);
    }));
}

/// Spec 0086, T8 (A2.1), Download: `error` im Ereignis ist **wörtlich** das
/// Befehlsergebnis — `ELEVATED_CHANNEL_INACTIVE`, ohne `Channel-Fehler: `.
///
/// Scheitert vor dem Fix am Präfix.
#[tokio::test]
async fn test_t8_a_revoked_download_reports_the_same_text_in_its_event_as_in_its_result() {
    let elevated = GatedSftp::small_tree(0);
    let s = setup(Box::new(elevated.clone())).await;
    let slot = s.slot();
    revoke_before_next_operation(&s, &slot);

    let emitter = Arc::new(app_logic::events::TestEmitter::default());
    let local_dir = tempfile::tempdir().unwrap();
    let local_path = local_dir.path().join("a.txt");
    let session_id = s.session_id;
    let em = emitter.clone();

    let result = with_browser_channel(
        s.session.clone(),
        s.elevated_channel(),
        move |session, channel| async move {
            download_one_file(
                em.as_ref(),
                &session,
                &channel,
                session_id,
                "/t/a.txt",
                local_path,
                None,
            )
            .await
        },
    )
    .await;

    assert!(
        slot.is_revoked(),
        "Vorbedingung: der Haltepunkt hat widerrufen"
    );
    let command_error = result
        .expect_err("nach dem Widerruf darf der Download nicht gelingen")
        .message;
    assert_eq!(
        transfer_finished_errors(&emitter),
        vec![Some(command_error.clone())],
        "die Transferliste muss denselben Text zeigen wie das Befehlsergebnis"
    );
    assert_eq!(
        command_error, ELEVATED_CHANNEL_INACTIVE_TEXT,
        "und dieser Text ist wörtlich ELEVATED_CHANNEL_INACTIVE, ohne Präfix"
    );
    assert!(
        elevated.ops().is_empty(),
        "gelesen wurde nach dem Widerruf nichts, war: {:?}",
        elevated.ops()
    );
}

/// Spec 0086, T8 (A2.1), Upload: dasselbe für die andere Richtung.
#[tokio::test]
async fn test_t8_a_revoked_upload_reports_the_same_text_in_its_event_as_in_its_result() {
    let elevated = GatedSftp::small_tree(0);
    let s = setup(Box::new(elevated.clone())).await;
    let slot = s.slot();
    revoke_before_next_operation(&s, &slot);

    let emitter = Arc::new(app_logic::events::TestEmitter::default());
    let local_dir = tempfile::tempdir().unwrap();
    let local_file = local_dir.path().join("upload.txt");
    std::fs::write(&local_file, b"NEUER-INHALT").unwrap();
    let local_path =
        app_logic::local_path_grants::GrantedLocalPath::unchecked_for_tests(local_file);
    let session_id = s.session_id;
    let em = emitter.clone();

    let result = with_browser_channel(
        s.session.clone(),
        s.elevated_channel(),
        move |session, channel| async move {
            upload_impl(
                em.as_ref(),
                &session,
                &channel,
                session_id,
                &local_path,
                "/t/a.txt",
            )
            .await
        },
    )
    .await;

    assert!(
        slot.is_revoked(),
        "Vorbedingung: der Haltepunkt hat widerrufen"
    );
    let command_error = result
        .expect_err("nach dem Widerruf darf der Upload nicht gelingen")
        .message;
    assert_eq!(
        transfer_finished_errors(&emitter),
        vec![Some(command_error.clone())],
        "die Transferliste muss denselben Text zeigen wie das Befehlsergebnis"
    );
    assert_eq!(command_error, ELEVATED_CHANNEL_INACTIVE_TEXT);
    assert!(
        !elevated.ops().iter().any(|op| op.starts_with("write_file")),
        "geschrieben wurde nach dem Widerruf nichts, war: {:?}",
        elevated.ops()
    );
}

/// Spec 0086, T9 (A2.2): Absicherung gegen eine zu breite Umstellung. Ein
/// gewöhnlicher `ChannelError` **ohne** Widerruf behält seinen Text im
/// Ereignis, Präfix inklusive — nur der Widerrufsfall wird übersetzt.
#[tokio::test]
async fn test_t9_an_ordinary_channel_error_keeps_its_text_in_the_event() {
    let elevated = GatedSftp::small_tree(0);
    let s = setup(Box::new(elevated.clone())).await;

    let emitter = Arc::new(app_logic::events::TestEmitter::default());
    let local_dir = tempfile::tempdir().unwrap();
    let local_path = local_dir.path().join("missing.txt");
    let session_id = s.session_id;
    let em = emitter.clone();

    let result = with_browser_channel(
        s.session.clone(),
        s.elevated_channel(),
        move |session, channel| async move {
            download_one_file(
                em.as_ref(),
                &session,
                &channel,
                session_id,
                // Im Baum nicht vorhanden — `GatedSftp::read_file` liefert
                // dafür einen gewöhnlichen `ChannelError`, keinen Widerruf.
                "/t/nicht-da.txt",
                local_path,
                None,
            )
            .await
        },
    )
    .await;

    let command_error = result
        .expect_err("ein fehlender Pfad muss scheitern")
        .message;
    assert_eq!(
        command_error, "Channel-Fehler: Datei nicht gefunden: /t/nicht-da.txt",
        "ohne Widerruf bleibt der Fehler, wie er ist — Präfix inklusive"
    );
    assert_eq!(
        transfer_finished_errors(&emitter),
        vec![Some(command_error)],
        "und das Ereignis trägt genau diesen Text"
    );
}

/// Spec 0086, A2.2, zweiter Satz: Ein Transfer, der **vor** dem Widerruf
/// fertig war, meldet weiter Erfolg (`error: None`).
///
/// Geprüft am rekursiven Download, der mehrere Transfer-Paare erzeugt:
/// Operation 1 und 2 sind das Durchlaufen des Baums (`list_dir` auf `/t` und
/// `/t/sub`, Issue #622), Operation 3 liest `/t/a.txt`.
/// Operation 4 liest die zweite Datei; genau dort hält der Mock an (`pause_after = 4`), der Test entfernt die
/// Sitzung und gibt frei — `/t/a.txt` läuft also zu Ende, jede weitere
/// Operation scheitert am Widerruf. Der Haltepunkt am Slot taugt hier nicht:
/// er wird beim ersten Durchlauf verbraucht und träfe das `list_dir`.
#[tokio::test]
async fn test_t9_a_transfer_finished_before_the_revocation_still_reports_success() {
    let elevated = GatedSftp::small_tree(4);
    let s = setup(Box::new(elevated.clone())).await;
    let slot = s.slot();

    let emitter = Arc::new(app_logic::events::TestEmitter::default());
    let local_dir = tempfile::tempdir().unwrap();
    let local_root = local_dir.path().join("t");
    let session_id = s.session_id;
    let em = emitter.clone();

    let command = spawn_command(
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

    elevated.wait_until_paused().await;
    let ops = elevated.ops();
    assert_eq!(
        &ops[..3],
        [
            "list_dir /t",
            "list_dir /t/sub",
            "read_file_chunked /t/a.txt"
        ],
        "Vorbedingung: erst der Baum, dann die erste Datei"
    );
    assert!(
        ops[3].starts_with("read_file_chunked "),
        "Vorbedingung: der Mock hält im zweiten Datei-Transfer, war: {ops:?}"
    );
    s.registry.remove_session(&s.sessions, s.session_id);
    s.wait_until_revoked(&slot).await;
    elevated.release();

    assert_inactive(join(command, &elevated).await);
    assert_eq!(
        transfer_finished_errors(&emitter),
        vec![None, Some(ELEVATED_CHANNEL_INACTIVE_TEXT.to_string()),],
        "der erste Transfer war vor dem Widerruf fertig und meldet weiter \
         Erfolg; erst der zweite trägt die Widerrufs-Meldung"
    );
}

// --- Issue #570: „Neue Datei" / „Neuer Ordner" im Dateibrowser -------------

/// Erhöhter Modus: „Neue Datei" läuft auf dem erhöhten Kanal (nie auf dem
/// normalen), schreibt genau eine Audit-Zeile mit dem Ziel-Nutzer und legt
/// eine leere Datei an.
#[tokio::test]
async fn test_create_file_runs_elevated_and_is_audited() {
    let elevated = GatedSftp::small_tree(0);
    let s = setup(Box::new(elevated.clone())).await;
    let (logs, _capture) = capture_logs();

    create_file_impl(&s.session, &s.elevated_channel(), "/t/new.txt")
        .await
        .expect("anlegen gelingt");

    assert!(elevated.exists("/t/new.txt"));
    assert_eq!(
        elevated.0.entries.lock().unwrap()["/t/new.txt"].content,
        Vec::<u8>::new()
    );
    assert!(
        s.normal.calls().is_empty(),
        "kein Rückfall auf den normalen Kanal"
    );
    let lines = logs.audit_lines("create_file", "/t/new.txt");
    assert_eq!(lines.len(), 1, "genau eine Audit-Zeile, war: {lines:?}");
    assert_eq!(lines[0].get("ok").map(String::as_str), Some("true"));
    assert_eq!(
        lines[0].get("target_user").map(String::as_str),
        Some("root")
    );
}

/// Ein vorhandener Name wird nie überschrieben — weder bei „Neue Datei" noch
/// bei „Neuer Ordner" (Datei oder Ordner als Ziel).
#[tokio::test]
async fn test_create_file_and_mkdir_never_overwrite_an_existing_name() {
    let elevated = GatedSftp::small_tree(0);
    let s = setup(Box::new(elevated.clone())).await;

    for path in ["/t/a.txt", "/t/sub"] {
        let err = create_file_impl(&s.session, &s.elevated_channel(), path)
            .await
            .expect_err("vorhandener Name");
        assert_eq!(err.code, Some("SSH_SFTP_ALREADY_EXISTS"), "{path}");
        let err = mkdir_impl(&s.session, &s.elevated_channel(), path)
            .await
            .expect_err("vorhandener Name");
        assert_eq!(err.code, Some("SSH_SFTP_ALREADY_EXISTS"), "{path}");
    }
    assert_eq!(
        elevated.0.entries.lock().unwrap()["/t/a.txt"].content,
        b"ROOT-INHALT".to_vec(),
        "der Inhalt der vorhandenen Datei bleibt unverändert"
    );
    assert!(
        !elevated.ops().iter().any(|o| o.starts_with("create_dir")),
        "mkdir auf vorhandenem Namen ruft create_dir nicht auf"
    );
}

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
    assert_eq!(err.code, Some(super::transfer::FILE_OPERATION_STOPPED));
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
    assert_eq!(err.code, Some(super::transfer::FILE_OPERATION_STOPPED));
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
    assert_eq!(err.code, Some(super::transfer::FILE_OPERATION_STOPPED));
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
    assert_eq!(err.code, Some(super::transfer::FILE_OPERATION_STOPPED));
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
    assert_eq!(err.code, Some(super::transfer::FILE_OPERATION_STOPPED));
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
