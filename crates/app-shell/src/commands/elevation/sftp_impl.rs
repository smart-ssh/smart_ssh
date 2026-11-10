//! `SftpSession` for [`BrowserSftp`], split out of `elevation.rs` (file size limit).

use async_trait::async_trait;

use ssh_manager_core::ssh::{
    ChunkControl, ChunkSource, RemoteEntry, SftpSession, SshError, TransferEnd,
};

use super::BrowserSftp;

// Jede Trait-Methode ist derselbe Dreischritt: Zugang für diese Operation
// holen (dabei Widerruf prüfen), delegieren, Sperre freigeben. Bewusst
// ausgeschrieben statt per Makro erzeugt: `#[async_trait]` expandiert vor
// `macro_rules!`, und an dieser Stelle soll ohnehin jede einzelne Operation
// im Quelltext sichtbar sein. Eine neue Trait-Methode in `SftpSession`
// erzwingt hier einen Eintrag, sonst kompiliert die Kiste nicht.
#[async_trait]
impl SftpSession for BrowserSftp<'_> {
    async fn list_dir(&mut self, path: &str) -> Result<Vec<RemoteEntry>, SshError> {
        self.operation().await?.sftp().list_dir(path).await
    }

    async fn read_file(&mut self, path: &str) -> Result<Vec<u8>, SshError> {
        self.operation().await?.sftp().read_file(path).await
    }

    async fn read_file_capped(
        &mut self,
        path: &str,
        max_bytes: u64,
        progress: &mut (dyn FnMut(u64) + Send),
    ) -> Result<Vec<u8>, SshError> {
        self.operation()
            .await?
            .sftp()
            .read_file_capped(path, max_bytes, progress)
            .await
    }

    async fn write_file(&mut self, path: &str, content: &[u8]) -> Result<(), SshError> {
        self.operation()
            .await?
            .sftp()
            .write_file(path, content)
            .await
    }

    async fn read_file_chunked(
        &mut self,
        path: &str,
        chunk_size: usize,
        on_chunk: &mut (dyn for<'a> FnMut(&'a [u8]) -> ChunkControl + Send),
    ) -> Result<TransferEnd, SshError> {
        self.operation()
            .await?
            .sftp()
            .read_file_chunked(path, chunk_size, on_chunk)
            .await
    }

    async fn write_file_chunked(
        &mut self,
        path: &str,
        chunk_size: usize,
        next_chunk: &mut (dyn for<'a> FnMut(&'a mut [u8]) -> ChunkSource + Send),
    ) -> Result<TransferEnd, SshError> {
        self.operation()
            .await?
            .sftp()
            .write_file_chunked(path, chunk_size, next_chunk)
            .await
    }

    async fn read_range(
        &mut self,
        path: &str,
        offset: u64,
        len: usize,
    ) -> Result<Vec<u8>, SshError> {
        self.operation()
            .await?
            .sftp()
            .read_range(path, offset, len)
            .await
    }

    async fn write_range(
        &mut self,
        path: &str,
        offset: u64,
        data: &[u8],
        truncate: bool,
    ) -> Result<(), SshError> {
        self.operation()
            .await?
            .sftp()
            .write_range(path, offset, data, truncate)
            .await
    }

    async fn stat(&mut self, path: &str) -> Result<RemoteEntry, SshError> {
        self.operation().await?.sftp().stat(path).await
    }

    async fn read_link(&mut self, path: &str) -> Result<String, SshError> {
        self.operation().await?.sftp().read_link(path).await
    }

    async fn lstat(&mut self, path: &str) -> Result<RemoteEntry, SshError> {
        self.operation().await?.sftp().lstat(path).await
    }

    async fn remove(&mut self, path: &str) -> Result<(), SshError> {
        self.operation().await?.sftp().remove(path).await
    }

    async fn rename(&mut self, from: &str, to: &str) -> Result<(), SshError> {
        self.operation().await?.sftp().rename(from, to).await
    }

    async fn posix_rename(&mut self, from: &str, to: &str) -> Result<bool, SshError> {
        self.operation().await?.sftp().posix_rename(from, to).await
    }

    async fn create_dir(&mut self, path: &str) -> Result<(), SshError> {
        self.operation().await?.sftp().create_dir(path).await
    }

    async fn create_file_exclusive(&mut self, path: &str) -> Result<(), SshError> {
        self.operation()
            .await?
            .sftp()
            .create_file_exclusive(path)
            .await
    }

    async fn remove_dir(&mut self, path: &str) -> Result<(), SshError> {
        self.operation().await?.sftp().remove_dir(path).await
    }

    async fn set_permissions(&mut self, path: &str, mode: u32) -> Result<(), SshError> {
        self.operation()
            .await?
            .sftp()
            .set_permissions(path, mode)
            .await
    }

    async fn set_owner(&mut self, path: &str, uid: u32, gid: u32) -> Result<(), SshError> {
        self.operation()
            .await?
            .sftp()
            .set_owner(path, uid, gid)
            .await
    }
}
