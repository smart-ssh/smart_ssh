//! Testsuite für die Filter-Engine.
//!
//! Tests 1-12 setzen Zeile für Zeile den Testfall-Katalog aus
//! `docs/specs/0002-filter-engine-spec.md`, Abschnitt 6 um (Testname
//! referenziert die Tabellenzeile in einem Kommentar). Danach folgen
//! zusätzliche Tests für Groß-/Kleinschreibung, Whitespace-Varianten,
//! verschachtelte Command-Substitution und die Elevated-Erkennung.

use async_trait::async_trait;

use super::*;

struct InMemoryPolicyStore {
    rules: Vec<Rule>,
}

impl InMemoryPolicyStore {
    fn new(rules: Vec<Rule>) -> Self {
        Self { rules }
    }
}

#[async_trait]
impl PolicyStore for InMemoryPolicyStore {
    async fn rules_for(&self, scope: &EffectiveScope) -> Vec<Rule> {
        self.rules
            .iter()
            .filter(|rule| scope_applies(&rule.scope, scope))
            .cloned()
            .collect()
    }
}

fn glob_rule(id: &str, glob: &str, action: RuleAction, scope: Scope, priority: i32) -> Rule {
    Rule {
        id: RuleId(id.to_string()),
        pattern: Pattern::Glob(glob.to_string()),
        action,
        scope,
        priority,
    }
}

/// `_server_label` dient nur der Lesbarkeit an den Call-Sites; kein Test
/// prüft `Scope::Server(...)`-Regeln gegen einen konkreten Wert, daher reicht
/// eine frische `ServerId` (seit Spec 0003 Uuid-basiert, s. `crate::shared`).
fn ctx(_server_label: &str, tags: &[&str]) -> EvalContext {
    EvalContext {
        server_id: ServerId::new(),
        tags: tags.iter().map(|t| t.to_string()).collect(),
    }
}

fn engine(rules: Vec<Rule>) -> FilterEngine<InMemoryPolicyStore> {
    FilterEngine::new(InMemoryPolicyStore::new(rules))
}

fn assert_auto_exec(decision: &Decision) {
    assert!(
        matches!(decision, Decision::AutoExec),
        "expected AutoExec, got {decision:?}"
    );
}

fn assert_confirm(decision: &Decision) {
    assert!(
        matches!(decision, Decision::Confirm { .. }),
        "expected Confirm, got {decision:?}"
    );
}

fn assert_deny(decision: &Decision) {
    assert!(
        matches!(decision, Decision::Deny { .. }),
        "expected Deny, got {decision:?}"
    );
}

fn assert_deny_or_confirm(decision: &Decision) {
    assert!(
        matches!(decision, Decision::Deny { .. } | Decision::Confirm { .. }),
        "expected Deny or Confirm, got {decision:?}"
    );
}

// --- Testfall-Katalog, Spec Abschnitt 6 -------------------------------

/// Tabellenzeile 1: einfacher Whitelist-Treffer.
#[tokio::test]
async fn test_whitelist_hit_grants_autoexec() {
    let eng = engine(vec![glob_rule(
        "allow-ls",
        "ls *",
        RuleAction::Allow,
        Scope::Global,
        0,
    )]);
    let decision = eng.evaluate("ls -la", &ctx("srv1", &[])).await;
    assert_auto_exec(&decision);
}

/// Tabellenzeile 2: Hard-Blacklist greift immer, mindestens Confirm.
#[tokio::test]
async fn test_hard_blacklist_forces_at_least_confirm() {
    let eng = engine(vec![]);
    let decision = eng.evaluate("rm -rf /", &ctx("srv1", &[])).await;
    assert_confirm(&decision);
}

/// Tabellenzeile 3: Chaining darf die Blacklist nicht umgehen — das erste
/// Teilkommando allein wäre AutoExec-fähig, das zweite trifft die Blacklist.
#[tokio::test]
async fn test_chaining_cannot_bypass_blacklist() {
    let eng = engine(vec![glob_rule(
        "allow-ls",
        "ls *",
        RuleAction::Allow,
        Scope::Global,
        0,
    )]);
    let decision = eng
        .evaluate("ls -la && rm -rf /var/backup", &ctx("srv1", &[]))
        .await;
    assert_deny_or_confirm(&decision);
}

/// Tabellenzeile 4: Command-Substitution erzwingt mindestens Confirm.
#[tokio::test]
async fn test_command_substitution_forces_confirm() {
    let eng = engine(vec![glob_rule(
        "allow-ls",
        "ls *",
        RuleAction::Allow,
        Scope::Global,
        0,
    )]);
    let decision = eng
        .evaluate("ls $(cat /etc/passwd)", &ctx("srv1", &[]))
        .await;
    assert_confirm(&decision);
}

/// Tabellenzeile 5: Scope-Präzedenz — eine Tag-scope Deny-Regel greift für
/// Server mit passendem Tag, unabhängig davon ob eine globale Regel existiert.
#[tokio::test]
async fn test_scope_precedence_tag_deny_overrides_default() {
    let eng = engine(vec![glob_rule(
        "deny-systemctl-prod",
        "systemctl *",
        RuleAction::Deny,
        Scope::Tag("production".to_string()),
        0,
    )]);
    let decision = eng
        .evaluate("systemctl status nginx", &ctx("srv1", &["production"]))
        .await;
    assert_deny(&decision);
}

/// Tabellenzeile 6: der Inhalt eines Strings wird nicht als eigenes Kommando
/// interpretiert — nur das literale `echo "..."` wird gegen `echo *` geprüft.
#[tokio::test]
async fn test_echo_argument_not_interpreted_as_command() {
    let eng = engine(vec![glob_rule(
        "allow-echo",
        "echo *",
        RuleAction::Allow,
        Scope::Global,
        0,
    )]);
    let decision = eng.evaluate(r#"echo "ls -la""#, &ctx("srv1", &[])).await;
    assert_auto_exec(&decision);
}

/// Tabellenzeile 7: eine aktive "sudo -> Confirm"-Regel matcht gegen den
/// Original-Text (inkl. Präfix), s. Design-Kommentar bei `evaluate_rules`.
#[tokio::test]
async fn test_sudo_prefix_rule_forces_confirm() {
    let eng = engine(vec![glob_rule(
        "confirm-sudo",
        "sudo *",
        RuleAction::Confirm,
        Scope::Global,
        0,
    )]);
    let decision = eng.evaluate("sudo apt update", &ctx("srv1", &[])).await;
    assert_confirm(&decision);
}

/// Tabellenzeile 8: unausgeglichene Anführungszeichen -> Parser-Fallback,
/// nie AutoExec — selbst mit einer maximal permissiven Allow-Regel.
#[tokio::test]
async fn test_ambiguous_quotes_never_autoexec() {
    let eng = engine(vec![glob_rule(
        "allow-all",
        "*",
        RuleAction::Allow,
        Scope::Global,
        0,
    )]);
    let decision = eng
        .evaluate(r#"echo "unterminated"#, &ctx("srv1", &[]))
        .await;
    assert_confirm(&decision);
}

/// Tabellenzeile 9: Default-Fallback für ein Teilkommando ohne Regel-Treffer.
#[tokio::test]
async fn test_default_fallback_for_unknown_subcommand() {
    let eng = engine(vec![glob_rule(
        "allow-ls",
        "ls *",
        RuleAction::Allow,
        Scope::Global,
        0,
    )]);
    let decision = eng
        .evaluate("ls -la; rm important.txt", &ctx("srv1", &[]))
        .await;
    assert_confirm(&decision);
}

/// Tabellenzeile 10: leerer/reiner Whitespace-String -> Deny.
#[tokio::test]
async fn test_empty_command_is_denied() {
    let eng = engine(vec![]);
    let decision = eng.evaluate("   \t  ", &ctx("srv1", &[])).await;
    assert_deny(&decision);
}

/// Tabellenzeile 11: Kommando über dem (konfigurierbaren) Längenlimit ->
/// Confirm, ohne dass der Rest überhaupt geparst wird.
#[tokio::test]
async fn test_command_exceeding_length_limit_forces_confirm() {
    let eng = FilterEngine::with_max_command_length(InMemoryPolicyStore::new(vec![]), 10);
    let long_command = format!("echo {}", "a".repeat(20));
    let decision = eng.evaluate(&long_command, &ctx("srv1", &[])).await;
    assert_confirm(&decision);
}

/// Tabellenzeile 12: bei gleicher Priorität/gleichem Scope gewinnt im
/// Zweifel die strengere Regel (Confirm schlägt Allow).
#[tokio::test]
async fn test_conflicting_rules_same_priority_prefer_stricter() {
    let rules = vec![
        glob_rule(
            "allow-restart",
            "restart-service *",
            RuleAction::Allow,
            Scope::Global,
            5,
        ),
        glob_rule(
            "confirm-restart",
            "restart-service *",
            RuleAction::Confirm,
            Scope::Global,
            5,
        ),
    ];
    let eng = engine(rules);
    let decision = eng
        .evaluate("restart-service nginx", &ctx("srv1", &[]))
        .await;
    assert_confirm(&decision);
}

// --- Zusätzliche Tests -------------------------------------------------

#[tokio::test]
async fn test_pattern_matching_is_case_sensitive_for_user_rules() {
    let eng = engine(vec![glob_rule(
        "allow-ls",
        "ls *",
        RuleAction::Allow,
        Scope::Global,
        0,
    )]);
    // "LS" ist nicht "ls" - Nutzerregeln matchen case-sensitiv, daher Default
    // Confirm statt AutoExec.
    let decision = eng.evaluate("LS -la", &ctx("srv1", &[])).await;
    assert_confirm(&decision);
}

#[tokio::test]
async fn test_hard_blacklist_is_case_insensitive() {
    let eng = engine(vec![]);
    // Anders als Nutzerregeln: die Blacklist matcht case-insensitiv als
    // zusätzliche Sicherheitsmarge.
    let decision = eng.evaluate("RM -RF /", &ctx("srv1", &[])).await;
    assert_confirm(&decision);
}

#[tokio::test]
async fn test_whitespace_variants_do_not_bypass_matching() {
    let eng = engine(vec![glob_rule(
        "allow-ls",
        "ls *",
        RuleAction::Allow,
        Scope::Global,
        0,
    )]);
    let decision = eng.evaluate("ls\t\t  -la", &ctx("srv1", &[])).await;
    assert_auto_exec(&decision);
}

#[tokio::test]
async fn test_nested_command_substitution_forces_confirm() {
    let eng = engine(vec![glob_rule(
        "allow-echo",
        "echo *",
        RuleAction::Allow,
        Scope::Global,
        0,
    )]);
    let decision = eng
        .evaluate("echo $(echo $(whoami))", &ctx("srv1", &[]))
        .await;
    assert_confirm(&decision);
}

#[tokio::test]
async fn test_sudo_prefix_sets_elevated_flag() {
    let (elevated, rest) = super::parser::detect_elevation("sudo apt update");
    assert!(elevated);
    assert_eq!(rest, "apt update");

    let (elevated_doas, rest_doas) = super::parser::detect_elevation("doas apt update");
    assert!(elevated_doas);
    assert_eq!(rest_doas, "apt update");

    let (elevated_none, rest_none) = super::parser::detect_elevation("apt update");
    assert!(!elevated_none);
    assert_eq!(rest_none, "apt update");
}

/// Ergänzt Zeile 2 der Tabelle: die Blacklist lässt sich nicht durch eine
/// explizite (auch hoch priorisierte) Allow-Regel aushebeln — genau die in
/// Spec Abschnitt 3.1 verlangte Eigenschaft "unabhängig von Nutzerregeln".
#[tokio::test]
async fn test_hard_blacklist_cannot_be_overridden_by_allow_rule() {
    let eng = engine(vec![glob_rule(
        "allow-everything-dangerous",
        "rm -rf /*",
        RuleAction::Allow,
        Scope::Global,
        100,
    )]);
    let decision = eng.evaluate("rm -rf /", &ctx("srv1", &[])).await;
    assert_confirm(&decision);
}

/// Ein Kommando mit mehreren `&&`-Teilkommandos und einer `$(...)`-
/// Substitution kombiniert mehrere Teil-Decisions zu einer einzigen
/// `Confirm`-Reason (`merge_reasons`) — ohne Deduplizierung wiederholt sich
/// "keine Regel gefunden" für jedes Teilkommando ohne passende Regel und
/// macht die im Frontend angezeigte Begründung unleserlich lang. Jeder
/// inhaltlich eigenständige Grund darf nur einmal vorkommen, egal wie oft
/// er beim Zusammenführen der Teilergebnisse erneut auftritt.
#[tokio::test]
async fn test_merged_reason_does_not_repeat_identical_parts_across_segments() {
    let eng = engine(vec![]);
    let decision = eng.evaluate(
        "cp /etc/fstab /etc/fstab.bak-$(date +%Y%m%d) && sed -i 's/a/b/' /etc/fstab && grep -n extern1 /etc/fstab",
        &ctx("srv1", &[]),
    ).await;
    let Decision::Confirm { reason, .. } = decision else {
        panic!("expected Confirm, got {decision:?}");
    };
    let parts: Vec<&str> = reason.split("; ").collect();
    let mut deduped = parts.clone();
    deduped.sort();
    deduped.dedup();
    assert_eq!(
        parts.len(),
        deduped.len(),
        "reason enthält doppelte Teile: {reason}"
    );
    assert!(reason.contains("keine Regel gefunden"));
    assert!(reason.contains("Command-Substitution"));
}

// --- Spec 0013: Security Hardening Tests (T1 - T4) --------------------

/// T1: Zeilenumbruch-Splitting verhindert Umgehung von Allow-Regeln.
#[tokio::test]
async fn test_t1_newline_splitting_cannot_bypass_allow_rule() {
    let eng = engine(vec![glob_rule(
        "allow-echo",
        "echo *",
        RuleAction::Allow,
        Scope::Global,
        10,
    )]);
    let decision = eng.evaluate("echo safe\nrm -rf /", &ctx("srv1", &[])).await;
    assert_deny_or_confirm(&decision);
}

/// T2: Hintergrund-Operator `&` wird als Segment-Trenner behandelt.
#[tokio::test]
async fn test_t2_ampersand_splitting() {
    let eng = engine(vec![glob_rule(
        "allow-cmd1",
        "cmd1",
        RuleAction::Allow,
        Scope::Global,
        10,
    )]);
    let decision = eng.evaluate("cmd1 & cmd2", &ctx("srv1", &[])).await;
    // cmd2 hat keine Regel -> Confirm
    assert_confirm(&decision);
}

/// T3: Bash-Prozess-Substitution `<(...)` und `>(...)` wird erkannt und rekursiv evaluiert.
#[tokio::test]
async fn test_t3_process_substitution_forces_confirm() {
    let eng = engine(vec![glob_rule(
        "allow-cat",
        "cat *",
        RuleAction::Allow,
        Scope::Global,
        10,
    )]);
    let decision_in = eng.evaluate("cat <(malicious)", &ctx("srv1", &[])).await;
    assert_confirm(&decision_in);

    let decision_out = eng.evaluate("cat >(malicious)", &ctx("srv1", &[])).await;
    assert_confirm(&decision_out);
}

/// T4: Nicht-druckbare Steuerzeichen / Escape-Sequenzen erzwingen Ambiguous/Confirm.
#[tokio::test]
async fn test_t4_control_characters_rejected() {
    let eng = engine(vec![glob_rule(
        "allow-all",
        "*",
        RuleAction::Allow,
        Scope::Global,
        100,
    )]);
    let decision_null = eng.evaluate("cmd\0extra", &ctx("srv1", &[])).await;
    assert_confirm(&decision_null);

    let decision_esc = eng.evaluate("cmd\x1b[31m", &ctx("srv1", &[])).await;
    assert_confirm(&decision_esc);
}

/// Blacklist-Evasion-Tests: Pfad-Varianten, Flag-Permutationen, dd, power commands, shadow.
#[tokio::test]
async fn test_blacklist_evasion_variants() {
    let eng = engine(vec![]);
    for cmd in [
        "/bin/rm -rf /",
        "/usr/bin/rm -r -f /",
        "rm -f -r /",
        "rm --recursive --force /",
        "rm / -rf",
        "\\rm -rf /",
        "rm -rf /*",
        "dd of=/dev/sda if=/dev/zero",
        "dd of=/dev/nvme0n1",
        "/sbin/shutdown -h now",
        "systemctl reboot",
        "systemctl poweroff",
        "init 0",
        "tee /etc/shadow",
        "sed -i 's/root/toor/' /etc/shadow",
        "chmod 777 /etc/shadow",
    ] {
        let decision = eng.evaluate(cmd, &ctx("srv1", &[])).await;
        assert_confirm(&decision);
    }
}
