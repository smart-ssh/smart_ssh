/// TCP forwarder for one connection that can go silent: once frozen it
/// keeps the sockets open but forwards nothing in either direction, like a
/// network that drops packets without a reset (issue #353).
pub struct FreezableProxy {
    pub port: u16,
    frozen: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// Number of server-to-client reads seen so far (every answered
    /// keepalive adds at least one).
    pub downstream_reads: tokio::sync::watch::Receiver<usize>,
    task: tokio::task::JoinHandle<()>,
}

impl FreezableProxy {
    pub async fn start(target: std::net::SocketAddr) -> Self {
        use std::sync::atomic::Ordering;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        let frozen = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = frozen.clone();
        let (reads_tx, downstream_reads) = tokio::sync::watch::channel(0usize);
        let task = tokio::spawn(async move {
            let (inbound, _) = listener.accept().await.unwrap();
            let outbound = tokio::net::TcpStream::connect(target).await.unwrap();
            let (mut in_r, mut in_w) = inbound.into_split();
            let (mut out_r, mut out_w) = outbound.into_split();
            let f1 = flag.clone();
            let up = tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                while let Ok(n) = in_r.read(&mut buf).await {
                    if n == 0 {
                        break;
                    }
                    if !f1.load(Ordering::SeqCst) && out_w.write_all(&buf[..n]).await.is_err() {
                        break;
                    }
                }
            });
            let mut buf = [0u8; 4096];
            while let Ok(n) = out_r.read(&mut buf).await {
                if n == 0 {
                    break;
                }
                reads_tx.send_modify(|c| *c += 1);
                if !flag.load(Ordering::SeqCst) && in_w.write_all(&buf[..n]).await.is_err() {
                    break;
                }
            }
            up.abort();
        });
        Self {
            port,
            frozen,
            downstream_reads,
            task,
        }
    }

    pub fn freeze(&self) {
        self.frozen.store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

impl Drop for FreezableProxy {
    fn drop(&mut self) {
        self.task.abort();
    }
}
