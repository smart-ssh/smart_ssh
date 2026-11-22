//! Read API of the activity record (Spec 0057, issue #1449): every filter,
//! keyset paging while entries are appended, and that chat content is never
//! read.

use chrono::{DateTime, Utc};
use uuid::Uuid;

use ssh_manager_core::audit::{LedgerEntryContent, LedgerSource};

use crate::{
    EntryFilter, PageRequest, RecordCursor, RecordedRow, SessionFilter, SqliteProfileStore,
};

fn sid(n: u128) -> Uuid {
    Uuid::from_u128(n)
}

fn ts(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
}

async fn store() -> SqliteProfileStore {
    SqliteProfileStore::connect_with(sqlx::sqlite::SqliteConnectOptions::new().filename(":memory:"))
        .await
        .unwrap()
}

async fn add_session(
    store: &SqliteProfileStore,
    id: u128,
    kind: &str,
    server: &str,
    groups: &str,
    started: &str,
    failed: i64,
) {
    sqlx::query(
        "INSERT INTO activity_sessions (id, kind, server_id, server_name, server_host, \
         server_port, server_user, group_ids, started_at, failed_writes) \
         VALUES (?, ?, ?, 'n', 'h', 22, 'u', ?, ?, ?)",
    )
    .bind(sid(id).to_string())
    .bind(kind)
    .bind(server)
    .bind(groups)
    .bind(started)
    .bind(failed)
    .execute(&store.pool)
    .await
    .unwrap();
}

async fn add_entry(
    store: &SqliteProfileStore,
    session: u128,
    seq: i64,
    source: &str,
    entry_type: &str,
    content: &str,
    created: &str,
) {
    sqlx::query(
        "INSERT INTO ledger_entries (id, session_id, sequence, source, entry_type, content, \
         created_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(Uuid::new_v4().to_string())
    .bind(sid(session).to_string())
    .bind(seq)
    .bind(source)
    .bind(entry_type)
    .bind(content)
    .bind(created)
    .execute(&store.pool)
    .await
    .unwrap();
}

const ALL: PageRequest = PageRequest {
    limit: 200,
    after: None,
};

#[tokio::test]
async fn test_session_filters_narrow_each_and_combined() {
    let s = store().await;
    add_session(
        &s,
        1,
        "chat",
        "srv-a",
        r#"["g1","g2"]"#,
        "2026-01-01T00:00:00+00:00",
        0,
    )
    .await;
    add_session(
        &s,
        2,
        "mcp",
        "srv-a",
        r#"["g1"]"#,
        "2026-02-01T00:00:00+00:00",
        2,
    )
    .await;
    add_session(
        &s,
        3,
        "terminal",
        "srv-b",
        r#"["g3"]"#,
        "2026-03-01T00:00:00+00:00",
        0,
    )
    .await;
    let q = s.ledger_store().record_query();
    let ids = |f: SessionFilter| {
        let q = q.clone();
        async move {
            q.list_sessions(&f, ALL)
                .await
                .unwrap()
                .items
                .into_iter()
                .map(|x| x.id)
                .collect::<Vec<_>>()
        }
    };
    assert_eq!(
        ids(SessionFilter::default()).await,
        vec![sid(1), sid(2), sid(3)]
    );
    assert_eq!(
        ids(SessionFilter {
            started_from: Some(ts("2026-02-01T00:00:00+00:00")),
            started_until: Some(ts("2026-03-01T00:00:00+00:00")),
            ..Default::default()
        })
        .await,
        vec![sid(2)]
    );
    assert_eq!(
        ids(SessionFilter {
            server_ids: vec!["srv-b".into()],
            ..Default::default()
        })
        .await,
        vec![sid(3)]
    );
    // Group g1 matches the session of g1 and the one of its subgroup g2.
    assert_eq!(
        ids(SessionFilter {
            group_id: Some("g1".into()),
            ..Default::default()
        })
        .await,
        vec![sid(1), sid(2)]
    );
    assert_eq!(
        ids(SessionFilter {
            group_id: Some("g2".into()),
            ..Default::default()
        })
        .await,
        vec![sid(1)]
    );
    assert_eq!(
        ids(SessionFilter {
            kinds: vec!["mcp".into(), "terminal".into()],
            ..Default::default()
        })
        .await,
        vec![sid(2), sid(3)]
    );
    assert_eq!(
        ids(SessionFilter {
            has_failed_writes: true,
            ..Default::default()
        })
        .await,
        vec![sid(2)]
    );
    assert_eq!(
        ids(SessionFilter {
            server_ids: vec!["srv-a".into()],
            group_id: Some("g1".into()),
            kinds: vec!["chat".into()],
            ..Default::default()
        })
        .await,
        vec![sid(1)]
    );
}

async fn filled() -> SqliteProfileStore {
    let s = store().await;
    add_session(
        &s,
        1,
        "chat",
        "srv-a",
        r#"["g1","g2"]"#,
        "2026-01-01T00:00:00+00:00",
        0,
    )
    .await;
    add_session(
        &s,
        2,
        "mcp",
        "srv-b",
        r#"["g3"]"#,
        "2026-01-01T00:00:00+00:00",
        0,
    )
    .await;
    let t = "2026-01-02T00:00:00+00:00";
    add_entry(
        &s,
        1,
        0,
        "ai",
        "command_proposed",
        r#"{"CommandProposed":{"command":"Uptime -p"}}"#,
        "2026-01-01T10:00:00+00:00",
    )
    .await;
    add_entry(&s, 1, 1, "user", "decision", r#"{"Decision":{"outcome":"Confirmed","reason":null,"code":null,"matched_rule":null,"matched_rule_origin":null}}"#, "2026-01-01T11:00:00+00:00").await;
    add_entry(&s, 1, 2, "user", "command_executed", r#"{"CommandExecuted":{"command":"false","output":{"stdout":[],"stderr":[],"exit_code":1},"cancelled":false}}"#, "2026-01-01T12:00:00+00:00").await;
    add_entry(
        &s,
        1,
        3,
        "ai",
        "ai_message",
        r#"{"AiMessage":{"text":"The UPTIME is fine"}}"#,
        "2026-01-01T13:00:00+00:00",
    )
    .await;
    add_entry(
        &s,
        1,
        4,
        "ai",
        "file_read",
        r#"{"FileRead":{"path":"/etc/uptime-notes","bytes":3}}"#,
        t,
    )
    .await;
    add_entry(&s, 2, 0, "mcp-agent", "decision", r#"{"Decision":{"outcome":"Rejected","reason":"x","code":null,"matched_rule":null,"matched_rule_origin":null}}"#, t).await;
    add_entry(&s, 2, 1, "mcp-agent", "command_executed", r#"{"CommandExecuted":{"command":"true","output":{"stdout":[],"stderr":[],"exit_code":0},"cancelled":false}}"#, t).await;
    add_entry(&s, 2, 2, "mcp-agent", "command_executed", r#"{"CommandExecuted":{"command":"sleep 9","output":{"stdout":[],"stderr":[],"exit_code":null},"cancelled":true}}"#, t).await;
    s
}

async fn seqs(s: &SqliteProfileStore, f: EntryFilter) -> Vec<(u128, i64)> {
    s.ledger_store()
        .record_query()
        .list_entries(&f, ALL)
        .await
        .unwrap()
        .items
        .into_iter()
        .map(|e| (e.session_id().unwrap().as_u128(), e.sequence()))
        .collect()
}

#[tokio::test]
async fn test_entry_filters_narrow_each_and_combined() {
    let s = filled().await;
    assert_eq!(seqs(&s, EntryFilter::default()).await.len(), 8);
    // time range: [from, until)
    assert_eq!(
        seqs(
            &s,
            EntryFilter {
                from: Some(ts("2026-01-01T11:00:00+00:00")),
                until: Some(ts("2026-01-01T13:00:00+00:00")),
                ..Default::default()
            }
        )
        .await,
        vec![(1, 1), (1, 2)]
    );
    assert_eq!(
        seqs(
            &s,
            EntryFilter {
                session_ids: vec![sid(2)],
                ..Default::default()
            }
        )
        .await
        .len(),
        3
    );
    assert_eq!(
        seqs(
            &s,
            EntryFilter {
                server_ids: vec!["srv-a".into()],
                ..Default::default()
            }
        )
        .await
        .len(),
        5
    );
    assert_eq!(
        seqs(
            &s,
            EntryFilter {
                group_id: Some("g2".into()),
                ..Default::default()
            }
        )
        .await
        .len(),
        5
    );
    assert_eq!(
        seqs(
            &s,
            EntryFilter {
                group_id: Some("g1".into()),
                ..Default::default()
            }
        )
        .await
        .len(),
        5
    );
    assert_eq!(
        seqs(
            &s,
            EntryFilter {
                sources: vec!["mcp-agent".into()],
                ..Default::default()
            }
        )
        .await
        .len(),
        3
    );
    assert_eq!(
        seqs(
            &s,
            EntryFilter {
                entry_types: vec!["decision".into()],
                ..Default::default()
            }
        )
        .await,
        vec![(1, 1), (2, 0)]
    );
    assert_eq!(
        seqs(
            &s,
            EntryFilter {
                decision_outcomes: vec!["Rejected".into()],
                ..Default::default()
            }
        )
        .await,
        vec![(2, 0)]
    );
    // exit code != 0: exit 1 and the unknown one, not exit 0, not other types.
    assert_eq!(
        seqs(
            &s,
            EntryFilter {
                exit_code_nonzero: true,
                ..Default::default()
            }
        )
        .await,
        vec![(1, 2), (2, 2)]
    );
    // text: case-insensitive over command text and AI text, not over paths.
    assert_eq!(
        seqs(
            &s,
            EntryFilter {
                text_contains: Some("uptime".into()),
                ..Default::default()
            }
        )
        .await,
        vec![(1, 0), (1, 3)]
    );
    // combined
    assert_eq!(
        seqs(
            &s,
            EntryFilter {
                server_ids: vec!["srv-a".into()],
                sources: vec!["ai".into()],
                text_contains: Some("UPTIME".into()),
                entry_types: vec!["ai_message".into()],
                ..Default::default()
            }
        )
        .await,
        vec![(1, 3)]
    );
}

#[tokio::test]
async fn test_entries_of_session_in_sequence_order() {
    let s = filled().await;
    let rows = s
        .ledger_store()
        .record_query()
        .entries_of_session(sid(1))
        .await
        .unwrap();
    let seq: Vec<i64> = rows
        .iter()
        .map(|r| match r {
            RecordedRow::Entry(e) => e.sequence,
            RecordedRow::Unreadable(u) => u.sequence,
        })
        .collect();
    assert_eq!(seq, vec![0, 1, 2, 3, 4]);
}

#[tokio::test]
async fn test_paging_returns_every_entry_once_while_entries_are_appended() {
    let s = store().await;
    add_session(&s, 1, "chat", "srv-a", "[]", "2026-01-01T00:00:00+00:00", 0).await;
    let ledger = s.ledger_store();
    let content = |i: usize| LedgerEntryContent::CommandProposed {
        command: format!("c{i}"),
    };
    for i in 0..10 {
        ledger
            .append_entry(sid(1), LedgerSource::Ai, &content(i))
            .await
            .unwrap();
    }
    let q = ledger.record_query();
    let mut seen = Vec::new();
    let mut after: Option<RecordCursor> = None;
    let mut appended = 10;
    loop {
        let page = q
            .list_entries(&EntryFilter::default(), PageRequest { limit: 3, after })
            .await
            .unwrap();
        seen.extend(page.items.iter().map(|e| e.sequence()));
        // Concurrent writer: one more entry between every two pages.
        ledger
            .append_entry(sid(1), LedgerSource::Ai, &content(appended))
            .await
            .unwrap();
        appended += 1;
        match page.next {
            Some(c) => after = Some(c),
            None => break,
        }
    }
    let mut dedup = seen.clone();
    dedup.sort();
    dedup.dedup();
    assert_eq!(dedup.len(), seen.len(), "no entry twice: {seen:?}");
    // Everything that existed when the walk began is there; nothing skipped.
    assert_eq!(seen, (0..seen.len() as i64).collect::<Vec<_>>());
    assert!(seen.len() >= 10);
}

#[tokio::test]
async fn test_page_size_is_clamped_to_200() {
    let s = store().await;
    add_session(&s, 1, "chat", "srv-a", "[]", "2026-01-01T00:00:00+00:00", 0).await;
    sqlx::query(
        "WITH RECURSIVE n(i) AS (SELECT 0 UNION ALL SELECT i + 1 FROM n WHERE i < 249) \
         INSERT INTO ledger_entries (id, session_id, sequence, source, entry_type, content, created_at) \
         SELECT 'e' || i, ?, i, 'ai', 'activity_tick', '\"ActivityTick\"', '2026-01-01T00:00:00+00:00' FROM n",
    )
    .bind(sid(1).to_string())
    .execute(&s.pool)
    .await
    .unwrap();
    let q = s.ledger_store().record_query();
    let page = q
        .list_entries(
            &EntryFilter::default(),
            PageRequest {
                limit: 10_000,
                after: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(page.items.len(), 200);
    assert!(page.next.is_some());
    let page = q
        .list_entries(
            &EntryFilter::default(),
            PageRequest {
                limit: 0,
                after: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(page.items.len(), 1);
}

#[tokio::test]
async fn test_chat_messages_are_never_read() {
    let s = store().await;
    let marker = "UNREDACTED-CHAT-MARKER";
    add_session(&s, 1, "chat", "srv-a", "[]", "2026-01-01T00:00:00+00:00", 0).await;
    add_entry(
        &s,
        1,
        0,
        "ai",
        "command_proposed",
        r#"{"CommandProposed":{"command":"ls"}}"#,
        "2026-01-01T00:00:00+00:00",
    )
    .await;
    sqlx::query("INSERT INTO servers (id,name,host,port,username,auth_method,created_at,updated_at) VALUES ('srv-a','n','h',22,'u','\"Agent\"','t','t')")
        .execute(&s.pool).await.unwrap();
    sqlx::query("INSERT INTO chat_sessions (id,server_id,started_at) VALUES (?, 'srv-a', 't')")
        .bind(sid(1).to_string())
        .execute(&s.pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO chat_messages (id,session_id,role,content_type,content,sequence,created_at) VALUES ('m1', ?, 'user', 'text', ?, 0, 't')")
        .bind(sid(1).to_string())
        .bind(format!("\"{marker}\""))
        .execute(&s.pool).await.unwrap();
    let q = s.ledger_store().record_query();
    let entries = q.list_entries(&EntryFilter::default(), ALL).await.unwrap();
    let sessions = q
        .list_sessions(&SessionFilter::default(), ALL)
        .await
        .unwrap();
    let of_session = q.entries_of_session(sid(1)).await.unwrap();
    let dump = format!("{entries:?}{sessions:?}{of_session:?}");
    assert!(!dump.contains(marker));
    let hit = q
        .list_entries(
            &EntryFilter {
                text_contains: Some(marker.into()),
                ..Default::default()
            },
            ALL,
        )
        .await
        .unwrap();
    assert!(hit.items.is_empty());
}

const GOOD: &str = r#"{"CommandProposed":{"command":"ls"}}"#;
const TS: &str = "2026-01-01T00:00:00+00:00";

/// Session 1 with entries 0..=4; the entry at sequence 2 is damaged by `bad`.
async fn with_damaged(bad: &str) -> SqliteProfileStore {
    let s = store().await;
    add_session(&s, 1, "chat", "srv-a", "[]", TS, 0).await;
    for seq in 0..5 {
        add_entry(&s, 1, seq, "ai", "command_proposed", GOOD, TS).await;
    }
    // Rows are damaged after the fact, as a broken file would be.
    sqlx::query("PRAGMA ignore_check_constraints = ON")
        .execute(&s.pool)
        .await
        .unwrap();
    let sql = match bad {
        "content" => "UPDATE ledger_entries SET content = 'not json' WHERE sequence = 2",
        "timestamp" => "UPDATE ledger_entries SET created_at = 'yesterday' WHERE sequence = 2",
        "snapshot" => "UPDATE ledger_entries SET rule_snapshot = '{broken' WHERE sequence = 2",
        "source" => "UPDATE ledger_entries SET source = 'robot' WHERE sequence = 2",
        _ => unreachable!(),
    };
    sqlx::query(sql).execute(&s.pool).await.unwrap();
    s
}

fn describe(rows: &[RecordedRow]) -> Vec<(i64, bool)> {
    rows.iter()
        .map(|r| match r {
            RecordedRow::Entry(e) => (e.sequence, true),
            RecordedRow::Unreadable(u) => (u.sequence, false),
        })
        .collect()
}

/// Issue #1496: a damaged row is marked per item, in a page and in a session
/// read, for every kind of damage; the cursor continues past it.
#[tokio::test]
async fn test_damaged_row_is_marked_not_fatal_for_every_kind_of_damage() {
    for bad in ["content", "timestamp", "snapshot", "source"] {
        let s = with_damaged(bad).await;
        let q = s.ledger_store().record_query();
        let expect = vec![(0, true), (1, true), (2, false), (3, true), (4, true)];
        let all = q.entries_of_session(sid(1)).await.unwrap();
        assert_eq!(describe(&all), expect, "session read, {bad}");
        let RecordedRow::Unreadable(u) = &all[2] else {
            unreachable!()
        };
        assert_eq!(u.session_id, Some(sid(1)));
        assert!(!u.reason.is_empty() && !u.id.is_empty());
        // Page of 2: [0,1] / [2(damaged),3] / [4]; the cursor walks past it.
        let (mut seen, mut after) = (Vec::new(), None);
        loop {
            let p = q
                .list_entries(&EntryFilter::default(), PageRequest { limit: 2, after })
                .await
                .unwrap();
            seen.extend(describe(&p.items));
            match p.next {
                Some(c) => after = Some(c),
                None => break,
            }
        }
        assert_eq!(seen, expect, "paged read, {bad}");
    }
}

/// Issue #1496: content filters over a row with invalid JSON neither fail nor
/// stop the keyset walk; the damaged row matches none of them.
#[tokio::test]
async fn test_content_filters_survive_a_row_with_invalid_content() {
    let s = with_damaged("content").await;
    let q = s.ledger_store().record_query();
    let filters = [
        EntryFilter {
            text_contains: Some("ls".to_owned()),
            ..Default::default()
        },
        EntryFilter {
            decision_outcomes: vec!["allowed".to_owned()],
            ..Default::default()
        },
        EntryFilter {
            exit_code_nonzero: true,
            ..Default::default()
        },
    ];
    for (i, f) in filters.iter().enumerate() {
        let (mut seen, mut after) = (Vec::new(), None);
        loop {
            let p = q
                .list_entries(f, PageRequest { limit: 1, after })
                .await
                .unwrap();
            seen.extend(describe(&p.items));
            match p.next {
                Some(c) => after = Some(c),
                None => break,
            }
        }
        let want: Vec<(i64, bool)> = if i == 0 {
            vec![(0, true), (1, true), (3, true), (4, true)]
        } else {
            vec![]
        };
        assert_eq!(seen, want, "filter {i}");
    }
}

/// Issue #1496: an unknown `source` is damage, not an AI entry.
#[tokio::test]
async fn test_unknown_source_reason_is_reported() {
    let s = with_damaged("source").await;
    let all = s
        .ledger_store()
        .record_query()
        .entries_of_session(sid(1))
        .await
        .unwrap();
    let RecordedRow::Unreadable(u) = &all[2] else {
        panic!("must not be mapped to an entry")
    };
    assert!(u.reason.contains("robot"), "{}", u.reason);
}

/// Issue #1496: an entry whose stored session id is not a UUID is marked.
#[tokio::test]
async fn test_invalid_session_id_is_marked() {
    let s = store().await;
    sqlx::query(
        "INSERT INTO activity_sessions (id, kind, server_id, server_name, server_host, \
         server_port, server_user, group_ids, started_at, failed_writes) \
         VALUES ('not-a-uuid', 'chat', 'srv-a', 'n', 'h', 22, 'u', '[]', ?, 0)",
    )
    .bind(TS)
    .execute(&s.pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO ledger_entries (id, session_id, sequence, source, entry_type, content, \
         created_at) VALUES ('e1', 'not-a-uuid', 7, 'ai', 'command_proposed', ?, ?)",
    )
    .bind(GOOD)
    .bind(TS)
    .execute(&s.pool)
    .await
    .unwrap();
    let p = s
        .ledger_store()
        .record_query()
        .list_entries(&EntryFilter::default(), ALL)
        .await
        .unwrap();
    let [RecordedRow::Unreadable(u)] = p.items.as_slice() else {
        panic!("{:?}", p.items)
    };
    assert_eq!((u.id.as_str(), u.sequence, u.session_id), ("e1", 7, None));
}

/// Issue #1496: the shared row mapping gives `load_entries` and the read API
/// the same entries, and both reject the same damage.
#[tokio::test]
async fn test_load_entries_and_read_api_map_rows_alike() {
    let s = with_damaged("timestamp").await;
    let ledger = s.ledger_store();
    assert!(ledger.load_entries(sid(1)).await.is_err());
    sqlx::query("UPDATE ledger_entries SET created_at = ? WHERE sequence = 2")
        .bind(TS)
        .execute(&s.pool)
        .await
        .unwrap();
    let loaded = ledger.load_entries(sid(1)).await.unwrap();
    let read = ledger
        .record_query()
        .entries_of_session(sid(1))
        .await
        .unwrap();
    assert_eq!(loaded.len(), read.len());
    for (l, r) in loaded.iter().zip(&read) {
        let RecordedRow::Entry(e) = r else {
            panic!("{r:?}")
        };
        assert_eq!(l, &e.entry);
    }
    sqlx::query("UPDATE ledger_entries SET source = 'robot' WHERE sequence = 2")
        .execute(&s.pool)
        .await
        .unwrap();
    assert!(ledger.load_entries(sid(1)).await.is_err());
}

/// More entries than one chunk: all returned, in order, none twice.
#[tokio::test]
async fn test_entries_of_session_spans_several_chunks() {
    let s = store().await;
    add_session(&s, 1, "chat", "srv-a", "[]", TS, 0).await;
    sqlx::query(
        "WITH RECURSIVE n(i) AS (SELECT 0 UNION ALL SELECT i + 1 FROM n WHERE i < 2499) \
         INSERT INTO ledger_entries (id, session_id, sequence, source, entry_type, content, created_at) \
         SELECT 'e' || i, ?, i, 'ai', 'activity_tick', '\"ActivityTick\"', ? FROM n",
    )
    .bind(sid(1).to_string())
    .bind(TS)
    .execute(&s.pool)
    .await
    .unwrap();
    let rows = s
        .ledger_store()
        .record_query()
        .entries_of_session(sid(1))
        .await
        .unwrap();
    let seq: Vec<i64> = describe(&rows).iter().map(|r| r.0).collect();
    assert_eq!(seq, (0..2500).collect::<Vec<_>>());
}

/// Benchmark (issue #1449): 100k entries, a page of 200 under 50 ms, an
/// append during a query not blocked for more than 50 ms.
/// Run with `cargo test -p persistence-sqlite --release -- --ignored record_query_bench`.
#[tokio::test]
#[ignore = "benchmark, run explicitly"]
async fn record_query_bench_page_of_200_in_100k_entries() {
    let s = store().await;
    add_session(
        &s,
        1,
        "chat",
        "srv-a",
        r#"["g1"]"#,
        "2026-01-01T00:00:00+00:00",
        0,
    )
    .await;
    sqlx::query(
        "WITH RECURSIVE n(i) AS (SELECT 0 UNION ALL SELECT i + 1 FROM n WHERE i < 99999) \
         INSERT INTO ledger_entries (id, session_id, sequence, source, entry_type, content, created_at) \
         SELECT 'e' || i, ?, i, 'ai', 'command_proposed', '{\"CommandProposed\":{\"command\":\"ls\"}}', '2026-01-01T00:00:00+00:00' FROM n",
    )
    .bind(sid(1).to_string())
    .execute(&s.pool)
    .await
    .unwrap();
    let q = s.ledger_store().record_query();
    let filter = EntryFilter {
        server_ids: vec!["srv-a".into()],
        ..Default::default()
    };
    for after in [None, Some(RecordCursor(50_000))] {
        let start = std::time::Instant::now();
        let page = q
            .list_entries(&filter, PageRequest { limit: 200, after })
            .await
            .unwrap();
        let took = start.elapsed();
        assert_eq!(page.items.len(), 200);
        assert!(took.as_millis() < 50, "page took {took:?}");
    }
    // An append that starts while a query runs.
    let ledger = s.ledger_store();
    let reader = {
        let q = q.clone();
        tokio::spawn(async move {
            // Includes a no-match text filter, which scans all 100k entries
            // in bounded chunks so appends can interleave.
            let no_match = EntryFilter {
                text_contains: Some("übersicht".into()),
                ..Default::default()
            };
            for _ in 0..5 {
                let none = q.list_entries(&no_match, ALL).await.unwrap();
                assert!(none.items.is_empty());
                assert!(none.next.is_some());
            }
            for _ in 0..20 {
                q.list_entries(&EntryFilter::default(), ALL).await.unwrap();
            }
        })
    };
    // Text-filtered page: no match and a selective match (Unicode needle).
    sqlx::query(
        "UPDATE ledger_entries SET content = '{\"CommandProposed\":{\"command\":\"Übersicht\"}}' \
         WHERE sequence = 90000",
    )
    .execute(&s.pool)
    .await
    .unwrap();
    for (needle, expected) in [("zzz-no-match", 0), ("ÜBERSICHT", 1)] {
        let f = EntryFilter {
            text_contains: Some(needle.into()),
            ..Default::default()
        };
        let start = std::time::Instant::now();
        let page = q
            .list_entries(
                &f,
                PageRequest {
                    limit: 200,
                    after: None,
                },
            )
            .await
            .unwrap();
        let took = start.elapsed();
        assert!(page.items.len() <= expected);
        assert!(took.as_millis() < 50, "text page {needle} took {took:?}");
        // Paging on `next` reaches the whole record.
        let (mut found, mut after) = (page.items.len(), page.next);
        while let Some(c) = after {
            let p = q
                .list_entries(
                    &f,
                    PageRequest {
                        limit: 200,
                        after: Some(c),
                    },
                )
                .await
                .unwrap();
            found += p.items.len();
            after = p.next;
        }
        assert_eq!(found, expected);
    }
    // Issue #1496: the whole session in chunks, appends interleave.
    let session_reader = {
        let q = q.clone();
        tokio::spawn(async move {
            let rows = q.entries_of_session(sid(1)).await.unwrap();
            assert!(rows.len() >= 100_000);
            let seq: Vec<i64> = rows
                .iter()
                .map(|r| match r {
                    RecordedRow::Entry(e) => e.sequence,
                    RecordedRow::Unreadable(u) => u.sequence,
                })
                .collect();
            assert!(seq.windows(2).all(|w| w[0] < w[1]), "sequence order");
        })
    };
    let mut worst = std::time::Duration::ZERO;
    for _ in 0..20 {
        let start = std::time::Instant::now();
        ledger
            .append_entry(sid(1), LedgerSource::Ai, &LedgerEntryContent::ActivityTick)
            .await
            .unwrap();
        worst = worst.max(start.elapsed());
    }
    reader.await.unwrap();
    session_reader.await.unwrap();
    assert!(worst.as_millis() < 50, "append blocked for {worst:?}");
}

/// Review of #1449: the text filter folds non-ASCII case too.
#[tokio::test]
async fn test_text_filter_is_unicode_case_insensitive() {
    let s = store().await;
    add_session(&s, 1, "chat", "srv-a", "[]", "2026-01-01T00:00:00+00:00", 0).await;
    add_entry(
        &s,
        1,
        0,
        "ai",
        "ai_message",
        r#"{"AiMessage":{"text":"Übersicht der Dienste"}}"#,
        "2026-01-01T14:00:00+00:00",
    )
    .await;
    let q = s.ledger_store().record_query();
    let page = q
        .list_entries(
            &EntryFilter {
                text_contains: Some("übersicht".into()),
                ..Default::default()
            },
            ALL,
        )
        .await
        .unwrap();
    assert_eq!(page.items.len(), 1);
}

/// Review of #1449: lower-case stored text, upper-case needle.
#[tokio::test]
async fn test_text_filter_upper_needle_matches_lower_stored_text() {
    let s = store().await;
    add_session(&s, 1, "chat", "srv-a", "[]", "2026-01-01T00:00:00+00:00", 0).await;
    add_entry(
        &s,
        1,
        0,
        "ai",
        "ai_message",
        r#"{"AiMessage":{"text":"übersicht der dienste"}}"#,
        "2026-01-01T14:00:00+00:00",
    )
    .await;
    let q = s.ledger_store().record_query();
    let page = q
        .list_entries(
            &EntryFilter {
                text_contains: Some("ÜBERSICHT".into()),
                ..Default::default()
            },
            ALL,
        )
        .await
        .unwrap();
    assert_eq!(page.items.len(), 1);
}

/// A text filter over more rows than the scan budget returns a cursor, and
/// following it finds a match beyond the budget.
#[tokio::test]
async fn test_text_filter_scan_budget_cursor_continues_to_later_match() {
    let s = store().await;
    add_session(&s, 1, "chat", "srv-a", "[]", "2026-01-01T00:00:00+00:00", 0).await;
    sqlx::query(
        "WITH RECURSIVE n(i) AS (SELECT 0 UNION ALL SELECT i + 1 FROM n WHERE i < 5999) \
         INSERT INTO ledger_entries (id, session_id, sequence, source, entry_type, content, created_at) \
         SELECT 'e' || i, ?, i, 'ai', 'command_proposed', \
         CASE WHEN i = 5999 THEN '{\"CommandProposed\":{\"command\":\"Needle\"}}' \
         ELSE '{\"CommandProposed\":{\"command\":\"ls\"}}' END, \
         '2026-01-01T00:00:00+00:00' FROM n",
    )
    .bind(sid(1).to_string())
    .execute(&s.pool)
    .await
    .unwrap();
    let q = s.ledger_store().record_query();
    let f = EntryFilter {
        text_contains: Some("NEEDLE".into()),
        ..Default::default()
    };
    let first = q.list_entries(&f, ALL).await.unwrap();
    assert!(first.items.is_empty());
    let cursor = first.next.expect("budget reached, cursor expected");
    let second = q
        .list_entries(
            &f,
            PageRequest {
                limit: 200,
                after: Some(cursor),
            },
        )
        .await
        .unwrap();
    assert_eq!(second.items.len(), 1);
    assert!(second.next.is_none());
}
