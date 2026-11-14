//! Spec 0084, A5: Testhilfen, die Tests **beider** Crates brauchen
//! (`app-logic` selbst und `app-shell`, dessen Tests über die Crate-Grenze
//! hinweg dieselben In-Memory-Doubles/Fixtures brauchen) — deshalb hinter
//! `#[cfg(any(test, feature = "test-support"))]` statt reinem
//! `#[cfg(test)]`, sonst wären sie außerhalb dieser Crate unsichtbar.
//! `app-shell` aktiviert das Feature nur unter `[dev-dependencies]` (s.
//! `app-logic/Cargo.toml`); kein Produktivbau aktiviert es.
//!
//! Ursprünglich (vor Spec 0084) ein einziges Modul in `app-shell` für die
//! Spec-0008-Unit-Tests (`groups`, `server_credentials`, `test_connection`)
//! und seither breit wiederverwendet. **Was hier bewusst NICHT liegt**: die
//! Testhilfen für den erhöhten Dateibrowser-Kanal (`BrowserAccess`,
//! `interleave_hook`) — die bleiben in `app-shell::test_support::elevation`
//! (Spec 0084, A1: `BrowserAccess` ist nur in `commands::elevation`
//! konstruierbar; ein Nachbau hier würde genau diese Garantie unterlaufen).
//! `app-shell`s `elevation`-Fixture ruft [`session_with_transport`]
//! trotzdem hier drüben auf (cross-crate) — nicht weil `Session`s Felder
//! privat wären (sie sind `pub`), sondern damit der Bauplan für eine
//! Test-`Session` an einer einzigen Stelle gepflegt wird, statt bei jedem
//! neuen `Session`-Feld auch in `app-shell` nachgezogen werden zu müssen.

use ssh_manager_core::shared::ServerId;

// Issue #909: the in-memory stores live in `ssh_manager_core::testing`; they are
// re-exported so existing call sites keep their import path.
pub use ssh_manager_core::testing::{InMemoryCredentialStore, InMemoryProfileStore, ServerBuilder};

/// Spec 0067: Test-Session mit frei wählbarem Transport und ohne KI — für
/// Tests des erhöhten Dateibrowser-Kanals. Ruft [`session_with_ai_and_
/// transport`] mit einem `AiProvider`, der nur `Done` liefert. Liegt hier
/// (statt in `app-shell`) als der eine kanonische Bauplan für eine
/// Test-`Session` — jedes Feld ist zwar `pub` und ließe sich auch von
/// `app-shell` aus einzeln literal bauen, aber nur an dieser einen Stelle
/// gepflegt zu werden erspart, ihn bei jedem neuen `Session`-Feld an
/// mehreren Stellen nachzuziehen. `app-shell::test_support::elevation::
/// fixture` ruft diese Funktion cross-crate auf.
pub fn session_with_transport(
    transport: Box<dyn ssh_manager_core::ssh::SshTransport>,
) -> crate::session::Session {
    struct NoAi;
    impl ssh_manager_core::ai::AiProvider for NoAi {
        fn send(
            &self,
            _context: ssh_manager_core::ai::RedactedContext,
        ) -> std::pin::Pin<Box<dyn futures::Stream<Item = ssh_manager_core::ai::AiEvent> + Send>>
        {
            Box::pin(futures::stream::iter(vec![
                ssh_manager_core::ai::AiEvent::Done,
            ]))
        }
    }

    session_with_ai_and_transport(NoAi, transport)
}

/// Wie [`session_with_transport`], aber mit frei wählbarem `AiProvider` —
/// für Tests, die eine bestimmte, vom Provider vorgeschlagene Aktion
/// brauchen (z. B. Spec 0084, T5/T5b: KI-/MCP-Dateizugriffe dürfen den
/// erhöhten Kanal nie erreichen, dafür muss die KI erst `ReadRemoteFile`/
/// `WriteRemoteFile` vorschlagen). `app-shell` reicht dafür z. B. seinen
/// eigenen `test_support::MockAiProvider` (Vec-von-Events) herein.
pub fn session_with_ai_and_transport(
    ai_provider: impl ssh_manager_core::ai::AiProvider + 'static,
    transport: Box<dyn ssh_manager_core::ssh::SshTransport>,
) -> crate::session::Session {
    session_with_ai_transport_and_server_id(ai_provider, transport, ServerId::new())
}

/// Like [`session_with_transport`], but with a chosen `ServerId` (e.g. the
/// nil id of the local pseudo-server, issue #974).
pub fn session_with_transport_and_server_id(
    transport: Box<dyn ssh_manager_core::ssh::SshTransport>,
    server_id: ServerId,
) -> crate::session::Session {
    struct NoAi;
    impl ssh_manager_core::ai::AiProvider for NoAi {
        fn send(
            &self,
            _context: ssh_manager_core::ai::RedactedContext,
        ) -> std::pin::Pin<Box<dyn futures::Stream<Item = ssh_manager_core::ai::AiEvent> + Send>>
        {
            Box::pin(futures::stream::iter(vec![
                ssh_manager_core::ai::AiEvent::Done,
            ]))
        }
    }
    session_with_ai_transport_and_server_id(NoAi, transport, server_id)
}

fn session_with_ai_transport_and_server_id(
    ai_provider: impl ssh_manager_core::ai::AiProvider + 'static,
    transport: Box<dyn ssh_manager_core::ssh::SshTransport>,
    server_id: ServerId,
) -> crate::session::Session {
    use std::sync::Arc;
    use tokio::sync::Mutex as AsyncMutex;

    crate::session::Session::new(crate::session::SessionParts {
        transport: crate::session::SessionTransport::new(transport),
        server_id,
        tags: Vec::new(),
        redactor: crate::session::SwappableRedactor::new(Box::new(
            ssh_manager_core::ai::DefaultOutputRedactor::new(),
        )),
        ai: crate::session::AiConfig {
            provider: Box::new(ai_provider),
            provider_budget: Arc::new(ai_providers::ProviderBudgetGuard::new()),
            context: AsyncMutex::new(ssh_manager_core::ai::SessionContext {
                system_context: String::new(),
                history: Vec::new(),
                available_actions: ssh_manager_core::ai::default_action_schemas(),
                max_tokens_hint: None,
            }),
            label: "test-provider".to_string(),
            model: "test-model".to_string(),
            system_context_parts: AsyncMutex::new(crate::compaction::SystemContextParts::default()),
            model_context_window_tokens: usize::MAX / 1_000,
            summary: AsyncMutex::new(None),
            context_usage: Default::default(),
            request_paced_at: AsyncMutex::new(None),
        },
        safety: crate::session::SafetyState {
            filter_engine: Box::new(ssh_manager_core::filter::FilterEngine::new(
                crate::policy::NoRulesPolicyStore,
            )),
            mcp_origin_flags: std::sync::Mutex::new(Vec::new()),
            sudo_password: crate::session::SwappableSudoPassword::new(None),
            mcp_confirmation_claim: std::sync::atomic::AtomicBool::new(false),
            risk_second_opinion_provider: None,
            risk_second_opinion_budget: None,
            // Spec 0092, A1.2: standardmäßig „an", wie in der App.
            red_risk_always_confirm: true,
            outgoing_transform: None,
            untrusted_content_ingested: std::sync::atomic::AtomicBool::new(false),
            post_ingest_policy: ssh_manager_core::profiles::PostIngestPolicy::default(),
            injection_check_provider: None,
            injection_check_budget: None,
            injection_suspected: std::sync::atomic::AtomicBool::new(false),
            injection_suspected_reason: std::sync::Mutex::new(None),
            injection_check_unavailable: std::sync::atomic::AtomicBool::new(false),
            second_opinion_setup_notice_pending: std::sync::atomic::AtomicBool::new(false),
            injection_check_inactive_notice_pending: std::sync::atomic::AtomicBool::new(false),
            redaction_incomplete_notice_pending: std::sync::atomic::AtomicBool::new(false),
        },
        persistence: crate::session::ChatPersistence {
            chat_content: Default::default(),
            title_attempts: Default::default(),
            chat_session_store: None,
            ledger_store: None,
            chat_session_id: AsyncMutex::new(None),
        },
        conn: crate::session::TransportState {
            terminal: std::sync::Mutex::new(None),
            status: std::sync::Mutex::new(crate::events::ConnectionStatus::Connected),
            pending_action: std::sync::Mutex::new(None),
            auto_continue_stop: std::sync::atomic::AtomicBool::new(false),
            auto_continue_stop_notify: tokio::sync::Notify::new(),
            user_decided_in_turn: std::sync::atomic::AtomicBool::new(false),
            chat_turn: std::sync::Mutex::new(crate::session::ChatTurnState::default()),
            running_command_cancellations: Arc::new(
                crate::confirmation::ConfirmationRegistry::new(),
            ),
            auto_reconnect: Default::default(),
        },
    })
}

/// Aufzeichnung der `tracing`-Ereignisse dieses Testbinaries.
///
/// **Warum geteilt und nicht je Testmodul eigen** (Spec 0077, T-6c): Ein
/// globaler `tracing`-Default lässt sich pro Prozess nur **einmal** setzen.
/// Installierte jedes Testmodul seinen eigenen, gewänne nur das zuerst
/// laufende; alle anderen Module schrieben danach in den thread-lokalen
/// Puffer des fremden Subscribers und sähen ihre eigenen Zeilen nie — je
/// nach Testreihenfolge mal so, mal so. Dieselbe Lehre steht in
/// `ai_providers::test_support`. Deshalb: **ein** Subscriber für das ganze
/// Binary, von `orchestration` und `filter_rules` gemeinsam genutzt.
///
/// **Warum global statt `with_default`**: `tracing-core` cacht das
/// Callsite-Interesse prozessweit. Trifft ein Thread ohne Subscriber eine
/// Log-Stelle zuerst, kann sie als „niemand interessiert" gecacht werden,
/// und ein späteres `with_default` auf einem anderen Thread gewinnt dieses
/// Wettrennen nicht zuverlässig zurück (beobachtet in `orchestration`:
/// etwa jeder dritte Lauf verlor den Eintrag).
///
/// **Aufzeichnung auf `TRACE`, Filter beim Lesen** (Spec 0094, §7): Hier
/// stand „Kein Level-Filter" — das war falsch. Ohne `with_max_level` gilt der
/// Standard von `tracing-subscriber`, und der ist `INFO`; `debug`- und
/// `trace`-Ereignisse landeten also nie im Puffer. Spec 0094 braucht beide
/// Richtungen: „auf `info` steht der Inhalt nicht" **und** „auf `debug` steht
/// er". Deshalb jetzt ausdrücklich `TRACE` beim Aufzeichnen und ein Filter
/// beim Lesen — [`recorded_error_lines`] für Spec 0077,
/// [`recorded_lines_at_info_or_above`] und [`recorded_debug_lines`] für
/// Spec 0094.
pub mod log_capture {
    thread_local! {
        /// Je Thread ein eigener Puffer — sicher unter paralleler
        /// Testausführung, da jeder Test-Thread nur seine eigenen Zeilen
        /// sieht.
        static BUFFER: std::cell::RefCell<Vec<u8>> =
            const { std::cell::RefCell::new(Vec::new()) };
    }

    #[derive(Clone, Default)]
    pub struct ThreadLocalTestWriter;

    impl std::io::Write for ThreadLocalTestWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            BUFFER.with(|b| b.borrow_mut().extend_from_slice(buf));
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

    /// Installiert den Subscriber einmal pro Testprozess und leert den
    /// Puffer dieses Threads. Jeder aufzeichnende Test ruft das als Erstes.
    pub fn start_recording() {
        static INIT: std::sync::Once = std::sync::Once::new();
        INIT.call_once(|| {
            let subscriber = tracing_subscriber::fmt()
                .json()
                .with_max_level(tracing::level_filters::LevelFilter::TRACE)
                .with_writer(ThreadLocalTestWriter)
                .finish();
            // `let _ =`: schlägt nur fehl, wenn schon ein globaler Default
            // gesetzt ist — dank `Once` wäre das bereits dieser hier.
            let _ = tracing::subscriber::set_global_default(subscriber);
        });
        BUFFER.with(|b| b.borrow_mut().clear());
    }

    /// Der rohe aufgezeichnete Text dieses Threads.
    pub fn recorded_text() -> String {
        BUFFER.with(|b| String::from_utf8(b.borrow().clone()).expect("Log ist kein UTF-8"))
    }

    /// Nur die Ereignisse auf ERROR-Ebene, eine JSON-Zeile je Ereignis
    /// (Spec 0077, T-6c/T-A10: Aussagen über das ERROR-Log dürfen nicht
    /// versehentlich ein INFO-Ereignis mitlesen).
    pub fn recorded_error_lines() -> Vec<String> {
        lines_at_levels(&["ERROR"])
    }

    /// Spec 0094, A1: die Zeilen, über die A1 eine Aussage macht — `info`,
    /// `warn`, `error`. `debug`/`trace` bleiben draußen, denn dort ist
    /// Inhalt laut A2 ausdrücklich erlaubt.
    pub fn recorded_lines_at_info_or_above() -> Vec<String> {
        lines_at_levels(&["INFO", "WARN", "ERROR"])
    }

    /// Spec 0094, A2: die `debug`-Zeilen, die den Inhalt tragen.
    pub fn recorded_debug_lines() -> Vec<String> {
        lines_at_levels(&["DEBUG"])
    }

    /// Filtert nach dem `level`-Feld der JSON-Zeile. Bewusst wörtlich
    /// gesucht statt die Zeile zu parsen — und mit einer Zusicherung davor,
    /// dass jede aufgezeichnete Zeile ein erkennbares Level trägt: eine
    /// Zeile, die der Filter nicht einordnen kann, würde bei einer
    /// Abwesenheits-Aussage („steht nicht auf `info`") sonst stillschweigend
    /// zu einem falschen Grün führen.
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
        /// Spec 0094, §7: dieselbe Selbstprüfung wie in
        /// `ssh_manager_core::filter::tests` — die Aussagen von T6/T9/T10
        /// sind nur belastbar, wenn der Mitschnitt `debug` und `info`
        /// wirklich sieht und beim Lesen auseinanderhält. Vor Spec 0094
        /// hätte dieser Test die `debug`-Hälfte nicht bestanden.
        #[test]
        fn test_log_capture_records_debug_and_info_separately() {
            super::start_recording();

            tracing::debug!(marker = "nur-debug-0094", "capture self-check (debug)");
            tracing::info!(marker = "nur-info-0094", "capture self-check (info)");

            let debug_lines = super::recorded_debug_lines();
            let info_or_above = super::recorded_lines_at_info_or_above();
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
}

/// Spec 0101, T17: **was als „Schlüssel in einer Ausgabe" zählt** — an
/// einer Stelle, für alle vier Pfade, die §7 nennt (T1, T3, T11, T13).
///
/// Der Grund für ein gemeinsames Modul: T17 ist eine Abwesenheits-Aussage,
/// und die ist nur so gut wie ihre Liste von Suchbegriffen. Vier Kopien
/// davon wären vier Gelegenheiten, in einem Pfad die Base64-Schreibweise
/// oder den `PRAGMA`-Wert zu vergessen — und die Lücke sähe man nicht,
/// weil der Test grün bliebe.
pub mod key_leak_needles {
    use secrecy::ExposeSecret;
    use ssh_manager_core::crypto::DatabaseKey;

    /// Ein Suchbegriff: seine Bezeichnung für die Fehlermeldung und der
    /// Text, der nirgends vorkommen darf.
    pub struct Needle {
        pub what: String,
        pub text: String,
    }

    fn hex_of(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// Jede Schreibweise, in der K oder der daraus abgeleitete
    /// Datenbankschlüssel in eine Zeile geraten könnte, dazu die
    /// Passwörter und Secrets, die der Pfad angefasst hat.
    ///
    /// **Hex in beiden Schreibweisen und Base64**, weil es drei Wege ins
    /// Log gibt: ein `{:02x}`-Format, ein `{:X}`-Format und die Form, in
    /// der K im Schlüsselbund liegt. **Die Byte-Liste aus `{:?}`** ist der
    /// kürzeste Weg von allen: ein versehentliches `#[derive(Debug)]` auf
    /// einem Typ, der K hält.
    pub fn for_root_key(root_key: &[u8; 32], passwords: &[&str], secrets: &[&str]) -> Vec<Needle> {
        use base64::engine::general_purpose::STANDARD as BASE64;
        use base64::Engine;

        let pragma =
            DatabaseKey::from_root_key(&ssh_manager_core::crypto::RootKey::for_tests(*root_key))
                .pragma_value();
        let pragma_value = pragma.expose_secret().to_string();
        let pragma_hex = pragma_value
            .trim_start_matches("x'")
            .trim_end_matches('\'')
            .to_string();

        let mut needles = vec![
            Needle {
                what: "K als Hex".to_string(),
                text: hex_of(root_key),
            },
            Needle {
                what: "K als Hex in Großschreibung".to_string(),
                text: hex_of(root_key).to_uppercase(),
            },
            Needle {
                what: "K als Base64".to_string(),
                text: BASE64.encode(root_key),
            },
            Needle {
                what: "K als Byte-Liste aus {:?}".to_string(),
                text: format!("{root_key:?}"),
            },
            Needle {
                what: "der Datenbankschlüssel als PRAGMA-Wert".to_string(),
                text: pragma_value,
            },
            Needle {
                what: "der Datenbankschlüssel als Hex".to_string(),
                text: pragma_hex.clone(),
            },
            Needle {
                what: "der Datenbankschlüssel als Hex in Großschreibung".to_string(),
                text: pragma_hex.to_uppercase(),
            },
        ];
        for password in passwords {
            needles.push(Needle {
                what: format!("das Passwort „{password}“"),
                text: (*password).to_string(),
            });
        }
        for secret in secrets {
            needles.push(Needle {
                what: format!("das Secret „{secret}“"),
                text: (*secret).to_string(),
            });
        }
        needles
    }

    /// Prüft jeden Suchbegriff gegen jeden benannten Heuhaufen (Log,
    /// Diagnosepaket, …).
    ///
    /// **Mit einer Zusicherung über die Suchbegriffe selbst**: Ein leerer
    /// Begriff wäre in jedem Text enthalten — fiele er versehentlich leer
    /// aus, prüfte `!contains` ab da nichts mehr und wäre trotzdem rot.
    /// Umgekehrt wäre ein Heuhaufen ohne Inhalt eine Abwesenheits-Aussage
    /// über nichts.
    pub fn assert_absent(needles: &[Needle], haystacks: &[(&str, &str)]) {
        assert!(!needles.is_empty(), "keine Suchbegriffe — das prüft nichts");
        for (name, haystack) in haystacks {
            assert!(
                !haystack.is_empty(),
                "T17: „{name}“ ist leer — eine Aussage darüber prüft nichts"
            );
            for needle in needles {
                assert!(
                    !needle.text.is_empty(),
                    "T17: leerer Suchbegriff für {} — der Test prüfte nichts",
                    needle.what
                );
                assert!(
                    !haystack.contains(needle.text.as_str()),
                    "T17: {} steht in „{name}“ (gesucht: {})",
                    needle.what,
                    needle.text
                );
            }
        }
    }
}
