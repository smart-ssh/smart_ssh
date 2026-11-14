//! Recording of `tracing` events for the unit tests of this crate (issue #1336).
//!
//! **One** global subscriber per test process, writing into a thread-local
//! buffer. No test of this binary may install a scoped dispatcher
//! (`tracing::subscriber::with_default`): registering/dropping one rebuilds
//! the process-wide callsite interest cache concurrently with other threads,
//! and a callsite can then be cached as "not interested", which left the
//! capture buffer of another test empty. Tests that need to read log output
//! use this module instead (guarded by `tests/scoped_dispatcher_guard.rs`).
//!
//! Aufzeichnung der `tracing`-Ereignisse dieser Testsuite.
//!
//! Dasselbe Muster wie `ai_providers::test_support` (dort steht die
//! ausführliche Begründung; Spec 0086, A4.2: hier stand zusätzlich der Name
//! eines `app-shell`-Moduls, das es nicht gibt): **ein** globaler Subscriber
//! pro Testprozess
//! (`Once`), der in einen thread-lokalen Puffer schreibt — mehrere
//! `set_global_default`-Aufrufe im selben Testbinary gewinnen sonst nur beim
//! ersten, und die übrigen Tests sähen nie ihre eigenen Zeilen.
//!
//! **Spec 0094, §7:** Aufgezeichnet wird auf `TRACE`, nicht mehr nur auf
//! `ERROR`. Vorher begrenzte `with_max_level(Level::ERROR)` die Aufnahme,
//! damit T-A10 (eine Aussage über ERROR-Ereignisse) nicht versehentlich das
//! INFO-Ereignis von `evaluate_explained` mitlas. Spec 0094 braucht
//! genau die andere Richtung: Aussagen darüber, was auf `info` **nicht** und
//! auf `debug` **doch** steht, sind nur möglich, wenn beide Level im Puffer
//! landen. Gefiltert wird deshalb beim Lesen —
//! [`recorded_error_events`] für T-A10, [`lines_at_info_or_above`] und
//! [`debug_lines`] für T1–T3.

thread_local! {
    static BUFFER: std::cell::RefCell<Vec<u8>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

#[derive(Clone, Default)]
pub(crate) struct Writer;

impl std::io::Write for Writer {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        BUFFER.with(|b| b.borrow_mut().extend_from_slice(buf));
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Writer {
    type Writer = Writer;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

/// Installiert den Subscriber einmal pro Testprozess und leert den
/// Puffer dieses Threads. Jeder Test ruft das als Erstes auf.
pub(crate) fn start_recording() {
    static INIT: std::sync::Once = std::sync::Once::new();
    INIT.call_once(|| {
        let subscriber = tracing_subscriber::fmt()
            .json()
            .with_max_level(tracing::Level::TRACE)
            .with_writer(Writer)
            .finish();
        // `let _ =`: schlägt nur fehl, wenn schon ein globaler Default
        // gesetzt ist — dann ist es dank `Once` bereits dieser hier.
        let _ = tracing::subscriber::set_global_default(subscriber);
        // Callsites, die vor dieser Zeile (noch ohne Subscriber) getroffen
        // wurden, werden sonst unter Umständen als „nicht interessiert"
        // zwischengespeichert (Issue #1336).
        tracing::callsite::rebuild_interest_cache();
    });
    BUFFER.with(|b| b.borrow_mut().clear());
}

/// Alle aufgezeichneten Ereignisse, eine JSON-Zeile je Ereignis, über
/// **alle** Level (s. `with_max_level(TRACE)` oben).
pub(crate) fn recorded_lines() -> Vec<String> {
    BUFFER.with(|b| {
        String::from_utf8(b.borrow().clone())
            .expect("Log ist kein UTF-8")
            .lines()
            .map(str::to_string)
            .collect()
    })
}

/// Nur die ERROR-Ereignisse (Spec 0077, T-A10). Vor Spec 0094 ergab sich
/// das aus `with_max_level(ERROR)`; jetzt wird beim Lesen gefiltert,
/// damit dieselbe Aussage gilt, obwohl der Puffer alle Level trägt.
pub(crate) fn recorded_error_events() -> Vec<String> {
    lines_at_levels(&["ERROR"])
}

/// Spec 0094, A1: die Zeilen, über die A1 eine Aussage macht — `info`,
/// `warn`, `error`. `debug`/`trace` bleiben draußen, denn dort ist
/// Inhalt laut A2 ausdrücklich erlaubt.
pub(crate) fn lines_at_info_or_above() -> Vec<String> {
    lines_at_levels(&["INFO", "WARN", "ERROR"])
}

/// Spec 0094, A2: die `debug`-Zeilen, die den Inhalt tragen.
pub(crate) fn debug_lines() -> Vec<String> {
    lines_at_levels(&["DEBUG"])
}

/// Filtert nach dem `level`-Feld der JSON-Zeile. Das Feld steht im
/// JSON-Format von `tracing-subscriber` immer als `"level":"INFO"` o. Ä.
/// da — bewusst wörtlich gesucht statt die Zeile zu parsen: ein
/// Parse-Fehler dürfte hier nie stillschweigend zu „keine Zeile, also
/// kein Treffer" führen, denn das wäre bei einer Abwesenheits-Aussage
/// ein falsches Grün.
fn lines_at_levels(levels: &[&str]) -> Vec<String> {
    let lines = recorded_lines();
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
        .collect()
}
