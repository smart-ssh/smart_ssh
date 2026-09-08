//! Gemeinsame Test-Infrastruktur für alle Testmodule dieser Crate,
//! `#[cfg(test)]`-only.
//!
//! Spec-Reviewer-Fund (Spec 0049, Review dieses Schritts): `request_logging.rs`
//! und `discovery.rs` installierten ursprünglich **je einen eigenen**
//! globalen `tracing`-Test-Subscriber (`set_global_default`). Das schlägt
//! fehl, sobald beide Testmodule im selben Testbinary (hier: derselben
//! `ai_providers`-Lib-Testsuite) laufen — `set_global_default` gewinnt nur
//! beim ersten Aufruf im Prozess, jeder weitere schlägt still fehl
//! (`let _ =`), und je nachdem, welches Testmodul zuerst dran ist, schreiben
//! die *anderen* Tests dann in den falschen Thread-lokalen Puffer und sehen
//! nie ihre eigenen Log-Zeilen. Dasselbe Grundmuster wie
//! `app_shell::orchestration`s `install_test_subscriber_once` (dortiger
//! Kommentar erklärt außerdem, warum ein globaler statt eines
//! thread-lokal-scopenden `with_default` nötig ist) — hier als eine
//! einzige, von allen Testmodulen dieser Crate geteilte Instanz.

thread_local! {
    static TEST_LOG_BUFFER: std::cell::RefCell<Vec<u8>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

#[derive(Clone, Default)]
struct ThreadLocalTestWriter;

impl std::io::Write for ThreadLocalTestWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        TEST_LOG_BUFFER.with(|b| b.borrow_mut().extend_from_slice(buf));
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for ThreadLocalTestWriter {
    type Writer = ThreadLocalTestWriter;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

/// Installiert genau einmal pro Testprozess einen globalen `tracing`-
/// Subscriber, der in den Thread-lokalen Puffer dieses Threads schreibt.
/// Vor jedem Test [`clear_log_buffer`] aufrufen, danach [`log_buffer_text`]
/// lesen.
pub(crate) fn install_test_subscriber_once() {
    static INIT: std::sync::Once = std::sync::Once::new();
    INIT.call_once(|| {
        let subscriber = tracing_subscriber::fmt()
            .json()
            .with_writer(ThreadLocalTestWriter)
            .finish();
        // `let _ =`: schlägt nur fehl, wenn bereits ein globaler Default
        // gesetzt ist — dann ist ohnehin schon dieser hier aktiv (dank
        // `Once`, kein anderer Test-Subscriber mehr in dieser Crate).
        let _ = tracing::subscriber::set_global_default(subscriber);
    });
}

pub(crate) fn clear_log_buffer() {
    TEST_LOG_BUFFER.with(|b| b.borrow_mut().clear());
}

pub(crate) fn log_buffer_text() -> String {
    TEST_LOG_BUFFER.with(|b| String::from_utf8(b.borrow().clone()).unwrap())
}
