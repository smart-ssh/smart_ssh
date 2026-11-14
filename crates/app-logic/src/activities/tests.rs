use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use super::*;

struct Collect(Mutex<Vec<(String, serde_json::Value)>>);
impl EventEmitter for Collect {
    fn emit_event(&self, event: &str, payload: serde_json::Value) {
        self.0.lock().unwrap().push((event.to_owned(), payload));
    }
}

fn setup() -> (Arc<ActivityRegistry>, Arc<Collect>, Arc<AtomicU64>) {
    let em = Arc::new(Collect(Mutex::new(Vec::new())));
    let now = Arc::new(AtomicU64::new(1_000));
    let n = Arc::clone(&now);
    let reg = ActivityRegistry::with_clock(em.clone(), Arc::new(move || n.load(Ordering::SeqCst)));
    (reg, em, now)
}

fn spec(kind: &str, cancellable: bool) -> ActivitySpec {
    ActivitySpec {
        kind: kind.into(),
        key: "k".into(),
        title_key: "t".into(),
        cancellable,
        total: Some(100),
        ..Default::default()
    }
}

fn count(em: &Collect) -> usize {
    em.0.lock().unwrap().len()
}

#[test]
fn progress_is_throttled_to_four_per_second_and_final_always_sent() {
    let (reg, em, now) = setup();
    let h = reg.start(spec("a", false));
    assert_eq!(count(&em), 1); // start event
                               // 20 updates within the same 250 ms window: none sent.
    for i in 1..=20 {
        now.fetch_add(10, Ordering::SeqCst);
        h.progress(i, Some(100));
    }
    assert_eq!(count(&em), 1);
    // Over one simulated second at 10 ms steps: at most 4 progress events.
    for i in 21..=120 {
        now.fetch_add(10, Ordering::SeqCst);
        h.progress(i, Some(100));
    }
    let progress_events = count(&em) - 1;
    assert!((1..=4).contains(&progress_events), "{progress_events}");
    // The last (throttled) progress value is not lost: the final event has it.
    let id = h.id().to_owned();
    h.succeed();
    let events = em.0.lock().unwrap();
    let last = &events.last().unwrap().1;
    assert_eq!(last["id"], id);
    assert_eq!(last["state"], "succeeded");
    assert_eq!(last["done"], 100);
    assert_eq!(events.last().unwrap().0, names::ACTIVITY_CHANGED);
}

#[test]
fn failure_final_state_is_sent_even_inside_throttle_window() {
    let (reg, em, _now) = setup();
    let h = reg.start(spec("a", false));
    h.progress(5, Some(100));
    h.fail("E", "boom");
    let list = reg.list();
    assert_eq!(list[0].state, ActivityState::Failed);
    assert_eq!(list[0].error_message.as_deref(), Some("boom"));
    assert_eq!(em.0.lock().unwrap().last().unwrap().1["state"], "failed");
}

#[test]
fn dropping_unfinished_handle_marks_failed() {
    let (reg, em, _now) = setup();
    let h = reg.start(spec("a", false));
    drop(h);
    let a = &reg.list()[0];
    assert_eq!(a.state, ActivityState::Failed);
    assert_eq!(a.error_code.as_deref(), Some(ERR_DROPPED));
    assert_eq!(em.0.lock().unwrap().last().unwrap().1["state"], "failed");
}

#[test]
fn dropping_after_succeed_keeps_success() {
    let (reg, _em, _now) = setup();
    reg.start(spec("a", false)).succeed();
    assert_eq!(reg.list()[0].state, ActivityState::Succeeded);
}

#[test]
fn cancel_runs_callback_and_ends_cancelled() {
    let (reg, _em, _now) = setup();
    let h = reg.start(spec("a", false));
    let called = Arc::new(AtomicBool::new(false));
    let c = Arc::clone(&called);
    h.on_cancel(move || c.store(true, Ordering::SeqCst));
    assert!(reg.list()[0].cancellable);
    reg.cancel(h.id()).unwrap();
    assert!(called.load(Ordering::SeqCst));
    assert_eq!(reg.list()[0].state, ActivityState::Cancelled);
    // A late success does not overwrite the cancelled state.
    h.succeed();
    assert_eq!(reg.list()[0].state, ActivityState::Cancelled);
}

#[test]
fn cancel_of_non_cancellable_is_refused() {
    let (reg, _em, _now) = setup();
    let h = reg.start(spec("a", false));
    let err = reg.cancel(h.id()).unwrap_err();
    assert_eq!(err.code, Some("activity_not_cancellable"));
    assert_eq!(reg.list()[0].state, ActivityState::Running);
}

#[test]
fn list_is_in_start_order_and_dismiss_removes_finished_only() {
    let (reg, em, now) = setup();
    let first = reg.start(spec("a", false));
    now.fetch_add(5, Ordering::SeqCst);
    let second = reg.start(spec("b", false));
    let ids: Vec<_> = reg.list().iter().map(|a| a.kind.clone()).collect();
    assert_eq!(ids, ["a", "b"]);
    let id1 = first.id().to_owned();
    assert!(reg.dismiss(&id1).is_err()); // still running
    first.succeed();
    reg.dismiss(&id1).unwrap();
    assert_eq!(reg.list().len(), 1);
    let ev = em.0.lock().unwrap().last().unwrap().1.clone();
    assert_eq!(ev["dismissed"], true);
    assert_eq!(ev["id"], id1);
    drop(second);
}

#[test]
fn status_change_is_sent_immediately_and_cleared_on_finish() {
    let (reg, em, _now) = setup();
    let h = reg.start(spec("a", false));
    h.status(Some("activities.verifying"));
    assert_eq!(count(&em), 2);
    assert_eq!(
        reg.list()[0].status_key.as_deref(),
        Some("activities.verifying")
    );
    h.succeed();
    assert_eq!(reg.list()[0].status_key, None);
}

#[test]
fn indeterminate_total_stays_none() {
    let (reg, _em, _now) = setup();
    let mut s = spec("a", false);
    s.total = None;
    let h = reg.start(s);
    h.progress(10, None);
    assert_eq!(reg.list()[0].total, None);
}

fn resumable(kind: &str, key: &str) -> ActivitySpec {
    ActivitySpec {
        resume: Some(ResumeSpec {
            kind: kind.into(),
            key: key.into(),
        }),
        ..spec(kind, false)
    }
}

#[test]
fn quit_blockers_are_running_cancellable_or_resumable_activities_only() {
    let (reg, _em, _now) = setup();
    let _plain = reg.start(spec("plain", false));
    let _cancellable = reg.start(spec("c", true));
    let _resumable = reg.start(resumable("r", "x"));
    let done = reg.start(spec("done", true));
    let done_id = done.id().to_owned();
    done.succeed();
    let kinds: Vec<String> = reg.quit_blockers().into_iter().map(|a| a.kind).collect();
    assert_eq!(kinds, ["c", "r"]);
    assert!(reg.quit_blockers().iter().all(|a| a.id != done_id));
}

#[test]
fn no_running_activity_means_no_quit_blockers() {
    let (reg, _em, _now) = setup();
    assert!(reg.quit_blockers().is_empty());
    let h = reg.start(spec("a", true));
    h.succeed();
    assert!(reg.quit_blockers().is_empty());
}

#[test]
fn interrupted_snapshot_holds_only_running_resumable_activities() {
    let (reg, _em, _now) = setup();
    let h = reg.start(resumable("speech-model", "base"));
    h.progress(40, Some(100));
    let _no_resume = reg.start(spec("other", true));
    let finished = reg.start(resumable("speech-model", "small"));
    finished.succeed();
    let snap = reg.interrupted_snapshot();
    assert_eq!(snap.len(), 1);
    let e = &snap[0];
    assert_eq!((e.kind.as_str(), e.key.as_str()), ("speech-model", "base"));
    assert_eq!((e.done, e.total), (40, Some(100)));
    assert_eq!(e.title_key, "t");
    // The stored form carries exactly these fields: no URL, no secret slot.
    let v = serde_json::to_value(e).unwrap();
    let mut keys: Vec<_> = v.as_object().unwrap().keys().cloned().collect();
    keys.sort();
    assert_eq!(
        keys,
        ["done", "id", "key", "kind", "titleKey", "titleParams", "total"]
    );
}

#[test]
fn activity_event_says_whether_it_is_resumable() {
    let (reg, em, _now) = setup();
    let _h = reg.start(resumable("r", "x"));
    assert_eq!(em.0.lock().unwrap()[0].1["resumable"], true);
}

fn entry(kind: &str) -> InterruptedActivity {
    InterruptedActivity {
        id: "e1".into(),
        kind: kind.into(),
        key: "k".into(),
        title_key: "t".into(),
        title_params: BTreeMap::new(),
        done: 1,
        total: Some(2),
    }
}

#[test]
fn resume_registry_calls_resume_and_discard_and_rejects_unknown_kinds() {
    let reg = ResumeRegistry::default();
    let resumed = Arc::new(AtomicBool::new(false));
    let discarded = Arc::new(AtomicBool::new(false));
    let (r, d) = (Arc::clone(&resumed), Arc::clone(&discarded));
    assert!(reg.register(
        "k1",
        Arc::new(move |_| {
            r.store(true, Ordering::SeqCst);
            Ok(())
        }),
        Some(Arc::new(move |_| d.store(true, Ordering::SeqCst))),
    ));
    // The first registration of a kind wins.
    assert!(!reg.register("k1", Arc::new(|_| Ok(())), None));
    assert!(reg.has("k1") && !reg.has("k2"));
    reg.resume(&entry("k1")).unwrap();
    reg.discard(&entry("k1"));
    assert!(resumed.load(Ordering::SeqCst) && discarded.load(Ordering::SeqCst));
    assert_eq!(
        reg.resume(&entry("k2")).unwrap_err().code.as_deref(),
        Some("activity_resume_unhandled")
    );
    // Discard without a handler is a no-op.
    reg.discard(&entry("k2"));
}
