//! Spec 0092: Ein rot eingestufter Vorschlag verlangt eine Bestätigung,
//! solange die app-weite Einstellung an ist — auch gegen eine Allow-Regel.
//!
//! **Alle Fälle hier laufen mit einer Allow-Regel**, die das Kommando sonst
//! automatisch ausführen ließe (Spec 0092, §7, Vorbemerkung) — sonst
//! landete die Filter-Engine ohnehin auf `Confirm` und der Test bewiese
//! nichts über das neue Glied.

use async_trait::async_trait;
use ssh_manager_core::ai::AiEvent;
use ssh_manager_core::filter::{
    EffectiveScope, EvalContext, FilterEngine, Pattern, PolicyStore, Rule, RuleAction, Scope,
};

use crate::events::TestEmitter;

use super::super::test_support::*;
use super::*;

/// Erlaubt genau `iptables -F` — für Spec 0092, T8b: eine Allow-Regel auf
/// die Form **ohne** `sudo`, die per Dual-Text-Matching (ADR 0002) auch
/// `sudo iptables -F` abdeckt.
struct AllowExactIptablesFlushStore;
#[async_trait]
impl PolicyStore for AllowExactIptablesFlushStore {
    async fn rules_for(&self, _scope: &EffectiveScope) -> Vec<Rule> {
        vec![Rule {
            id: RuleId("allow-iptables-flush".to_string()),
            pattern: Pattern::Exact("iptables -F".to_string()),
            action: RuleAction::Allow,
            scope: Scope::Global,
            priority: 0,
            origin: RuleOrigin::User,
        }]
    }
}

/// Verbietet `iptables …` — für Spec 0092, T5.
struct DenyIptablesStore;
#[async_trait]
impl PolicyStore for DenyIptablesStore {
    async fn rules_for(&self, _scope: &EffectiveScope) -> Vec<Rule> {
        vec![Rule {
            id: RuleId("deny-iptables".to_string()),
            pattern: Pattern::Glob("iptables*".to_string()),
            action: RuleAction::Deny,
            scope: Scope::Global,
            priority: 10,
            origin: RuleOrigin::User,
        }]
    }
}

/// Die in diesem Modul verwendeten Beispiele, je mit der Achse, auf der sie
/// rot sind.
const SERVER_RED: &str = "iptables -F";
const DATA_RED: &str = "printenv";
/// Rot auf der Daten-Achse (Muster `credentials`), aber **kein** Secret-Pfad
/// (`\bcredentials\b` greift vor `_` nicht) — Spec 0092, T9.
const DATA_RED_READ_PATH: &str = "/srv/app/credentials_backup.txt";
/// Nur Gelb — Spec 0092, T4.
const YELLOW_ONLY: &str = "systemctl restart nginx";

fn confirm_reason(payload: &serde_json::Value) -> &str {
    payload["decision"]["Confirm"]["reason"]
        .as_str()
        .unwrap_or("")
}

/// Eine Sitzung mit Allow-für-alles und eingeschalteter Einstellung.
fn red_risk_session(transport: MockSshTransport) -> Session {
    let mut session = test_session(vec![AiEvent::Done], transport);
    session.parts_mut_for_tests().filter_engine =
        Box::new(FilterEngine::new(AllowEverythingPolicyStore));
    assert!(
        session.red_risk_always_confirm,
        "Spec 0092, §5: Test-Fixtures starten mit eingeschalteter Einstellung"
    );
    session
}

// --- Auswahl der roten Beispiele (Spec 0092, §7, Vorbemerkung) ----------

/// Die in diesem Modul benutzten Beispiele werden **vom Klassifizierer
/// ermittelt, nicht per Hand behauptet**: Dieser Test hält fest, welche
/// Achse rot ist, und dass sie weder an der Hard-Blacklist noch an
/// Secret-Pfad oder `sftp-server` hängen (sonst prüften die Tests unten ein
/// anderes Glied der Kette).
#[tokio::test]
async fn test_chosen_examples_are_red_without_any_other_escalation_reason() {
    let cases: [(&str, RiskLevel, RiskLevel); 4] = [
        (SERVER_RED, RiskLevel::Red, RiskLevel::None),
        (DATA_RED, RiskLevel::None, RiskLevel::Red),
        (
            &sftp_read_pseudo_command(DATA_RED_READ_PATH),
            RiskLevel::None,
            RiskLevel::Red,
        ),
        (YELLOW_ONLY, RiskLevel::Yellow, RiskLevel::None),
    ];
    let engine = FilterEngine::new(AllowEverythingPolicyStore);
    let ctx = EvalContext {
        server_id: ssh_manager_core::shared::ServerId::new(),
        tags: Vec::new(),
    };
    for (command, server_risk, data_risk) in cases {
        let assessment = RuleBasedRiskClassifier.classify(command);
        assert_eq!(assessment.server_risk, server_risk, "{command}");
        assert_eq!(assessment.data_risk, data_risk, "{command}");
        assert!(
            ssh_manager_core::risk::secret_path_read_reason(command).is_none(),
            "{command} darf kein Secret-Pfad sein, sonst greift ein anderes Glied"
        );
        assert!(
            ssh_manager_core::risk::sftp_server_invocation_reason(command).is_none(),
            "{command} darf kein sftp-server-Aufruf sein"
        );
        // Beweist zugleich, dass die Hard-Blacklist nicht greift — sie wäre
        // hier als `Confirm { FILTER_HARD_BLACKLIST }` sichtbar.
        assert!(
            matches!(engine.evaluate(command, &ctx).await, Decision::AutoExec),
            "{command} muss mit Allow-Regel auf AutoExec landen, sonst prüft der Test nichts"
        );
    }
}

// --- T1/T2: je eine rote Achse ----------------------------------------

/// Spec 0092, T1: Server-Rot mit Allow-Regel und eingeschalteter
/// Einstellung → Bestätigung mit dem neuen Code, keine Ausführung.
#[tokio::test]
async fn test_t1_server_red_requires_confirmation_despite_allow_rule() {
    let session =
        red_risk_session(MockSshTransport::default().with_response(SERVER_RED, output("")));
    let (decision, payload) = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        proposed_decision_code(
            &session,
            AiAction::SuggestCommand {
                command: SERVER_RED.to_string(),
            },
        ),
    )
    .await
    .expect("Dialog muss enden");

    assert!(
        matches!(&decision, Decision::Confirm { code, .. }
            if code == "FILTER_RED_RISK_REQUIRES_CONFIRM"),
        "{payload}"
    );
    // A2.1: der Grund nennt die rote Achse UND deren Begründung.
    let reason = confirm_reason(&payload);
    assert!(reason.contains("Server-Risiko rot"), "{reason}");
    assert!(
        reason.contains("Firewall-Regeln werden vollständig geleert"),
        "{reason}"
    );
    assert!(
        !reason.contains("Daten-Risiko"),
        "nur die tatsächlich rote Achse gehört in den Grund: {reason}"
    );
}

/// Spec 0092, T2: dasselbe für die **Daten**-Achse (Server nicht rot) —
/// scheitert, wenn nur eine der beiden Achsen geprüft wird.
#[tokio::test]
async fn test_t2_data_red_requires_confirmation_despite_allow_rule() {
    let session = red_risk_session(MockSshTransport::default().with_response(DATA_RED, output("")));
    let (decision, payload) = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        proposed_decision_code(
            &session,
            AiAction::SuggestCommand {
                command: DATA_RED.to_string(),
            },
        ),
    )
    .await
    .expect("Dialog muss enden");

    assert!(
        matches!(&decision, Decision::Confirm { code, .. }
            if code == "FILTER_RED_RISK_REQUIRES_CONFIRM"),
        "{payload}"
    );
    let reason = confirm_reason(&payload);
    assert!(reason.contains("Daten-Risiko rot"), "{reason}");
    assert!(
        reason.contains("Gibt alle Umgebungsvariablen aus"),
        "{reason}"
    );
    assert!(
        !reason.contains("Server-Risiko"),
        "nur die tatsächlich rote Achse gehört in den Grund: {reason}"
    );
}

/// Spec 0092, T1/T2, zweite Hälfte: „keine Ausführung vor Klick" — die
/// abgelehnte Aktion erzeugt kein `chat-action-result`, der Transport wird
/// also nie angefasst.
#[tokio::test]
async fn test_t1_t2_nothing_runs_while_the_confirmation_is_pending() {
    for command in [SERVER_RED, DATA_RED] {
        let session =
            red_risk_session(MockSshTransport::default().with_response(command, output("")));
        let emitter = TestEmitter::default();
        let profile_store = InMemoryProfileStore::default();
        let confirmations = ConfirmationRegistry::new();

        let handled = handle_action_proposed(
            &session,
            Uuid::new_v4(),
            AiAction::SuggestCommand {
                command: command.to_string(),
            },
            &emitter,
            &profile_store,
            &confirmations,
            ActionOrigin::Internal,
            test_fresh_rejection_flag(),
        );
        let responder = async {
            resolve_first_confirm(&emitter, &confirmations, ActionUserDecision::Deny).await;
            // Zum Zeitpunkt der Entscheidung darf noch nichts gelaufen sein.
            assert!(
                !emitter
                    .events
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|(name, _)| name == "chat-action-result"),
                "{command} lief schon vor dem Klick"
            );
        };
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            tokio::join!(handled, responder)
        })
        .await
        .expect("Dialog muss enden");

        assert!(
            !emitter
                .events
                .lock()
                .unwrap()
                .iter()
                .any(|(name, _)| name == "chat-action-result"),
            "{command} lief trotz Ablehnung"
        );
    }
}

// --- T3: Einstellung aus ----------------------------------------------

/// Spec 0092, T3/A2.5: Einstellung aus → Entscheidung genau wie vor dieser
/// Spec, für beide Achsen.
#[tokio::test]
async fn test_t3_setting_off_keeps_todays_autoexec() {
    for command in [SERVER_RED, DATA_RED] {
        let mut session =
            red_risk_session(MockSshTransport::default().with_response(command, output("ok")));
        session.parts_mut_for_tests().red_risk_always_confirm = false;
        let (decision, payload) = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            proposed_decision_code(
                &session,
                AiAction::SuggestCommand {
                    command: command.to_string(),
                },
            ),
        )
        .await
        .expect("AutoExec muss ohne Dialog enden");
        assert!(
            matches!(decision, Decision::AutoExec),
            "{command}: {payload}"
        );
    }
}

// --- T4: Gelb eskaliert nicht ----------------------------------------

/// Spec 0092, T4: Gelb bleibt Gelb — die Einstellung darf nicht auf „jedes
/// Risiko" ausrutschen.
#[tokio::test]
async fn test_t4_yellow_alone_is_not_escalated() {
    let session =
        red_risk_session(MockSshTransport::default().with_response(YELLOW_ONLY, output("ok")));
    let (decision, payload) = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        proposed_decision_code(
            &session,
            AiAction::SuggestCommand {
                command: YELLOW_ONLY.to_string(),
            },
        ),
    )
    .await
    .expect("AutoExec muss ohne Dialog enden");
    assert!(matches!(decision, Decision::AutoExec), "{payload}");
}

// --- T5/T5b: nur Eskalation, nie Umschreiben -------------------------

/// Spec 0092, T5 / §6 („Nur Eskalation"): ein `Deny` bleibt `Deny`, auch
/// wenn das Kommando rot ist.
#[tokio::test]
async fn test_t5_deny_rule_stays_deny_for_a_red_command() {
    let mut session = red_risk_session(MockSshTransport::default());
    session.parts_mut_for_tests().filter_engine = Box::new(FilterEngine::new(DenyIptablesStore));
    let (decision, payload) = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        proposed_decision_code(
            &session,
            AiAction::SuggestCommand {
                command: SERVER_RED.to_string(),
            },
        ),
    )
    .await
    .expect("Deny endet ohne Dialog");
    assert!(matches!(decision, Decision::Deny { .. }), "{payload}");
}

/// Spec 0092, T5b: die Hard-Blacklist liefert heute `Confirm` mit ihrem
/// eigenen Code — das neue Glied darf einen bereits vorliegenden `Confirm`
/// nicht umschreiben und damit den genaueren Grund verlieren.
#[tokio::test]
async fn test_t5b_hard_blacklist_keeps_its_own_code() {
    let session = red_risk_session(MockSshTransport::default());
    let (decision, payload) = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        proposed_decision_code(
            &session,
            AiAction::SuggestCommand {
                command: "rm -rf /tmp/x".to_string(),
            },
        ),
    )
    .await
    .expect("Dialog muss enden");
    assert!(
        matches!(&decision, Decision::Confirm { code, .. } if code == "FILTER_HARD_BLACKLIST"),
        "{payload}"
    );
}

// --- T6: Injection-Flag wird nur gelesen ------------------------------

/// Spec 0092, T6/A2.3: Bei gesetztem Verdachts-Flag zeigt das neue Glied den
/// Injection-Grund — und **verbraucht das Flag nicht**. Die nächste,
/// unauffällige Aktion mit Allow-Regel muss deshalb noch per Injection
/// eskalieren. Genau das war der Grund, dieses Glied vor die
/// Injection-Prüfung zu setzen.
#[tokio::test]
async fn test_t6_red_with_injection_flag_shows_injection_reason_and_keeps_the_flag() {
    let session = red_risk_session(
        MockSshTransport::default()
            .with_response(SERVER_RED, output(""))
            .with_response("ls -la", output("total 0")),
    );
    session
        .injection_suspected
        .store(true, std::sync::atomic::Ordering::SeqCst);

    let (first, first_payload) = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        proposed_decision_code(
            &session,
            AiAction::SuggestCommand {
                command: SERVER_RED.to_string(),
            },
        ),
    )
    .await
    .expect("Dialog muss enden");
    assert!(
        matches!(&first, Decision::Confirm { code, .. }
            if code == "FILTER_INJECTION_SUSPECTED_REQUIRES_CONFIRM"),
        "{first_payload}"
    );
    // Der rote Grund bleibt im Text sichtbar, nur der Code ist der
    // alarmierendere.
    assert!(
        confirm_reason(&first_payload).contains("Server-Risiko rot"),
        "{first_payload}"
    );
    assert!(
        session
            .injection_suspected
            .load(std::sync::atomic::Ordering::SeqCst),
        "das neue Glied darf das Verdachts-Flag nicht verbrauchen"
    );

    // Die eigentliche Folgeaktion: unauffällig, mit Allow-Regel — muss
    // trotzdem eskalieren, weil das Flag noch steht.
    let (second, second_payload) = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        proposed_decision_code(
            &session,
            AiAction::SuggestCommand {
                command: "ls -la".to_string(),
            },
        ),
    )
    .await
    .expect("Dialog muss enden");
    assert!(
        matches!(&second, Decision::Confirm { code, .. }
            if code == "FILTER_INJECTION_SUSPECTED_REQUIRES_CONFIRM"),
        "Folgeaktion darf nicht automatisch laufen: {second_payload}"
    );
}

// --- T7: Secret-Pfad hat Vorrang --------------------------------------

/// Spec 0092, T7/A2.4: Ein Secret-Pfad-Lesen, das zugleich rot ist, behält
/// seinen genaueren Code — sonst verlöre der Dialog die Aussage „hier wird
/// ein Geheimnis gelesen".
#[tokio::test]
async fn test_t7_secret_path_code_wins_over_the_red_risk_code() {
    let session =
        red_risk_session(MockSshTransport::default().with_response("cat /etc/shadow", output("")));
    let (decision, payload) = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        proposed_decision_code(
            &session,
            AiAction::SuggestCommand {
                command: "cat /etc/shadow".to_string(),
            },
        ),
    )
    .await
    .expect("Dialog muss enden");
    assert_eq!(
        RuleBasedRiskClassifier
            .classify("cat /etc/shadow")
            .data_risk,
        RiskLevel::Red,
        "Vorbedingung: der Fall ist tatsächlich auch rot"
    );
    assert!(
        matches!(&decision, Decision::Confirm { code, .. }
            if code == "FILTER_SECRET_PATH_READ_REQUIRES_CONFIRM"),
        "{payload}"
    );
}

/// Spec 0092, A2.4, Gegenstück zum `sftp-server`-Glied: derselbe Vorrang.
#[tokio::test]
async fn test_sftp_server_code_wins_over_the_red_risk_code() {
    let session =
        red_risk_session(MockSshTransport::default().with_response("sftp-server", output("")));
    let (decision, payload) = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        proposed_decision_code(
            &session,
            AiAction::SuggestCommand {
                command: "sftp-server".to_string(),
            },
        ),
    )
    .await
    .expect("Dialog muss enden");
    assert!(
        matches!(&decision, Decision::Confirm { code, .. }
            if code == "FILTER_SFTP_SERVER_REQUIRES_CONFIRM"),
        "{payload}"
    );
}

// --- T8/T8b/T9: die Wege am Glied vorbei ------------------------------

/// Spec 0092, T8/A2.2: MCP läuft durch dieselbe Funktion — das neue Glied
/// steht vor dem MCP-Glied, der Dialog nennt deshalb den roten Grund statt
/// nur „über MCP angefragt".
#[tokio::test]
async fn test_t8_mcp_origin_gets_the_red_risk_code() {
    let session =
        red_risk_session(MockSshTransport::default().with_response(SERVER_RED, output("")));
    let (decision, payload) = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        proposed_decision_code_with_origin(
            &session,
            AiAction::SuggestCommand {
                command: SERVER_RED.to_string(),
            },
            ActionOrigin::Mcp { client_name: None },
        ),
    )
    .await
    .expect("Dialog muss enden");
    assert!(
        matches!(&decision, Decision::Confirm { code, .. }
            if code == "FILTER_RED_RISK_REQUIRES_CONFIRM"),
        "{payload}"
    );
}

/// Spec 0092, T8b: Eine Allow-Regel auf die Form **ohne** `sudo` deckt per
/// Dual-Text-Matching (ADR 0002) auch `sudo …` ab — der Klassifizierer löst
/// `sudo` ebenfalls auf, das Glied greift also auch hier. Kein hinterlegtes
/// Sudo-Passwort, damit nicht dessen eigene Eskalation den Code liefert.
#[tokio::test]
async fn test_t8b_sudo_form_under_a_non_sudo_allow_rule_is_escalated() {
    let sudo_command = format!("sudo {SERVER_RED}");
    let mut session =
        red_risk_session(MockSshTransport::default().with_response(&sudo_command, output("")));
    session.parts_mut_for_tests().filter_engine =
        Box::new(FilterEngine::new(AllowExactIptablesFlushStore));
    assert!(
        session.sudo_password.is_none(),
        "Vorbedingung: kein gespeichertes Sudo-Passwort"
    );

    let (decision, payload) = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        proposed_decision_code(
            &session,
            AiAction::SuggestCommand {
                command: sudo_command.clone(),
            },
        ),
    )
    .await
    .expect("Dialog muss enden");
    assert!(
        matches!(&decision, Decision::Confirm { code, .. }
            if code == "FILTER_RED_RISK_REQUIRES_CONFIRM"),
        "{payload}"
    );
}

/// Spec 0092, T9: Auch Pseudokommandos werden geprüft — ein
/// `ReadRemoteFile` auf einen rot eingestuften Pfad, der **kein**
/// Secret-Pfad ist (sonst griffe das Glied von Spec 0068).
#[tokio::test]
async fn test_t9_read_remote_file_on_a_red_non_secret_path_is_escalated() {
    let session = red_risk_session(MockSshTransport::default());
    let (decision, payload) = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        proposed_decision_code(
            &session,
            AiAction::ReadRemoteFile {
                path: DATA_RED_READ_PATH.to_string(),
            },
        ),
    )
    .await
    .expect("Dialog muss enden");
    assert!(
        matches!(&decision, Decision::Confirm { code, .. }
            if code == "FILTER_RED_RISK_REQUIRES_CONFIRM"),
        "{payload}"
    );
}

/// Spec 0092 (Folge aus T1/T2): `cat ~/.ssh/id_rsa.pub` ist kein
/// Secret-Pfad (öffentlicher Schlüssel, s. Spec 0068), der Klassifizierer
/// stuft es auf der Daten-Achse aber rot ein — mit eingeschalteter
/// Einstellung eskaliert es deshalb über das neue Glied. Hier festgehalten,
/// weil dieser Fall den bestehenden Spec-0068-Test angepasst hat
/// (`test_secret_path_read_always_requires_confirm_even_with_allow_rule`).
#[tokio::test]
async fn test_red_risk_escalates_a_public_key_read_that_is_no_secret_path() {
    let command = "cat ~/.ssh/id_rsa.pub";
    assert!(
        ssh_manager_core::risk::secret_path_read_reason(command).is_none(),
        "Vorbedingung: kein Secret-Pfad"
    );
    let session = red_risk_session(MockSshTransport::default().with_response(command, output("")));
    let (decision, payload) = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        proposed_decision_code(
            &session,
            AiAction::SuggestCommand {
                command: command.to_string(),
            },
        ),
    )
    .await
    .expect("Dialog muss enden");
    assert!(
        matches!(&decision, Decision::Confirm { code, .. }
            if code == "FILTER_RED_RISK_REQUIRES_CONFIRM"),
        "{payload}"
    );
}

// --- T17: Messfall Mehrzeilen-Skript ----------------------------------

/// Spec 0092, T17 (Messfall): ein mehrzeiliges Skript, dessen **eine** Zeile
/// rot ist. Der Klassifizierer stuft es rot ein (er prüft jedes
/// Teilkommando, `segment_command` trennt auch an Zeilenumbrüchen) — dann
/// muss auch das Glied greifen, sonst wäre ein Mehrzeiler der einfache Weg
/// daran vorbei.
#[tokio::test]
async fn test_t17_multiline_script_with_one_red_line_is_escalated() {
    let script = format!("echo hallo\n{SERVER_RED}");
    assert_eq!(
        RuleBasedRiskClassifier.classify(&script).server_risk,
        RiskLevel::Red,
        "Messfall-Vorbedingung: der Klassifizierer stuft den Mehrzeiler rot ein"
    );
    let session = red_risk_session(MockSshTransport::default().with_response(&script, output("")));
    let (decision, payload) = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        proposed_decision_code(
            &session,
            AiAction::SuggestCommand {
                command: script.clone(),
            },
        ),
    )
    .await
    .expect("Dialog muss enden");
    assert!(
        matches!(&decision, Decision::Confirm { code, .. }
            if code == "FILTER_RED_RISK_REQUIRES_CONFIRM"),
        "{payload}"
    );
}

// --- reine Funktion `red_risk_reason` ---------------------------------

fn assessment(server: RiskLevel, data: RiskLevel) -> RiskAssessment {
    RiskAssessment {
        server_risk: server,
        server_risk_reason: (server != RiskLevel::None).then(|| "S-Grund".to_string()),
        data_risk: data,
        data_risk_reason: (data != RiskLevel::None).then(|| "D-Grund".to_string()),
        ai_reviewed: false,
    }
}

#[test]
fn test_red_risk_reason_is_none_without_an_assessment() {
    assert!(red_risk_reason(None).is_none());
}

#[test]
fn test_red_risk_reason_is_none_below_red() {
    for (server, data) in [
        (RiskLevel::None, RiskLevel::None),
        (RiskLevel::Yellow, RiskLevel::None),
        (RiskLevel::None, RiskLevel::Yellow),
        (RiskLevel::Yellow, RiskLevel::Yellow),
    ] {
        assert!(
            red_risk_reason(Some(&assessment(server, data))).is_none(),
            "{server:?}/{data:?}"
        );
    }
}

/// A2.1: Rot auf **einer** der Achsen genügt; sind beide rot, nennt der
/// Grund beide.
#[test]
fn test_red_risk_reason_names_every_red_axis() {
    let only_server = red_risk_reason(Some(&assessment(RiskLevel::Red, RiskLevel::Yellow)))
        .expect("Server-Rot genügt");
    assert_eq!(only_server, "Server-Risiko rot: S-Grund");

    let only_data = red_risk_reason(Some(&assessment(RiskLevel::Yellow, RiskLevel::Red)))
        .expect("Daten-Rot genügt");
    assert_eq!(only_data, "Daten-Risiko rot: D-Grund");

    let both =
        red_risk_reason(Some(&assessment(RiskLevel::Red, RiskLevel::Red))).expect("beide rot");
    assert_eq!(
        both,
        "Server-Risiko rot: S-Grund; Daten-Risiko rot: D-Grund"
    );
}

/// Eine fehlende Begründung darf die Eskalation nicht verhindern —
/// fail-safe in Richtung Bestätigung.
#[test]
fn test_red_risk_reason_escalates_even_without_a_pattern_reason() {
    let bare = RiskAssessment {
        server_risk: RiskLevel::Red,
        server_risk_reason: None,
        data_risk: RiskLevel::None,
        data_risk_reason: None,
        ai_reviewed: false,
    };
    assert_eq!(
        red_risk_reason(Some(&bare)).as_deref(),
        Some("Server-Risiko rot: rot eingestuft")
    );
}
