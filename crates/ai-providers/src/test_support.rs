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
//! nie ihre eigenen Log-Zeilen. Deshalb hier **eine** einzige, von allen
//! Testmodulen dieser Crate geteilte Instanz ([`install_test_subscriber_once`]
//! unten).
//!
//! Spec 0086, A4.2: Hier stand zusätzlich ein Verweis auf eine gleichnamige
//! Funktion in `app-shell`. Die gibt es dort nicht — sie liegt in dieser
//! Datei, der Verweis zeigte also auf sich selbst.
//!
//! Das heutige Ziel ist `app_logic::test_support::log_capture`: dort steht
//! unter **„Warum global statt `with_default`"** die Begründung, die der
//! alte Verweis mitnahm — `tracing-core` cacht das Callsite-Interesse
//! prozessweit, ein `with_default` auf einem anderen Thread gewinnt das
//! Wettrennen nicht zuverlässig zurück. Sie gilt für den Subscriber hier
//! genauso. Wer das Muster ein drittes Mal sehen will:
//! `ssh_manager_core::filter::tests` (Spec 0077, T-A10) hält den
//! ERROR-Mitschnitt ebenso.

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
            .with_max_level(tracing::level_filters::LevelFilter::TRACE)
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

/// Spec 0094, A1: die Zeilen, über die A1 eine Aussage macht — `info`,
/// `warn`, `error`. `debug`/`trace` bleiben draußen, denn dort ist Inhalt
/// laut A2 ausdrücklich erlaubt. Der Subscriber oben zeichnet bereits auf
/// `TRACE` auf; gefiltert wird deshalb hier beim Lesen.
pub(crate) fn log_lines_at_info_or_above() -> Vec<String> {
    log_lines_at_levels(&["INFO", "WARN", "ERROR"])
}

/// Spec 0094, A2: die `debug`-Zeilen, die den Inhalt tragen.
pub(crate) fn debug_log_lines() -> Vec<String> {
    log_lines_at_levels(&["DEBUG"])
}

/// Filtert nach dem `level`-Feld der JSON-Zeile. Bewusst wörtlich gesucht
/// statt die Zeile zu parsen — und mit einer Zusicherung davor, dass jede
/// aufgezeichnete Zeile ein erkennbares Level trägt: eine Zeile, die der
/// Filter nicht einordnen kann, würde bei einer Abwesenheits-Aussage
/// („steht nicht auf `info`") sonst stillschweigend zu einem falschen Grün
/// führen.
fn log_lines_at_levels(levels: &[&str]) -> Vec<String> {
    let text = log_buffer_text();
    let lines: Vec<&str> = text.lines().collect();
    for line in &lines {
        assert!(
            ["TRACE", "DEBUG", "INFO", "WARN", "ERROR"]
                .iter()
                .any(|l| line.contains(&format!("\"level\":\"{l}\""))),
            "Log-Zeile ohne erkennbares level-Feld — der Filter würde sie \
             stillschweigend übergehen: {line}"
        );
    }
    lines
        .into_iter()
        .filter(|line| {
            levels
                .iter()
                .any(|l| line.contains(&format!("\"level\":\"{l}\"")))
        })
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod capture_self_check {
    use super::*;

    /// Spec 0094, §7: dieselbe Selbstprüfung wie in
    /// `ssh_manager_core::filter::tests` — die Aussagen von T4/T5/T8/T9 sind
    /// nur belastbar, wenn der Mitschnitt `debug` und `info` wirklich sieht
    /// und beim Lesen auseinanderhält.
    #[test]
    fn test_log_capture_records_debug_and_info_separately() {
        install_test_subscriber_once();
        clear_log_buffer();

        tracing::debug!(marker = "nur-debug-0094", "capture self-check (debug)");
        tracing::info!(marker = "nur-info-0094", "capture self-check (info)");

        let debug_lines = debug_log_lines();
        let info_or_above = log_lines_at_info_or_above();
        assert!(
            debug_lines.iter().any(|l| l.contains("nur-debug-0094")),
            "debug-Ereignis muss aufgezeichnet werden: {debug_lines:?}"
        );
        assert!(
            !debug_lines.iter().any(|l| l.contains("nur-info-0094")),
            "info-Ereignis darf nicht als debug-Zeile gelesen werden: {debug_lines:?}"
        );
        assert!(
            info_or_above.iter().any(|l| l.contains("nur-info-0094")),
            "info-Ereignis muss aufgezeichnet werden: {info_or_above:?}"
        );
        assert!(
            !info_or_above.iter().any(|l| l.contains("nur-debug-0094")),
            "debug-Ereignis darf nicht als info-Zeile gelesen werden: {info_or_above:?}"
        );
    }
}
