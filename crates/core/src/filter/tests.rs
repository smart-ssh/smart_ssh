//! Testsuite für die Filter-Engine.
//!
//! Tests 1-12 setzen Zeile für Zeile den Testfall-Katalog aus
//! `docs/specs/0002-filter-engine-spec.md`, Abschnitt 6 um (Testname
//! referenziert die Tabellenzeile in einem Kommentar). Danach folgen
//! zusätzliche Tests für Groß-/Kleinschreibung, Whitespace-Varianten,
//! verschachtelte Command-Substitution und die Elevated-Erkennung.

use std::sync::Arc;

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
    glob_rule_with_origin(id, glob, action, scope, priority, RuleOrigin::User)
}

/// Wie [`glob_rule`], mit explizit wählbarer `RuleOrigin` — für die Spec-
/// 0037-Policy-Ebenen-Tests, die eine `Organization`- gegen eine `User`-
/// Regel im selben Aktions-Bucket testen (s. `AllowEverythingPolicyStore`-
/// ähnliche `InMemoryPolicyStore`-Instanzen unten).
fn glob_rule_with_origin(
    id: &str,
    glob: &str,
    action: RuleAction,
    scope: Scope,
    priority: i32,
    origin: RuleOrigin,
) -> Rule {
    Rule {
        id: RuleId(id.to_string()),
        pattern: Pattern::Glob(glob.to_string()),
        action,
        scope,
        priority,
        origin,
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

/// Baut ein `depth`-fach verschachteltes Command-Substitutions-Kommando
/// (`echo $(echo $(... whoami ...))`) — `depth` Ebenen `echo $(...)` um
/// `whoami`. Mit `strip_substitutions`/`segment_command`s Rekursions-
/// Zählweise wird das innerste `whoami` dann bei genau `depth`
/// ausgewertet (s. `parser::MAX_SUBSTITUTION_DEPTH`-Doc-Kommentar).
fn nested_substitution_command(depth: usize) -> String {
    let mut cmd = "whoami".to_string();
    for _ in 0..depth {
        cmd = format!("echo $({cmd})");
    }
    cmd
}

/// Spec 0043, Fund B: ein Kommando, das über den expliziten Rekursions-Cap
/// hinaus verschachtelt ist, landet bei `Deny` mit dem korrekten Grund
/// ("zu tief verschachtelt") — kein Stack-Overflow, kein `AutoExec`.
///
/// `Deny`, NICHT `Confirm` (Abweichung vom wörtlichen Spec-0043-Text, s.
/// ADR-Kommentar in `engine::evaluate_parsed_explained`): ein zu tief
/// verschachteltes Kommando ist nicht sicher analysierbar, könnte also
/// jedes beliebige innere Kommando verstecken — `Confirm` wäre hier eine
/// Abschwächung gegenüber einem `Deny`, das auf einer nicht mehr
/// erreichten inneren Ebene gegriffen hätte (s.
/// `test_t43_deep_nesting_cannot_downgrade_a_deny_rule_to_confirm`).
#[tokio::test]
async fn test_t43_substitution_depth_over_cap_forces_deny_with_reason() {
    let eng = engine(vec![]);
    let command = nested_substitution_command(super::parser::MAX_SUBSTITUTION_DEPTH + 1);

    let trace = eng.evaluate_explained(&command, &ctx("srv1", &[])).await;

    match &trace.decision {
        Decision::Deny { reason, .. } => {
            assert!(
                reason.contains("zu tief verschachtelt"),
                "erwartete Tiefen-Cap-Begründung, war: {reason}"
            );
        }
        other => panic!("erwartete Deny, war: {other:?}"),
    }
}

/// Regressionstest für den spec-reviewer-Fund zu Commit fd5b45a: ein
/// Nutzer-`Deny "rm *"` darf durch ausreichende Verschachtelungstiefe NICHT
/// auf `Confirm` abgeschwächt werden — ein 33-fach verschachteltes, nur
/// ~270 Zeichen langes (also weit unter `DEFAULT_MAX_COMMAND_LENGTH`)
/// `echo $(...)`-Kommando um `rm -rf /` muss weiterhin `Deny` liefern, auch
/// wenn die Rekursion die innere `rm`-Ebene wegen des Tiefen-Caps gar nicht
/// mehr erreicht — der Tiefen-Cap selbst liefert dann `Deny` (s. oben),
/// nicht `Confirm`.
#[tokio::test]
async fn test_t43_deep_nesting_cannot_downgrade_a_deny_rule_to_confirm() {
    let eng = engine(vec![glob_rule(
        "deny-rm",
        "rm *",
        RuleAction::Deny,
        Scope::Global,
        0,
    )]);
    let mut inner = "rm -rf /".to_string();
    for _ in 0..(super::parser::MAX_SUBSTITUTION_DEPTH + 1) {
        inner = format!("echo $({inner})");
    }

    let decision = eng.evaluate(&inner, &ctx("srv1", &[])).await;

    match decision {
        Decision::Deny { .. } => {}
        other => panic!(
            "ein Deny darf durch Verschachtelungstiefe nicht auf {other:?} abgeschwächt werden"
        ),
    }
}

/// Spec 0043, Fund B, Testbarkeit: ein Kommando knapp UNTER dem Cap wird
/// weiterhin normal geparst — kein Fehlalarm über die Tiefen-Cap-
/// Begründung (die übliche "Command-Substitution erkannt"-Confirm bleibt
/// unabhängig davon bestehen, s. `test_nested_command_substitution_
/// forces_confirm` oben).
#[tokio::test]
async fn test_t43_substitution_depth_at_cap_parses_normally() {
    let eng = engine(vec![]);
    let command = nested_substitution_command(super::parser::MAX_SUBSTITUTION_DEPTH);

    let trace = eng.evaluate_explained(&command, &ctx("srv1", &[])).await;

    match &trace.decision {
        Decision::Confirm { reason, .. } => {
            assert!(
                !reason.contains("zu tief verschachtelt"),
                "Kommando genau am Cap darf nicht als zu tief verschachtelt gelten, Grund war: {reason}"
            );
        }
        other => panic!("erwartete Confirm (Command-Substitution), war: {other:?}"),
    }
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

// --- Unabhängiger Review-Pass (Spec 0002): verifizierte Bypässe ---------
//
// Diese drei Testgruppen bilden exakt die vom Review-Agent empirisch gegen
// die echte Engine nachgewiesenen Umgehungen nach — vorher jeweils
// AutoExec, obwohl Abschnitt 3.1 die Hard-Blacklist "unabhängig von
// Nutzerregeln" verlangt bzw. eine Ausgabe-Umleitung von der Engine bislang
// überhaupt nicht als eigenständig zu bewertendes Element erkannt wurde.

/// Die harmloseste denkbare Whitelist-Regel (`ls *`, das Spec-eigene
/// Beispiel) durfte bislang trotzdem eine beliebige Datei überschreiben,
/// weil Umleitungsziele der Engine komplett unsichtbar waren.
#[tokio::test]
async fn test_output_redirection_forces_confirm_even_with_matching_allow_rule() {
    let eng = engine(vec![glob_rule(
        "allow-ls",
        "ls *",
        RuleAction::Allow,
        Scope::Global,
        100,
    )]);
    for cmd in [
        "ls -la > /etc/passwd",
        "ls -la >> /root/.ssh/authorized_keys",
        "ls -la > \"/etc/shadow\"",
        "ls -la 2>/tmp/err",
    ] {
        let decision = eng.evaluate(cmd, &ctx("srv1", &[])).await;
        assert_confirm(&decision);
    }
}

/// Ein `>` innerhalb von Anführungszeichen (reiner Text, keine echte
/// Umleitung) und `>(` (Process-Substitution, separat behandelt) dürfen
/// NICHT fälschlich als Umleitung erkannt werden.
#[tokio::test]
async fn test_quoted_or_process_substitution_greater_than_is_not_a_redirection() {
    let eng = engine(vec![glob_rule(
        "allow-echo",
        "echo *",
        RuleAction::Allow,
        Scope::Global,
        100,
    )]);
    let decision = eng.evaluate("echo \"a > b\"", &ctx("srv1", &[])).await;
    assert_auto_exec(&decision);
}

/// Wrapper-Kommandos (`env`, `nice`, `nohup`, `time`, `command`) sowie
/// Sudo-Flags/-Verdopplung und Quoting/Escaping des ersten Worts durften
/// die Hard-Blacklist bislang umgehen.
#[tokio::test]
async fn test_hard_blacklist_cannot_be_evaded_via_wrappers_sudo_flags_or_quoting() {
    let eng = engine(vec![]);
    for cmd in [
        "env rm -rf /",
        "nice rm -rf /",
        "nohup rm -rf /",
        "time rm -rf /",
        "command rm -rf /",
        "sudo -u root rm -rf /",
        "sudo --user=root rm -rf /",
        "sudo sudo rm -rf /",
        "\"rm\" -rf /",
        "'rm' -rf /",
        "r\\m -rf /",
    ] {
        let decision = eng.evaluate(cmd, &ctx("srv1", &[])).await;
        assert_confirm(&decision);
    }
}

/// Regressionstest für den unabhängigen Review-Pass (Spec 0013): weitere,
/// von einer adversarialen Prüfung gegen die echte Engine verifizierte
/// Umgehungen — Wrapper mit Pflicht-Positionsargument (`timeout`/`chroot`/
/// `flock`), zusätzliche einfache Wrapper (`xargs`/`setsid`/`stdbuf`/
/// `ionice`/`busybox`/`script`), eine bloße Variablenzuweisung ganz ohne
/// Wrapper-Kommando (`FOO=1 rm -rf /`) sowie teilweise/versetzte Quotierung
/// des ersten Worts (`r"m"`, `r''m`, `$'rm'`).
#[tokio::test]
async fn test_hard_blacklist_cannot_be_evaded_via_positional_wrappers_or_bare_assignment() {
    let eng = engine(vec![]);
    for cmd in [
        "timeout 5 rm -rf /",
        "timeout --signal=9 5 rm -rf /",
        "chroot /mnt rm -rf /",
        "flock /tmp/lock rm -rf /",
        "xargs rm -rf /",
        "setsid rm -rf /",
        "stdbuf -oL rm -rf /",
        "ionice -c3 rm -rf /",
        "busybox rm -rf /",
        "script -c \"rm -rf /\" /dev/null",
        "FOO=1 rm -rf /",
        "FOO=1 BAR=2 rm -rf /",
        "FOO=1 sudo rm -rf /",
        "LD_PRELOAD=/tmp/x.so rm -rf /",
        "r\"m\" -rf /",
        "r''m -rf /",
        "$'rm' -rf /",
    ] {
        let decision = eng.evaluate(cmd, &ctx("srv1", &[])).await;
        assert_confirm(&decision);
    }
}

/// Dieselbe Wrapper-Umgehung darf auch nicht über Chaining eine sonst
/// zutreffende Allow-Regel unterlaufen (Testfall 3: "Chaining darf die
/// Blacklist nicht umgehen").
#[tokio::test]
async fn test_chaining_with_wrapper_evasion_still_hits_blacklist() {
    let eng = engine(vec![
        glob_rule("allow-ls", "ls *", RuleAction::Allow, Scope::Global, 100),
        glob_rule(
            "allow-sudo",
            "sudo *",
            RuleAction::Allow,
            Scope::Global,
            100,
        ),
    ]);
    let decision = eng
        .evaluate("ls -la && sudo -u root rm -rf /", &ctx("srv1", &[]))
        .await;
    assert_confirm(&decision);
}

/// Regressionstest für den unabhängigen Review-Pass (Spec 0009): eine
/// Deny-Regel muss dieselbe Wrapper-/Sudo-Flag-/Quoting-Normalisierung
/// bekommen wie die Hard-Blacklist — sonst umgeht `env docker rm -f prod`
/// eine explizite `Deny "docker *"`-Regel genauso, wie es vorher die
/// Blacklist umging.
#[tokio::test]
async fn test_deny_rule_cannot_be_evaded_via_wrapper_command() {
    let eng = engine(vec![glob_rule(
        "deny-docker",
        "docker *",
        RuleAction::Deny,
        Scope::Global,
        100,
    )]);
    for cmd in [
        "docker rm -f prod",
        "env docker rm -f prod",
        "sudo docker rm -f prod",
    ] {
        let decision = eng.evaluate(cmd, &ctx("srv1", &[])).await;
        assert_deny(&decision);
    }
}

/// Regressionstest für den unabhängigen Review-Pass (Spec 0002): eine
/// `Deny`-Regel muss auch greifen, wenn dasselbe Programm über einen
/// absoluten oder relativen Pfad statt des bloßen Namens aufgerufen wird —
/// die Hard-Blacklist deckt das bereits über ihre eigene Pfad-Alternation
/// ab (`blacklist.rs`), `resolve_effective_command` (die laut Spec 0002,
/// Abschnitt 4.6 "die Basis für jede Prüfung" sein soll) hatte diese Sicht
/// vorher nicht.
#[tokio::test]
async fn test_deny_rule_cannot_be_evaded_via_absolute_or_relative_path() {
    let eng = engine(vec![glob_rule(
        "deny-docker",
        "docker *",
        RuleAction::Deny,
        Scope::Global,
        100,
    )]);
    for cmd in ["/usr/bin/docker rm -f prod", "./docker rm -f prod"] {
        let decision = eng.evaluate(cmd, &ctx("srv1", &[])).await;
        assert_deny(&decision);
    }
}

/// Regressionstest für den unabhängigen Review-Pass (Spec 0002): die volle
/// Wrapper-/Elevation-/Variablen-Normalisierung (`resolved`) darf ein
/// `Allow` niemals zusätzlich ermöglichen — sonst würde z. B.
/// `LD_PRELOAD=/tmp/evil.so ls -la` (beliebiger Code über einen
/// Umgebungsvariablen-Angriff) oder `chroot /mnt ls -la` unter einer
/// harmlosen `Allow: ls *`-Regel AutoExec statt Confirm bekommen, obwohl
/// weder der Original- noch der nur-sudo-bereinigte Text matchen. Das
/// `sudo`-Stripping für Allow (Testfall 7, ADR 0002) bleibt davon
/// unberührt, da es über `stripped`, nicht `resolved`, läuft.
#[tokio::test]
async fn test_allow_rule_is_not_widened_by_full_wrapper_normalization() {
    let eng = engine(vec![glob_rule(
        "allow-ls",
        "ls *",
        RuleAction::Allow,
        Scope::Global,
        100,
    )]);
    for cmd in [
        "LD_PRELOAD=/tmp/evil.so ls -la",
        "chroot /mnt ls -la",
        "flock /tmp/l ls -la",
    ] {
        let decision = eng.evaluate(cmd, &ctx("srv1", &[])).await;
        assert_confirm(&decision);
    }
    // Gegenprobe: `sudo`-Stripping für Allow bleibt erhalten.
    let decision = eng.evaluate("sudo ls -la", &ctx("srv1", &[])).await;
    assert_auto_exec(&decision);
}

/// Regressionstest für den unabhängigen Review-Pass (Spec 0002): `-c`/`-e`-
/// Code-Flags anderer Skript-Interpreter (nicht nur `bash`/`sh`) müssen
/// denselben "als Ganzes behandeln, nie AutoExec"-Schutz bekommen wie
/// `bash -c "..."` — sonst würde `python3 -c "import os; os.system(...)"`
/// unter einer harmlosen `Allow: python3 *`-Regel beliebigen Code
/// automatisch ausführen.
#[tokio::test]
async fn test_interpreter_code_flag_is_treated_as_complex_script_block() {
    let eng = engine(vec![glob_rule(
        "allow-python",
        "python3 *",
        RuleAction::Allow,
        Scope::Global,
        100,
    )]);
    let decision = eng
        .evaluate(
            "python3 -c \"import os; os.system('rm -rf /')\"",
            &ctx("srv1", &[]),
        )
        .await;
    assert_confirm(&decision);
}

/// Regressionstest für den unabhängigen Review-Pass (Spec 0002, Abschnitt
/// 4.6, letzter Satz): eine `Deny`-Regel darf nicht dadurch umgangen werden
/// können, dass das verbotene Kommando in `bash -c "..."` gewickelt wird —
/// ADR 0001 verlangt weiterhin kein Sub-Parsing in mehrere Kommandos, aber
/// der `-c`-Inhalt selbst muss trotzdem gegen Deny/Blacklist geprüft
/// werden.
#[tokio::test]
async fn test_deny_rule_cannot_be_evaded_via_shell_c_wrapper() {
    let eng = engine(vec![glob_rule(
        "deny-docker",
        "docker *",
        RuleAction::Deny,
        Scope::Global,
        100,
    )]);
    for cmd in [
        "bash -c \"docker rm -f prod\"",
        "sudo bash -c \"docker rm -f prod\"",
        "sh -c 'env docker rm -f prod'",
    ] {
        let decision = eng.evaluate(cmd, &ctx("srv1", &[])).await;
        assert_deny(&decision);
    }
}

/// Gegenprobe zum vorigen Test: ein harmloser `bash -c`-Aufruf ohne
/// Deny-/Blacklist-Treffer im Argument bleibt weiterhin bei `Confirm` (nie
/// `AutoExec`, ADR 0001) — die neue Argument-Prüfung darf nur eskalieren,
/// nie ein `AutoExec` für den ganzen Block erzeugen.
#[tokio::test]
async fn test_benign_shell_c_wrapper_stays_at_confirm_not_autoexec() {
    let eng = engine(vec![glob_rule(
        "allow-ls",
        "ls *",
        RuleAction::Allow,
        Scope::Global,
        100,
    )]);
    let decision = eng
        .evaluate("bash -c \"cd /app && ls -la\"", &ctx("srv1", &[]))
        .await;
    assert_confirm(&decision);
}

/// `sudo bash -c "..."` / `bash -xc "..."` müssen wie ein einfaches
/// `bash -c "..."` als Ganzes-Skript-Block behandelt werden (ADR 0001) —
/// vorher griff der Guard nur, wenn "bash"/"sh"/... exakt das erste Wort
/// war, nicht nach einem `sudo`-Präfix bzw. bei kombinierten Kurz-Flags.
#[tokio::test]
async fn test_sudo_or_combined_flag_shell_c_is_treated_as_complex_script_block() {
    let eng = engine(vec![glob_rule(
        "allow-sudo",
        "sudo *",
        RuleAction::Allow,
        Scope::Global,
        100,
    )]);
    for cmd in [
        "sudo bash -c \"rm -rf /\"",
        "bash -xc \"rm -rf /\"",
        "sh -lc 'rm -rf /'",
    ] {
        let decision = eng.evaluate(cmd, &ctx("srv1", &[])).await;
        assert_confirm(&decision);
    }
}

// --- Spec 0037, Abschnitt 5/9: Policy-Ebenen (RuleOrigin) ------------------

/// Einfache In-Memory-`PolicySource`-Testimplementierung mit fest
/// vorgegebener `origin()` — Spec 0037, Abschnitt 9: "ergänze für diesen
/// Test eine einfache In-Memory-PolicySource-Testimplementierung mit
/// `origin() == Organization`" (hier generisch über `origin` gehalten,
/// deckt aber auch die `User`-Quelle in denselben Tests ab, statt einen
/// zweiten, praktisch identischen Typ zu duplizieren).
struct InMemoryPolicySource {
    origin: RuleOrigin,
    rules: Vec<Rule>,
}

#[async_trait]
impl PolicySource for InMemoryPolicySource {
    fn origin(&self) -> RuleOrigin {
        self.origin
    }

    async fn rules(&self) -> PolicySourceResult<Vec<Rule>> {
        Ok(self.rules.clone())
    }

    async fn watch(&self) -> PolicySourceResult<tokio::sync::watch::Receiver<Vec<Rule>>> {
        let (_tx, rx) = tokio::sync::watch::channel(self.rules.clone());
        Ok(rx)
    }
}

fn combined_engine(
    org_rules: Vec<Rule>,
    user_rules: Vec<Rule>,
) -> FilterEngine<CombinedPolicySource> {
    FilterEngine::new(CombinedPolicySource::new(vec![
        Arc::new(InMemoryPolicySource {
            origin: RuleOrigin::Organization,
            rules: org_rules,
        }),
        Arc::new(InMemoryPolicySource {
            origin: RuleOrigin::User,
            rules: user_rules,
        }),
    ]))
}

/// Spec 0037, Abschnitt 9: "Test für Allow(User) gegen Deny(Organization)
/// ... mit höherer Scope-Spezifität auf Nutzerseite." Bereits durch die
/// unveränderte Aktions-Tier-Reihenfolge (Spec 0002 Abschnitt 3: Deny vor
/// Allow) korrekt entschieden — dieser Test belegt, dass die neue
/// `RuleOrigin`-Sortierung diese bestehende Garantie nicht versehentlich
/// abschwächt.
#[tokio::test]
async fn test_organization_deny_global_beats_user_allow_server_despite_lower_specificity() {
    let server_id = ServerId::new();
    let eng = combined_engine(
        vec![glob_rule_with_origin(
            "org-deny-rm",
            "rm *",
            RuleAction::Deny,
            Scope::Global,
            0,
            RuleOrigin::Organization,
        )],
        vec![glob_rule_with_origin(
            "user-allow-rm",
            "rm *",
            RuleAction::Allow,
            Scope::Server(server_id),
            100,
            RuleOrigin::User,
        )],
    );

    let ctx = EvalContext {
        server_id,
        tags: Vec::new(),
    };
    let decision = eng.evaluate("rm -rf /tmp/x", &ctx).await;

    assert!(
        matches!(decision, Decision::Deny { .. }),
        "eine Organization-Deny-Regel darf durch eine spezifischere User-Allow-Regel nicht \
         aufgehoben werden, war: {decision:?}"
    );
}

/// Spec 0037, Abschnitt 5, Schritt 3 (der wörtlich in der Aufgabenstellung
/// genannte Fall): eine `Organization`-Confirm-Regel mit Global-Scope
/// gegen eine `User`-Allow-Regel mit Server-Scope (also spezifischer) —
/// Ergebnis muss `Confirm` sein, nicht `Allow`. Wie beim Deny-Test oben
/// bereits durch die unveränderte Aktions-Tier-Reihenfolge (Confirm vor
/// Allow) korrekt entschieden — auch hier: Regressionsschutz, dass
/// `RuleOrigin` diese Garantie nicht unterläuft.
#[tokio::test]
async fn test_organization_confirm_global_beats_user_allow_server_despite_lower_specificity() {
    let server_id = ServerId::new();
    let eng = combined_engine(
        vec![glob_rule_with_origin(
            "org-confirm-deploy",
            "deploy *",
            RuleAction::Confirm,
            Scope::Global,
            0,
            RuleOrigin::Organization,
        )],
        vec![glob_rule_with_origin(
            "user-allow-deploy",
            "deploy *",
            RuleAction::Allow,
            Scope::Server(server_id),
            100,
            RuleOrigin::User,
        )],
    );

    let ctx = EvalContext {
        server_id,
        tags: Vec::new(),
    };
    let decision = eng.evaluate("deploy prod", &ctx).await;

    assert!(
        matches!(decision, Decision::Confirm { .. }),
        "eine Organization-Confirm-Regel darf durch eine spezifischere User-Allow-Regel nicht \
         aufgehoben werden, war: {decision:?}"
    );
}

/// Die eigentlich *neue* Sortierebene aus Schritt 3 (im Unterschied zu den
/// beiden Tests oben, die schon allein durch die Aktions-Tier-Reihenfolge
/// entschieden wären): **innerhalb desselben** Aktions-Tiers (hier beide
/// `Confirm`) muss die `Organization`-Regel vor der `User`-Regel gewinnen
/// — unabhängig davon, dass die `User`-Regel den spezifischeren
/// Server-Scope trägt und ohne Schritt 3 (reine Scope-/Prioritäts-
/// Sortierung wie vor Spec 0037) zuerst geprüft würde.
#[tokio::test]
async fn test_organization_rule_wins_over_more_specific_user_rule_within_same_action_tier() {
    let server_id = ServerId::new();
    let eng = combined_engine(
        vec![glob_rule_with_origin(
            "org-confirm-restart",
            "restart *",
            RuleAction::Confirm,
            Scope::Global,
            0,
            RuleOrigin::Organization,
        )],
        vec![glob_rule_with_origin(
            "user-confirm-restart",
            "restart *",
            RuleAction::Confirm,
            Scope::Server(server_id),
            100,
            RuleOrigin::User,
        )],
    );

    let ctx = EvalContext {
        server_id,
        tags: Vec::new(),
    };
    let trace = eng.evaluate_explained("restart nginx", &ctx).await;

    assert!(matches!(trace.decision, Decision::Confirm { .. }));
    assert_eq!(
        trace.matched_rule,
        Some(RuleId("org-confirm-restart".to_string())),
        "die Organization-Regel muss greifen, nicht die spezifischere User-Regel — \
         tatsächlicher Trace: {trace:?}"
    );
    assert_eq!(trace.matched_rule_origin, Some(RuleOrigin::Organization));
}

/// `evaluate_explained` nennt korrekt die `RuleOrigin` der greifenden
/// Regel (Spec 0037, Abschnitt 10) — auch im einfachen Ein-Quellen-Fall
/// (keine `Organization`-Konkurrenz), damit der Regelfall (Community
/// Edition: nur die eine SQLite-`User`-Quelle) ebenfalls abgedeckt ist.
#[tokio::test]
async fn test_evaluate_explained_reports_user_origin_for_sole_sqlite_style_source() {
    let eng = engine(vec![glob_rule(
        "allow-ls",
        "ls *",
        RuleAction::Allow,
        Scope::Global,
        0,
    )]);

    let trace = eng.evaluate_explained("ls -la", &ctx("srv1", &[])).await;

    assert_eq!(trace.matched_rule_origin, Some(RuleOrigin::User));
}

// --- Spec 0060: Glob `*` überquert `/` nicht mehr (pfadförmige Muster) ---

/// Spec 0060, Testbarkeit: der Grundfall — `*` matcht weiterhin innerhalb
/// desselben Verzeichnisses.
#[tokio::test]
async fn test_path_shaped_allow_rule_matches_within_same_directory() {
    let eng = engine(vec![glob_rule(
        "allow-log",
        "cat /var/log/*",
        RuleAction::Allow,
        Scope::Global,
        0,
    )]);
    let decision = eng.evaluate("cat /var/log/syslog", &ctx("srv1", &[])).await;
    assert_auto_exec(&decision);
}

/// Spec 0060, Testbarkeit: `*` überquert keine `/`-Grenze mehr — ein
/// tieferes Unterverzeichnis matcht nicht mehr automatisch.
#[tokio::test]
async fn test_path_shaped_allow_rule_does_not_cross_into_subdirectory() {
    let eng = engine(vec![glob_rule(
        "allow-log",
        "cat /var/log/*",
        RuleAction::Allow,
        Scope::Global,
        0,
    )]);
    let decision = eng
        .evaluate("cat /var/log/sub/deep", &ctx("srv1", &[]))
        .await;
    assert_confirm(&decision);
}

/// Spec 0060, das titelgebende Problem: `Allow: cat /var/log/*` darf NICHT
/// mehr auf einen `../`-Ausbruch matchen.
#[tokio::test]
async fn test_path_shaped_allow_rule_rejects_the_headline_traversal_case() {
    let eng = engine(vec![glob_rule(
        "allow-log",
        "cat /var/log/*",
        RuleAction::Allow,
        Scope::Global,
        0,
    )]);
    let decision = eng
        .evaluate("cat /var/log/../../../etc/shadow", &ctx("srv1", &[]))
        .await;
    assert_confirm(&decision);
}

/// Spec 0060, Abschnitt 4 (adversarial, PFLICHT) — jeder erfundene
/// Umgehungsversuch gegen dieselbe `Allow: cat /var/log/*`-Regel muss zu
/// „kein Match" (Confirm/Deny, nie AutoExec) führen.
#[tokio::test]
async fn test_path_shaped_allow_rule_rejects_all_adversarial_traversal_variants() {
    let eng = engine(vec![glob_rule(
        "allow-log",
        "cat /var/log/*",
        RuleAction::Allow,
        Scope::Global,
        0,
    )]);
    let adversarial_commands = [
        // Direkte `../`-Ausbrüche, unterschiedlich tief.
        "cat /var/log/../../etc/shadow",
        "cat /var/log/../log/../../etc/passwd",
        // Einzelnes `..`-Segment OHNE eingebettetes `/` — der Fall, den
        // `literal_separator` allein NICHT abfängt (nur die vorgeschaltete
        // lexikalische Normalisierung tut das, s. `pattern.rs`).
        "cat /var/log/..",
        // Doppelte Slashes / `.`-Segmente dürfen nicht versehentlich einen
        // Ausbruch ermöglichen (werden zu einem harmlosen, aber
        // NICHT-matchenden Pfad normalisiert, da sie das Zielverzeichnis
        // verlassen).
        "cat /var//log/../../etc/shadow",
        "cat /var/log/./../../etc/shadow",
        // Groß-/Kleinschreibungstrick — Matching bleibt case-sensitiv,
        // ein anderer Groß-/Kleinschreibungs-Pfad matcht `/var/log/*`
        // ohnehin nicht (kein Bypass, aber zur Dokumentation mitgeprüft).
        "cat /VAR/LOG/../../etc/shadow",
        // Whitespace-Trick: zusätzliche Leerzeichen um den Pfad ändern
        // nichts an der Segment-Auflösung.
        "cat  /var/log/../../etc/shadow",
        // Relativer Ausbruch — `foo/bar/*`-artige bare relative Pfade
        // gelten ebenfalls als pfadförmig (s. `is_path_shaped_pattern`),
        // ein Ausbruch über mehrere `..` muss also auch hier scheitern.
        "cat var/log/../../etc/shadow",
    ];
    for cmd in adversarial_commands {
        let decision = eng.evaluate(cmd, &ctx("srv1", &[])).await;
        assert!(
            !matches!(decision, Decision::AutoExec),
            "adversarialer Fall darf NIE AutoExec ergeben: {cmd:?} -> {decision:?}"
        );
    }
}

/// Spec 0060, Abschnitt 4: ein Muster, das die „pfadförmig"-Erkennung
/// austricksen will, indem es KEIN `/` im Muster selbst trägt, aber
/// trotzdem versucht, über ein Kommando-Argument einen Pfad-artigen Treffer
/// zu erzwingen. Ein bewusst breiter, vom Regel-Autor selbst so gewählter
/// Nicht-Pfad-Glob (`cat *`) ist kein Umgehungsfall der Spec-0060-
/// Schutzmaßnahme (der Autor hat selbst uneingeschränkt erlaubt) — die
/// eigentliche Prüfung hier: ein Muster, das NUR knapp an der URL-Ausnahme
/// vorbeischrammt (z. B. ein Trick-Schema ohne echtes `://`), wird trotzdem
/// als pfadförmig erkannt und bleibt entsprechend strikt.
#[tokio::test]
async fn test_pattern_trying_to_evade_path_shaped_detection_stays_strict() {
    // `file:/var/log/*` enthält kein `://` (nur ein einzelner Doppelpunkt +
    // Slash) — die URL-Ausnahme greift NICHT, das Muster bleibt pfadförmig.
    let eng = engine(vec![glob_rule(
        "allow-trick",
        "cat file:/var/log/*",
        RuleAction::Allow,
        Scope::Global,
        0,
    )]);
    let decision = eng
        .evaluate("cat file:/var/log/../../etc/shadow", &ctx("srv1", &[]))
        .await;
    assert!(
        !matches!(decision, Decision::AutoExec),
        "ein Beinahe-URL-Trick darf die pfadförmig-Erkennung nicht umgehen: {decision:?}"
    );
}

/// Spec 0060, Abschnitt 3 / „Die Design-Entscheidung": Nicht-pfadförmige
/// Globs (z. B. über ein URL-artiges Kommando-Argument) behalten
/// unverändert das alte Verhalten — `*` überquert weiterhin `/`. Fehlt
/// dieser Regressionstest, könnte eine künftige Änderung versehentlich
/// `literal_separator` auf ALLE Globs anwenden und diesen Normalfall
/// brechen, ohne dass ein Test das auffinge.
#[tokio::test]
async fn test_non_path_shaped_glob_still_crosses_slash_boundary_unchanged() {
    let eng = engine(vec![glob_rule(
        "allow-url",
        "curl http://example.com/*",
        RuleAction::Allow,
        Scope::Global,
        0,
    )]);
    let decision = eng
        .evaluate("curl http://example.com/api/v1/resource", &ctx("srv1", &[]))
        .await;
    assert_auto_exec(&decision);
}

/// Spec 0060, Testbarkeit: das Regel-Test-Panel (0009) nutzt
/// `evaluate_explained` — dieselbe strengere Auswertung muss sich dort
/// identisch zeigen (kein separater Codepfad, der die Verschärfung
/// umgehen könnte).
#[tokio::test]
async fn test_evaluate_explained_reflects_the_stricter_path_shaped_matching() {
    let eng = engine(vec![glob_rule(
        "allow-log",
        "cat /var/log/*",
        RuleAction::Allow,
        Scope::Global,
        0,
    )]);
    let trace = eng
        .evaluate_explained("cat /var/log/../../../etc/shadow", &ctx("srv1", &[]))
        .await;
    assert!(
        !matches!(trace.decision, Decision::AutoExec),
        "Regel-Test-Panel muss den `../`-Ausbruch als kein-Match/Confirm zeigen: {trace:?}"
    );
    assert_ne!(
        trace.matched_rule,
        Some(RuleId("allow-log".to_string())),
        "die Allow-Regel darf für den Ausbruchsversuch nicht mehr als greifend gemeldet werden"
    );
}

// --- Spec 0060, spec-reviewer-Funde (ERHÖHT + adversarial): Quoting-/
// Escaping-Umgehung der lexikalischen Normalisierung, `://`-Skip-Lücke,
// Deny-Abschwächung ---------------------------------------------------

/// spec-reviewer-Fund 1 (KRITISCH): die rein lexikalische `..`-Erkennung
/// vergleicht rohe Textsegmente gegen das Literal `".."` — jede
/// Shell-Schreibweise, die NACH der Shell-Expansion `..` ergibt, übersteht
/// diesen Vergleich unverändert. Empirisch gegen die ungehärtete Fassung
/// verifiziert: jeder dieser Fälle ergab dort `AutoExec` statt `Confirm`.
#[tokio::test]
async fn test_path_shaped_allow_rule_rejects_shell_quoting_and_escaping_tricks() {
    let eng = engine(vec![glob_rule(
        "allow-tmp",
        "cat /tmp/**",
        RuleAction::Allow,
        Scope::Global,
        0,
    )]);
    let adversarial_commands = [
        r#"cat /tmp/\../\../etc/shadow"#,
        r#"cat /tmp/".."/".."/etc/shadow"#,
        r#"cat /tmp/'..'/'..'/etc/shadow"#,
        r#"cat /tmp/{..,..}/etc/shadow"#,
        r#"cat /tmp/.[.]/../etc/shadow"#,
    ];
    for cmd in adversarial_commands {
        let decision = eng.evaluate(cmd, &ctx("srv1", &[])).await;
        assert!(
            !matches!(decision, Decision::AutoExec),
            "Shell-Quoting/-Escaping darf die `..`-Erkennung nicht umgehen: {cmd:?} -> {decision:?}"
        );
    }
}

/// spec-reviewer-Fund 1: dieselbe Quoting-Erkennung greift auch für ein
/// einzelnes `*` (nicht nur `**`) — ein einzelnes maskiertes `..`-Segment
/// darf ebenfalls nicht mehr durchrutschen.
#[tokio::test]
async fn test_path_shaped_allow_rule_rejects_escaped_single_segment_traversal() {
    let eng = engine(vec![glob_rule(
        "allow-log",
        "cat /var/log/*",
        RuleAction::Allow,
        Scope::Global,
        0,
    )]);
    for cmd in [r#"cat /var/log/\.."#, r#"cat /var/log/".""#] {
        let decision = eng.evaluate(cmd, &ctx("srv1", &[])).await;
        assert!(
            !matches!(decision, Decision::AutoExec),
            "ein maskiertes `..`-Segment darf nicht matchen: {cmd:?} -> {decision:?}"
        );
    }
}

/// spec-reviewer-Fund 3: die `://`-Ausnahme aus der Muster-Klassifizierung
/// darf auf der Kommando-Seite NICHT gelten — ein Token mit zufällig
/// eingebettetem `://` (z. B. ein zuvor angelegtes Verzeichnis `x:`) darf
/// die `..`-Normalisierung nicht umgehen.
#[tokio::test]
async fn test_path_shaped_allow_rule_rejects_traversal_hidden_behind_embedded_url_syntax() {
    let eng = engine(vec![glob_rule(
        "allow-tmp",
        "cat /tmp/**",
        RuleAction::Allow,
        Scope::Global,
        0,
    )]);
    let decision = eng
        .evaluate("cat /tmp/x://../../../etc/shadow", &ctx("srv1", &[]))
        .await;
    assert!(
        !matches!(decision, Decision::AutoExec),
        "ein zufällig eingebettetes `://` darf die Normalisierung nicht umgehen: {decision:?}"
    );
}

/// spec-reviewer-Fund 4: eine bestehende Deny-Regel mit einem einzelnen
/// `*` darf durch Spec 0060 NIE schwächer werden — `matches_for_user_rule`
/// fällt für Deny/Confirm zusätzlich auf das alte, permissive Matching
/// zurück (Oder-Verknüpfung), sodass ein legitimer (kein Traversal-)
/// Unterverzeichnis-Zugriff weiterhin gedeckt bleibt.
#[tokio::test]
async fn test_path_shaped_deny_rule_still_matches_subdirectory_like_before_the_fix() {
    let eng = engine(vec![glob_rule(
        "deny-home",
        "rm /home/u/*",
        RuleAction::Deny,
        Scope::Global,
        0,
    )]);
    let decision = eng.evaluate("rm /home/u/sub/file", &ctx("srv1", &[])).await;
    assert_deny(&decision);
}

/// spec-reviewer-Fund 4, Gegenprobe: die Oder-Verknüpfung darf die
/// eigentliche Spec-0060-Verbesserung nicht rückgängig machen — ein
/// tatsächlicher `../`-Ausbruch bleibt für Deny/Confirm weiterhin
/// erkennbar (über das alte, permissive Matching, das den literalen
/// String ohnehin schon traf).
#[tokio::test]
async fn test_path_shaped_deny_rule_still_catches_traversal_via_permissive_fallback() {
    let eng = engine(vec![glob_rule(
        "deny-etc",
        "cat /etc/*",
        RuleAction::Deny,
        Scope::Global,
        0,
    )]);
    let decision = eng
        .evaluate("cat /etc/../etc/shadow", &ctx("srv1", &[]))
        .await;
    assert_deny(&decision);
}

/// spec-reviewer-Fund: nur `cmd`, nicht das Muster selbst zu normalisieren,
/// brach ein relatives pfadförmiges Muster (`./foo/*` matchte `./foo/x`
/// nicht mehr, weil `cmd` zu `foo/x` normalisiert wurde, das Muster aber
/// `./foo/*` blieb) — beide Seiten werden jetzt symmetrisch normalisiert.
#[tokio::test]
async fn test_path_shaped_allow_rule_with_explicit_relative_pattern_still_matches() {
    let eng = engine(vec![glob_rule(
        "allow-rel",
        "cat ./foo/*",
        RuleAction::Allow,
        Scope::Global,
        0,
    )]);
    let decision = eng.evaluate("cat ./foo/x", &ctx("srv1", &[])).await;
    assert_auto_exec(&decision);
}

// --- Spec 0077: Muster, die sich nicht übersetzen lassen --------------------

/// Spec 0077, T-7: Jedes Muster der Hard-Blacklist übersetzt mit genau der
/// Übersetzung, die `Pattern::matches` benutzt (`Glob::new`/`Regex::new`).
/// Ein Muster, das nicht übersetzt, passt still nie — dieser Test macht das
/// sichtbar. Den strengen Glob-Zweig benutzt die Hard-Blacklist nicht.
#[test]
fn test_spec_0077_t7_hard_blacklist_patterns_all_compile() {
    let patterns = blacklist::hard_blacklist();
    assert!(!patterns.is_empty());
    for pattern in patterns {
        let result = match pattern {
            Pattern::Exact(_) => Ok(()),
            Pattern::Glob(p) => globset::Glob::new(p).map(|_| ()).map_err(|e| e.to_string()),
            Pattern::Regex(p) => regex::Regex::new(p).map(|_| ()).map_err(|e| e.to_string()),
        };
        assert!(
            result.is_ok(),
            "Hard-Blacklist-Muster übersetzt nicht: {pattern:?}: {result:?}"
        );
    }
}

// --- Spec 0077, 3.2.2/6.3: Auswertung mit ungültigen Mustern ---------------

/// Aufzeichnung der ERROR-Ereignisse aus 3.2.2 (Spec 0077, T-A10).
///
/// Dasselbe Muster wie `ai_providers::test_support` und
/// `app_shell::orchestration`: **ein** globaler Subscriber pro Testprozess
/// (`Once`), der in einen thread-lokalen Puffer schreibt — mehrere
/// `set_global_default`-Aufrufe im selben Testbinary gewinnen sonst nur beim
/// ersten, und die übrigen Tests sähen nie ihre eigenen Zeilen.
///
/// Bewusst auf `Level::ERROR` begrenzt: 3.2.2 ist eine Aussage über
/// ERROR-Ereignisse, und `evaluate_explained` loggt das Kommando schon
/// heute auf INFO (`engine.rs`, Spec 0016). Ohne diese Grenze würde T-A10
/// dieses INFO-Ereignis mitlesen und über etwas urteilen, das nicht
/// Gegenstand dieser Spec ist.
mod pattern_error_log {
    thread_local! {
        static BUFFER: std::cell::RefCell<Vec<u8>> =
            const { std::cell::RefCell::new(Vec::new()) };
    }

    #[derive(Clone, Default)]
    pub(super) struct Writer;

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
    pub(super) fn start_recording() {
        static INIT: std::sync::Once = std::sync::Once::new();
        INIT.call_once(|| {
            let subscriber = tracing_subscriber::fmt()
                .json()
                .with_max_level(tracing::Level::ERROR)
                .with_writer(Writer)
                .finish();
            // `let _ =`: schlägt nur fehl, wenn schon ein globaler Default
            // gesetzt ist — dann ist es dank `Once` bereits dieser hier.
            let _ = tracing::subscriber::set_global_default(subscriber);
        });
        BUFFER.with(|b| b.borrow_mut().clear());
    }

    /// Die aufgezeichneten Ereignisse, eine JSON-Zeile je Ereignis. Alle
    /// sind ERROR-Ereignisse (s. `with_max_level` oben).
    pub(super) fn recorded_error_events() -> Vec<String> {
        BUFFER.with(|b| {
            String::from_utf8(b.borrow().clone())
                .expect("Log ist kein UTF-8")
                .lines()
                .map(str::to_string)
                .collect()
        })
    }
}

/// Belegt, dass genau für `rule_id` ein ERROR-Ereignis aus 3.2.2 vorliegt.
fn assert_pattern_error_logged(rule_id: &str) {
    let events = pattern_error_log::recorded_error_events();
    let hits: Vec<&String> = events
        .iter()
        .filter(|line| {
            line.contains("filter rule pattern does not compile")
                && line.contains(&format!("\"rule_id\":\"{rule_id}\""))
        })
        .collect();
    assert_eq!(
        hits.len(),
        1,
        "genau ein ERROR-Ereignis für Regel {rule_id} erwartet, \
         aufgezeichnet: {events:?}"
    );
}

/// Belegt, dass für `rule_id` **kein** ERROR-Ereignis aus 3.2.2 vorliegt —
/// eine gültige Regel wird nicht gemeldet.
fn assert_no_pattern_error_logged(rule_id: &str) {
    let events = pattern_error_log::recorded_error_events();
    assert!(
        !events
            .iter()
            .any(|line| line.contains(&format!("\"rule_id\":\"{rule_id}\""))),
        "unerwartetes ERROR-Ereignis für Regel {rule_id}: {events:?}"
    );
}

fn regex_rule(id: &str, regex: &str, action: RuleAction, priority: i32) -> Rule {
    Rule {
        id: RuleId(id.to_string()),
        pattern: Pattern::Regex(regex.to_string()),
        action,
        scope: Scope::Global,
        priority,
        origin: RuleOrigin::User,
    }
}

/// Spec 0077, T-A1: Allow `systemctl *` neben einer Deny-Regel, deren Regex
/// nicht übersetzt — die Entscheidung ist dieselbe wie mit der Allow-Regel
/// allein (**AutoExec**), und die ungültige Regel wird gemeldet.
///
/// Scheitert, wenn eine Ersatz-Eskalation auf Confirm/Deny eingebaut wird
/// (3.2.1, Entscheidung in §1) oder wenn das Log aus 3.2.2 fehlt.
#[tokio::test]
async fn test_spec_0077_ta1_invalid_deny_regex_leaves_decision_at_auto_exec_and_is_logged() {
    pattern_error_log::start_recording();
    let eng = engine(vec![
        glob_rule(
            "allow-systemctl",
            "systemctl *",
            RuleAction::Allow,
            Scope::Global,
            0,
        ),
        regex_rule("deny-broken", "^systemctl stop (.*", RuleAction::Deny, 100),
    ]);

    let decision = eng
        .evaluate("systemctl stop nginx", &ctx("srv1", &[]))
        .await;

    assert_auto_exec(&decision);
    assert_pattern_error_logged("deny-broken");
    assert_no_pattern_error_logged("allow-systemctl");
}

/// Spec 0077, T-A2: wie T-A1, aber mit einem Glob, der nicht übersetzt.
#[tokio::test]
async fn test_spec_0077_ta2_invalid_deny_glob_leaves_decision_at_auto_exec_and_is_logged() {
    pattern_error_log::start_recording();
    let eng = engine(vec![
        glob_rule(
            "allow-systemctl",
            "systemctl *",
            RuleAction::Allow,
            Scope::Global,
            0,
        ),
        glob_rule(
            "deny-broken",
            "systemctl [stop",
            RuleAction::Deny,
            Scope::Global,
            100,
        ),
    ]);

    let decision = eng
        .evaluate("systemctl stop nginx", &ctx("srv1", &[]))
        .await;

    assert_auto_exec(&decision);
    assert_pattern_error_logged("deny-broken");
}

/// Spec 0077, T-A3: wie T-A1, aber mit einer Confirm-Regel — auch sie wird
/// nicht durch eine Ersatz-Eskalation aufgewertet.
#[tokio::test]
async fn test_spec_0077_ta3_invalid_confirm_rule_leaves_decision_at_auto_exec_and_is_logged() {
    pattern_error_log::start_recording();
    let eng = engine(vec![
        glob_rule(
            "allow-systemctl",
            "systemctl *",
            RuleAction::Allow,
            Scope::Global,
            0,
        ),
        regex_rule(
            "confirm-broken",
            "^systemctl stop (.*",
            RuleAction::Confirm,
            100,
        ),
    ]);

    let decision = eng
        .evaluate("systemctl stop nginx", &ctx("srv1", &[]))
        .await;

    assert_auto_exec(&decision);
    assert_pattern_error_logged("confirm-broken");
}

/// Spec 0077, T-A4: Eine gültige Deny-Regel und eine ungültige Deny-Regel
/// mit **niedrigerer** Priorität; das Kommando passt auf die gültige.
///
/// Scheitert, wenn eine ungültige Regel die Auswertung abbricht oder die
/// übrigen Regeln überspringt — und, weil die ungültige Regel **hinter**
/// dem Treffer liegt, auch dann, wenn das Log in der Bucket-Schleife sitzt
/// statt vor der Auswertung (3.2.2): Die Bucket-Schleife kehrt beim ersten
/// Treffer zurück und käme an `deny-broken` nie vorbei.
#[tokio::test]
async fn test_spec_0077_ta4_valid_deny_still_bites_and_lower_priority_invalid_rule_is_logged() {
    pattern_error_log::start_recording();
    let eng = engine(vec![
        glob_rule(
            "allow-systemctl",
            "systemctl *",
            RuleAction::Allow,
            Scope::Global,
            0,
        ),
        glob_rule(
            "deny-valid",
            "systemctl stop *",
            RuleAction::Deny,
            Scope::Global,
            100,
        ),
        regex_rule("deny-broken", "^systemctl stop (.*", RuleAction::Deny, 1),
    ]);

    let trace = eng
        .evaluate_explained("systemctl stop nginx", &ctx("srv1", &[]))
        .await;

    assert_deny(&trace.decision);
    assert_eq!(
        trace.matched_rule.as_ref().map(|r| r.0.as_str()),
        Some("deny-valid"),
        "die gültige Regel muss greifen"
    );
    assert_pattern_error_logged("deny-broken");
}

/// Spec 0077, T-A5: Eine Allow-Regel mit ungültigem Muster allein führt nie
/// zu AutoExec — es bleibt beim Confirm „keine Regel", wie ohne die Regel.
#[tokio::test]
async fn test_spec_0077_ta5_invalid_allow_rule_alone_never_grants_auto_exec() {
    pattern_error_log::start_recording();
    let eng = engine(vec![glob_rule(
        "allow-broken",
        "ls [la",
        RuleAction::Allow,
        Scope::Global,
        0,
    )]);

    let decision = eng.evaluate("ls -la", &ctx("srv1", &[])).await;

    assert_confirm(&decision);
    assert_pattern_error_logged("allow-broken");
}

/// Spec 0077, T-A9: Ein Regex über dem Größenlimit der `regex`-
/// Voreinstellung verhält sich wie ein Syntaxfehler (kein eigenes Limit,
/// §8 Punkt 3).
#[tokio::test]
async fn test_spec_0077_ta9_regex_over_size_limit_behaves_like_a_syntax_error() {
    pattern_error_log::start_recording();
    let eng = engine(vec![
        glob_rule(
            "allow-systemctl",
            "systemctl *",
            RuleAction::Allow,
            Scope::Global,
            0,
        ),
        regex_rule("deny-toolarge", "a{1000}{1000}", RuleAction::Deny, 100),
    ]);

    let decision = eng
        .evaluate("systemctl stop nginx", &ctx("srv1", &[]))
        .await;

    assert_auto_exec(&decision);
    assert_pattern_error_logged("deny-toolarge");
}

/// Spec 0077, T-A10: Das Log aus 3.2.2 ist eine neue Datensenke — es trägt
/// die Regel, nie das Kommando. Hier mit einem Kommando, das ein Geheimnis
/// im Argument trägt.
///
/// Scheitert, wenn das Kommando (oder ein Teil davon) in ein
/// ERROR-Ereignis geschrieben wird. Geprüft werden nur ERROR-Ereignisse:
/// `evaluate_explained` loggt das Kommando schon heute auf INFO, und das
/// ist nicht Gegenstand dieser Spec (§6.3, T-A10).
#[tokio::test]
async fn test_spec_0077_ta10_pattern_error_log_names_the_rule_but_never_the_command() {
    pattern_error_log::start_recording();
    let eng = engine(vec![
        glob_rule(
            "allow-systemctl",
            "systemctl *",
            RuleAction::Allow,
            Scope::Global,
            0,
        ),
        regex_rule("deny-broken", "^systemctl stop (.*", RuleAction::Deny, 100),
    ]);

    let decision = eng
        .evaluate("systemctl stop nginx --password=hunter2", &ctx("srv1", &[]))
        .await;

    assert_auto_exec(&decision);
    assert_pattern_error_logged("deny-broken");

    let events = pattern_error_log::recorded_error_events();
    assert!(!events.is_empty(), "kein ERROR-Ereignis aufgezeichnet");
    for line in &events {
        assert!(
            !line.contains("hunter2"),
            "Geheimnis aus dem Kommando im ERROR-Log: {line}"
        );
        assert!(!line.contains("nginx"), "Kommando im ERROR-Log: {line}");
    }
}

/// Spec 0077, T-A12 (§1, Tabelle „Gemessen, zweiter Befund"): Ein
/// pfadförmiger Glob, bei dem nur **einer** der beiden Zweige übersetzt,
/// greift weiter über den Zweig, der übersetzt — „wie nicht vorhanden"
/// gilt nur für das, was nicht übersetzt (3.2.1).
///
/// Erster Fall: der **strenge** Zweig trägt (`rm /x/[a/../b` normalisiert
/// zu `rm /x/b`). Gegenbeweis geführt: rot gegen eine Fassung, die Regeln
/// mit ungültigem Muster vor der Auswertung verwirft.
#[tokio::test]
async fn test_spec_0077_ta12_single_branch_invalid_deny_still_bites_via_strict_branch() {
    pattern_error_log::start_recording();
    let eng = engine(vec![
        glob_rule("allow-rm", "rm *", RuleAction::Allow, Scope::Global, 0),
        glob_rule(
            "deny-half-broken",
            "rm /x/[a/../b",
            RuleAction::Deny,
            Scope::Global,
            100,
        ),
    ]);

    let trace = eng.evaluate_explained("rm /x/b", &ctx("srv1", &[])).await;

    assert_deny(&trace.decision);
    assert!(
        matches!(&trace.decision, Decision::Deny { code, .. } if code == "FILTER_RULE_DENY"),
        "erwartet FILTER_RULE_DENY, bekommen: {:?}",
        trace.decision
    );
    assert_eq!(
        trace.matched_rule.as_ref().map(|r| r.0.as_str()),
        Some("deny-half-broken")
    );
    assert_pattern_error_logged("deny-half-broken");
}

/// Spec 0077, T-A12, zweiter Fall: die andere Richtung — hier trägt der
/// **permissive** Zweig (`rm /x/[a/b]/../c` übersetzt roh, der strenge
/// Zweig scheitert an `rm /x/[a/c`). Gegenbeweis geführt.
#[tokio::test]
async fn test_spec_0077_ta12_single_branch_invalid_deny_still_bites_via_permissive_branch() {
    pattern_error_log::start_recording();
    let eng = engine(vec![
        glob_rule("allow-rm", "rm *", RuleAction::Allow, Scope::Global, 0),
        glob_rule(
            "deny-half-broken",
            "rm /x/[a/b]/../c",
            RuleAction::Deny,
            Scope::Global,
            100,
        ),
    ]);

    let trace = eng
        .evaluate_explained("rm /x/a/../c", &ctx("srv1", &[]))
        .await;

    assert_deny(&trace.decision);
    assert!(
        matches!(&trace.decision, Decision::Deny { code, .. } if code == "FILTER_RULE_DENY"),
        "erwartet FILTER_RULE_DENY, bekommen: {:?}",
        trace.decision
    );
    assert_eq!(
        trace.matched_rule.as_ref().map(|r| r.0.as_str()),
        Some("deny-half-broken")
    );
    assert_pattern_error_logged("deny-half-broken");
}

/// Spec 0077, T-A12, dritter Fall: dieselbe Regel, ein Kommando, auf das
/// **kein** Zweig passt → AutoExec wie heute, mit ERROR-Ereignis.
///
/// Scheitert, wenn eine Ersatz-Eskalation eingebaut wird oder das Log fehlt.
#[tokio::test]
async fn test_spec_0077_ta12_single_branch_invalid_deny_does_not_escalate_when_nothing_matches() {
    pattern_error_log::start_recording();
    let eng = engine(vec![
        glob_rule("allow-rm", "rm *", RuleAction::Allow, Scope::Global, 0),
        glob_rule(
            "deny-half-broken",
            "rm /x/[a/b]/../c",
            RuleAction::Deny,
            Scope::Global,
            100,
        ),
    ]);

    let decision = eng.evaluate("rm /x/c", &ctx("srv1", &[])).await;

    assert_auto_exec(&decision);
    assert_pattern_error_logged("deny-half-broken");
}

/// Spec 0077, T-A6 (Sicherheits-Invariante aus §5, „Nicht lockern"): Für
/// jede Kombination aus Aktion, Musterzustand und Mustertyp ist die
/// Entscheidung **dieselbe** wie vor der Änderung — jeweils neben einer
/// Allow-Regel, die passt.
///
/// Die erwarteten Werte sind die am unveränderten Stand gemessenen. Der
/// Test ist bewusst ein Invarianz-Test: Er ist vor und nach der Änderung
/// grün. Rot wird er, sobald die Auswertung doch angefasst wird — etwa
/// durch eine Ersatz-Eskalation (ungültig würde zu Confirm/Deny) oder durch
/// ein Verwerfen ungültiger Regeln (die Einzelzweig-Zeilen würden zu
/// AutoExec).
#[tokio::test]
async fn test_spec_0077_ta6_decision_matrix_is_unchanged_for_every_pattern_state() {
    // (Fall, Muster der Testregel, Kommando, erwartet je Aktion
    //  Allow / Confirm / Deny)
    #[derive(Debug)]
    struct Row {
        label: &'static str,
        pattern: Pattern,
        command: &'static str,
        expected_allow: &'static str,
        expected_confirm: &'static str,
        expected_deny: &'static str,
    }

    let rows = vec![
        Row {
            label: "glob, gültig passend",
            pattern: Pattern::Glob("rm /x/c".to_string()),
            command: "rm /x/c",
            expected_allow: "auto_exec",
            expected_confirm: "confirm",
            expected_deny: "deny",
        },
        Row {
            label: "glob, gültig nicht passend",
            pattern: Pattern::Glob("rm /y/*".to_string()),
            command: "rm /x/c",
            expected_allow: "auto_exec",
            expected_confirm: "auto_exec",
            expected_deny: "auto_exec",
        },
        Row {
            label: "glob, ungültig (beide Zweige)",
            pattern: Pattern::Glob("rm /x/[a/b/..".to_string()),
            command: "rm /x/c",
            expected_allow: "auto_exec",
            expected_confirm: "auto_exec",
            expected_deny: "auto_exec",
        },
        Row {
            label: "glob, nur strenger Zweig ungültig, permissiv passend",
            pattern: Pattern::Glob("rm /x/[a/b]/../c".to_string()),
            command: "rm /x/a/../c",
            // Allow verlangt den strengen Zweig (Spec 0060) — er trägt hier
            // nicht, die Regel matcht also nicht; die Basis-Allow-Regel
            // `rm *` entscheidet.
            expected_allow: "auto_exec",
            expected_confirm: "confirm",
            expected_deny: "deny",
        },
        Row {
            label: "glob, nur permissiver Zweig ungültig, streng passend",
            pattern: Pattern::Glob("rm /x/[a/../b".to_string()),
            command: "rm /x/b",
            expected_allow: "auto_exec",
            expected_confirm: "confirm",
            expected_deny: "deny",
        },
        Row {
            label: "regex, gültig passend",
            pattern: Pattern::Regex("^rm /x/c$".to_string()),
            command: "rm /x/c",
            expected_allow: "auto_exec",
            expected_confirm: "confirm",
            expected_deny: "deny",
        },
        Row {
            label: "regex, gültig nicht passend",
            pattern: Pattern::Regex("^rm /y/.*$".to_string()),
            command: "rm /x/c",
            expected_allow: "auto_exec",
            expected_confirm: "auto_exec",
            expected_deny: "auto_exec",
        },
        Row {
            label: "regex, ungültig",
            pattern: Pattern::Regex("^rm /x/(.*".to_string()),
            command: "rm /x/c",
            expected_allow: "auto_exec",
            expected_confirm: "auto_exec",
            expected_deny: "auto_exec",
        },
        // „nur in einem Zweig ungültig" entfällt für Regex: Ein Regex hat
        // keinen zweiten Zweig (§6.3, T-A6).
    ];

    for row in &rows {
        for (action, expected) in [
            (RuleAction::Allow, row.expected_allow),
            (RuleAction::Confirm, row.expected_confirm),
            (RuleAction::Deny, row.expected_deny),
        ] {
            let action_label = format!("{action:?}");
            let eng = engine(vec![
                glob_rule("allow-rm", "rm *", RuleAction::Allow, Scope::Global, 0),
                Rule {
                    id: RuleId("under-test".to_string()),
                    pattern: row.pattern.clone(),
                    action,
                    scope: Scope::Global,
                    priority: 100,
                    origin: RuleOrigin::User,
                },
            ]);

            let decision = eng.evaluate(row.command, &ctx("srv1", &[])).await;
            let actual = match &decision {
                Decision::AutoExec => "auto_exec",
                Decision::Confirm { .. } => "confirm",
                Decision::Deny { .. } => "deny",
            };
            assert_eq!(
                actual, expected,
                "{} / {action_label} / Kommando {:?}: {decision:?}",
                row.label, row.command
            );
        }
    }
}
