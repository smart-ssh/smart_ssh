//! Process-wide log capture routed per thread (Issue #114).

use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex};

/// Eingefangene `tracing`-Ereignisse, Feldname → Wert.
#[derive(Clone, Default)]
pub(crate) struct CapturedLogs(pub(crate) Arc<StdMutex<Vec<HashMap<String, String>>>>);

impl CapturedLogs {
    /// Die Audit-Zeilen zu `action` auf `path` (s.
    /// `elevation::audit_elevated_change`).
    pub(crate) fn audit_lines(&self, action: &str, path: &str) -> Vec<HashMap<String, String>> {
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
pub(crate) struct CaptureGuard(Option<CapturedLogs>);

impl Drop for CaptureGuard {
    fn drop(&mut self) {
        let previous = self.0.take();
        THREAD_CAPTURE.with(|capture| *capture.borrow_mut() = previous);
    }
}

pub(crate) fn capture_logs() -> (CapturedLogs, CaptureGuard) {
    static INSTALL: std::sync::Once = std::sync::Once::new();
    INSTALL.call_once(|| {
        tracing::subscriber::set_global_default(RoutingSubscriber)
            .expect("in diesem Testlauf darf niemand sonst einen prozessweiten Subscriber setzen");
    });
    let logs = CapturedLogs::default();
    let previous = THREAD_CAPTURE.with(|capture| capture.borrow_mut().replace(logs.clone()));
    (logs, CaptureGuard(previous))
}
