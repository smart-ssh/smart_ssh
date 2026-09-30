//! Test-Infrastruktur dieser Crate, `#[cfg(test)]`-only — Mitschnitt der
//! `tracing`-Ereignisse (Spec 0094, §7).
//!
//! Dasselbe Grundmuster wie `ai_providers::test_support`,
//! `app_logic::test_support::log_capture` und
//! `ssh_manager_core::filter::tests`: **ein** globaler Subscriber pro
//! Testprozess (`Once`), aufgezeichnet wird auf `TRACE`, gefiltert wird
//! beim Lesen. Mehrere `set_global_default`-Aufrufe im selben Testbinary
//! gewinnen sonst nur beim ersten, und die übrigen Tests sähen nie ihre
//! eigenen Zeilen.
//!
//! **Ein Unterschied zu den anderen drei, und er ist der Grund, warum es
//! diese Datei bis Spec 0094 nicht gab:** Die Tests dieser Crate laufen auf
//! `#[tokio::test(flavor = "multi_thread")]`, und die geprüfte Log-Zeile
//! entsteht in `tool_server.rs` innerhalb eines `tokio::spawn`, also auf
//! einem Worker-Thread. Ein *thread-lokaler* Puffer wie dort würde diese
//! Zeile nie zu sehen bekommen — der Test-Thread liest seinen eigenen,
//! leeren Puffer und eine Abwesenheits-Aussage („steht nicht auf `info`")
//! wäre trivial wahr, also wertlos.
//!
//! Deshalb hier ein **prozessweiter** Puffer unter `Mutex`. Damit sich zwei
//! gleichzeitig laufende Tests nicht gegenseitig die Zeilen in den Puffer
//! schreiben, serialisiert [`start_recording`] die aufzeichnenden Tests über
//! eine zweite Sperre, die der zurückgegebene [`CaptureGuard`] bis zum Ende
//! des Tests hält.

use std::sync::{Mutex, MutexGuard};

/// Prozessweiter Puffer (s. Modul-Doc: thread-lokal reicht hier nicht).
static BUFFER: Mutex<Vec<u8>> = Mutex::new(Vec::new());

/// Serialisiert die aufzeichnenden Tests gegeneinander.
static CAPTURE_LOCK: Mutex<()> = Mutex::new(());

/// Nimmt eine vergiftete Sperre trotzdem an. Ein `panic!` in einem Test
/// (also ein fehlgeschlagener `assert!`) vergiftet sonst beide Sperren, und
/// jeder folgende aufzeichnende Test scheiterte an der Vergiftung statt an
/// seiner eigenen Aussage — ein einzelner echter Fund würde dann als
/// Dutzend Fehlschläge erscheinen und den eigentlichen verdecken.
fn lock<T>(m: &'static Mutex<T>) -> MutexGuard<'static, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[derive(Clone, Default)]
struct SharedTestWriter;

impl std::io::Write for SharedTestWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        lock(&BUFFER).extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for SharedTestWriter {
    type Writer = SharedTestWriter;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

/// Hält die Aufzeichnungs-Sperre, solange der Test läuft. Muss in einer
/// Variablen gehalten werden (`let _guard = start_recording();`) — ein
/// `let _ = ...` würde ihn sofort droppen und die Serialisierung aufheben.
pub(crate) struct CaptureGuard(#[allow(dead_code)] MutexGuard<'static, ()>);

/// Installiert den Subscriber einmal pro Testprozess, übernimmt die
/// Aufzeichnungs-Sperre und leert den Puffer. Jeder aufzeichnende Test ruft
/// das als Erstes auf und hält den Rückgabewert bis zum Ende.
pub(crate) fn start_recording() -> CaptureGuard {
    static INIT: std::sync::Once = std::sync::Once::new();
    let guard = lock(&CAPTURE_LOCK);
    INIT.call_once(|| {
        let subscriber = tracing_subscriber::fmt()
            .json()
            .with_max_level(tracing::level_filters::LevelFilter::TRACE)
            .with_writer(SharedTestWriter)
            .finish();
        // `let _ =`: schlägt nur fehl, wenn schon ein globaler Default
        // gesetzt ist — dank `Once` wäre das bereits dieser hier.
        let _ = tracing::subscriber::set_global_default(subscriber);
    });
    lock(&BUFFER).clear();
    CaptureGuard(guard)
}

/// Der rohe aufgezeichnete Text.
pub(crate) fn recorded_text() -> String {
    String::from_utf8(lock(&BUFFER).clone()).expect("Log ist kein UTF-8")
}

/// Spec 0094, A1: die Zeilen, über die A1 eine Aussage macht — `info`,
/// `warn`, `error`. `debug`/`trace` bleiben draußen, denn dort ist Inhalt
/// laut A2 ausdrücklich erlaubt.
pub(crate) fn recorded_lines_at_info_or_above() -> Vec<String> {
    lines_at_levels(&["INFO", "WARN", "ERROR"])
}

/// Spec 0094, A2: die `debug`-Zeilen, die den Inhalt tragen.
pub(crate) fn recorded_debug_lines() -> Vec<String> {
    lines_at_levels(&["DEBUG"])
}

/// Filtert nach dem `level`-Feld der JSON-Zeile. Bewusst wörtlich gesucht
/// statt die Zeile zu parsen — und mit einer Zusicherung davor, dass jede
/// aufgezeichnete Zeile ein erkennbares Level trägt: eine Zeile, die der
/// Filter nicht einordnen kann, würde bei einer Abwesenheits-Aussage
/// stillschweigend zu einem falschen Grün führen.
fn lines_at_levels(levels: &[&str]) -> Vec<String> {
    let text = recorded_text();
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

    /// Spec 0094, §7: Selbstprüfung der Aufzeichnung. T7 und T9 (mcp-server)
    /// sind nur belastbar, wenn der Mitschnitt `debug` und `info`
    /// auseinanderhält **und** eine Zeile von einem Worker-Thread sieht —
    /// genau der Fall, an dem ein thread-lokaler Puffer scheitern würde.
    #[tokio::test(flavor = "multi_thread")]
    async fn test_log_capture_records_debug_and_info_from_a_worker_thread() {
        let _guard = start_recording();

        tokio::spawn(async {
            tracing::debug!(marker = "nur-debug-0094", "capture self-check (debug)");
            tracing::info!(marker = "nur-info-0094", "capture self-check (info)");
        })
        .await
        .unwrap();

        let debug_lines = recorded_debug_lines();
        let info_or_above = recorded_lines_at_info_or_above();
        assert!(
            debug_lines.iter().any(|l| l.contains("nur-debug-0094")),
            "debug-Ereignis vom Worker-Thread muss aufgezeichnet werden: {debug_lines:?}"
        );
        assert!(
            !debug_lines.iter().any(|l| l.contains("nur-info-0094")),
            "info-Ereignis darf nicht als debug-Zeile gelesen werden: {debug_lines:?}"
        );
        assert!(
            info_or_above.iter().any(|l| l.contains("nur-info-0094")),
            "info-Ereignis vom Worker-Thread muss aufgezeichnet werden: {info_or_above:?}"
        );
        assert!(
            !info_or_above.iter().any(|l| l.contains("nur-debug-0094")),
            "debug-Ereignis darf nicht als info-Zeile gelesen werden: {info_or_above:?}"
        );
    }
}
