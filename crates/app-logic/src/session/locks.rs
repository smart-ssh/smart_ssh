//! Lock hierarchy of one session (issue #923).
//!
//! The permitted order is part of the API: [`SessionLocks`] is the only way
//! to take more than one of the async locks of a session, and it offers no
//! method that takes the transport and then the SFTP channel. The levels,
//! what each lock protects and the rules around them are documented in
//! `docs/architecture.md`, section "Lock hierarchy of a session".
//!
//! ```text
//! level 1  elevated-channel slot   (app-shell, ElevatedSftpRegistry)
//! level 2  normal SFTP channel     SessionLocks::sftp
//! level 3  transport               SessionLocks::transport
//! ```
//!
//! A level may be taken while holding a lower number, never the reverse.
//! Level 1 is never held together with level 3: the revocation of an
//! elevated channel runs outside the transport lock.

use super::{NormalSftpGuard, Session, TransportGuard};

/// Ordered access to the async locks of a [`Session`]. Get one with
/// [`Session::locks`].
///
/// Every method takes `&mut self` and returns guards that borrow it. While
/// a guard is alive the handle is borrowed, so no second acquisition through
/// the same handle is possible: transport first and SFTP afterwards is not
/// expressible. Taking both in the permitted order is
/// [`SessionLocks::sftp_then_transport`].
///
/// The forbidden order does not compile; the twin below differs only in the
/// forbidden line:
///
/// ```compile_fail
/// # use app_logic::session::Session;
/// async fn f(session: &Session) {
///     let mut locks = session.locks();
///     let _transport = locks.transport().await;
///     let _sftp = locks.sftp().await;
/// }
/// ```
///
/// Twin (the permitted order, in one call):
///
/// ```
/// # use app_logic::session::Session;
/// async fn f(session: &Session) {
///     let mut locks = session.locks();
///     let (_sftp, _transport) = locks.sftp_then_transport().await;
/// }
/// ```
///
/// Taking the transport and holding it while asking for SFTP again through
/// the same handle is rejected as well:
///
/// ```compile_fail
/// # use app_logic::session::Session;
/// async fn f(session: &Session) {
///     let mut locks = session.locks();
///     let transport = locks.transport().await;
///     let sftp = locks.sftp().await;
///     drop((transport, sftp));
/// }
/// ```
pub struct SessionLocks<'a> {
    session: &'a Session,
}

impl<'a> SessionLocks<'a> {
    pub(super) fn new(session: &'a Session) -> Self {
        Self { session }
    }

    /// Level 2: the normal SFTP channel.
    pub async fn sftp(&mut self) -> NormalSftpGuard<'_> {
        self.session.lock_sftp().await
    }

    /// Level 3: the transport.
    pub async fn transport(&mut self) -> TransportGuard<'_> {
        self.session.transport.lock().await
    }

    /// Level 2, then level 3, in the permitted order.
    pub async fn sftp_then_transport(&mut self) -> (NormalSftpGuard<'_>, TransportGuard<'_>) {
        let session = self.session;
        let sftp = session.lock_sftp().await;
        let transport = session.transport.lock().await;
        (sftp, transport)
    }

    /// Level 2, then level 3 **only if the SFTP channel is still closed**
    /// (the lazy open of [`crate::orchestration::ensure_sftp_open`]). An
    /// already open channel does not wait for a busy transport.
    pub async fn sftp_then_transport_if_closed(
        &mut self,
    ) -> (NormalSftpGuard<'_>, Option<TransportGuard<'_>>) {
        let session = self.session;
        let sftp = session.lock_sftp().await;
        let transport = if sftp.is_open() {
            None
        } else {
            Some(session.transport.lock().await)
        };
        (sftp, transport)
    }
}

impl Session {
    /// Ordered access to the session's async locks — see [`SessionLocks`].
    pub fn locks(&self) -> SessionLocks<'_> {
        SessionLocks::new(self)
    }
}
