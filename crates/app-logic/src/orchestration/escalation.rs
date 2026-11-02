//! The post-engine escalation chain for AI and MCP actions, once (issue #826).
//!
//! [`escalate`] is a pure function: engine decision + [`EscalationFacts`] in,
//! [`Escalation`] out. It combines decisions only through
//! [`Decision::stricter`], so it can never return a decision less strict than
//! its input. Reading session state ([`EscalationFacts`] is built by the
//! caller) and consuming the one-shot flags (named in [`Escalation`]) happen
//! outside, so the decision path and the read-only rule-suggestion verdict
//! share this one function.

use ssh_manager_core::filter::Decision;
use ssh_manager_core::risk::CommandCheckFinding;

use super::action_exec::INJECTION_REASON_MARKER;

/// Every input the chain reads. Plain data, no session access.
#[derive(Debug, Clone, Default)]
pub(crate) struct EscalationFacts {
    /// Secret-path read finding (Spec 0068, part 2).
    pub secret_path: Option<CommandCheckFinding>,
    /// `sftp-server` invocation finding (ADR 0058 §8).
    pub sftp_server: Option<CommandCheckFinding>,
    /// Red-risk reason; `None` when the setting is off or nothing is red
    /// (Spec 0092, A2).
    pub red_risk_reason: Option<String>,
    /// Untrusted content was ingested and the post-ingest policy escalates
    /// this action (Spec 0039).
    pub post_ingest_escalates: bool,
    /// One-shot flag: injection suspected (value at read time).
    pub injection_suspected: bool,
    /// Reason the checker gave for the suspicion.
    pub injection_reason: Option<String>,
    /// One-shot flag: the injection check did not run (issue #102).
    pub injection_check_unavailable: bool,
    /// The action comes from MCP (Spec 0028, §5).
    pub mcp_origin: bool,
    /// The action uses the stored sudo password (Spec 0018).
    pub uses_stored_sudo_password: bool,
    /// The outgoing transform marks the text (issue #815).
    pub transform_marks_text: bool,
    /// An earlier action of the same AI response was rejected (Spec 0068, 4).
    pub earlier_rejection: bool,
}

/// Result of [`escalate`].
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Escalation {
    pub decision: Decision,
    /// Code of the step that turned `AutoExec` into `Confirm`, if any.
    pub code: Option<&'static str>,
    /// The caller must reset `injection_suspected` (and take its reason).
    pub consume_injection_suspected: bool,
    /// The caller must reset `injection_check_unavailable`.
    pub consume_injection_check_unavailable: bool,
}

const INJECTION_CODE: &str = "FILTER_INJECTION_SUSPECTED_REQUIRES_CONFIRM";
const NOT_ASSESSABLE_CODE: &str = "FILTER_COMMAND_NOT_ASSESSABLE_REQUIRES_CONFIRM";

fn with_reason(text: String, reason: Option<&str>) -> String {
    match reason {
        Some(r) if !r.is_empty() => format!("{text}{INJECTION_REASON_MARKER}{r}"),
        _ => text,
    }
}

/// Combines the current decision with a step's candidate via
/// [`Decision::stricter`]; records the code when this step is the one that
/// leaves `AutoExec`.
fn apply(out: &mut Escalation, reason: String, code: &'static str) {
    let was_auto = matches!(out.decision, Decision::AutoExec);
    let candidate = Decision::Confirm {
        reason,
        code: code.to_string(),
    };
    let current = std::mem::replace(&mut out.decision, Decision::AutoExec);
    out.decision = current.stricter(candidate);
    if was_auto && !matches!(out.decision, Decision::AutoExec) {
        out.code = Some(code);
    }
}

/// Reason text and code when an injection suspicion is merely *read* by a
/// step that has its own finding (`found`): the more alarming reason shows.
fn injection_read_step(found: &str, facts: &EscalationFacts) -> (String, &'static str) {
    (
        with_reason(
            format!(
                "Möglicher Versuch, Anweisungen über Serverinhalt einzuschleusen, \
                 erkannt; außerdem: {found} – erfordert Bestätigung"
            ),
            facts.injection_reason.as_deref(),
        ),
        INJECTION_CODE,
    )
}

/// Secret-path / `sftp-server` step. The injection flag is only read here.
fn finding_step(
    finding: CommandCheckFinding,
    match_code: &'static str,
    facts: &EscalationFacts,
) -> (String, &'static str) {
    let reason = finding.reason();
    if facts.injection_suspected {
        injection_read_step(reason, facts)
    } else if finding.is_not_assessable() {
        (
            format!("{reason} – erfordert immer Bestätigung"),
            NOT_ASSESSABLE_CODE,
        )
    } else {
        (
            format!("{reason} – erfordert immer Bestätigung"),
            match_code,
        )
    }
}

/// Applies the chain to the engine decision. Pure; escalation only.
///
/// A `Confirm`/`Deny` input is returned unchanged (nothing to escalate, no
/// flag consumed). For `AutoExec` the steps run in order; each applies only
/// while the decision is still `AutoExec`, and a one-shot flag is consumed
/// only by the step that actually escalated.
pub(crate) fn escalate(engine: Decision, facts: &EscalationFacts) -> Escalation {
    let mut out = Escalation {
        decision: engine,
        code: None,
        consume_injection_suspected: false,
        consume_injection_check_unavailable: false,
    };
    let auto = |out: &Escalation| matches!(out.decision, Decision::AutoExec);

    // 1. Secret-path read.
    if auto(&out) {
        if let Some(f) = facts.secret_path {
            let (r, c) = finding_step(f, "FILTER_SECRET_PATH_READ_REQUIRES_CONFIRM", facts);
            apply(&mut out, r, c);
        }
    }
    // 2. `sftp-server` invocation.
    if auto(&out) {
        if let Some(f) = facts.sftp_server {
            let (r, c) = finding_step(f, "FILTER_SFTP_SERVER_REQUIRES_CONFIRM", facts);
            apply(&mut out, r, c);
        }
    }
    // 3. Red risk.
    if auto(&out) {
        if let Some(reason) = facts.red_risk_reason.as_deref() {
            if facts.injection_suspected {
                let (r, c) = injection_read_step(reason, facts);
                apply(&mut out, r, c);
            } else {
                apply(
                    &mut out,
                    format!("{reason} – erfordert immer Bestätigung"),
                    "FILTER_RED_RISK_REQUIRES_CONFIRM",
                );
            }
        }
    }
    // 4. Post-ingest policy.
    if auto(&out) && facts.post_ingest_escalates {
        apply(
            &mut out,
            "Serverinhalt wurde in dieser Sitzung bereits eingelesen – erfordert Bestätigung"
                .to_string(),
            "FILTER_POST_INGEST_REQUIRES_CONFIRM",
        );
    }
    // 5. Injection suspected (consumes the flag).
    if auto(&out) && facts.injection_suspected {
        out.consume_injection_suspected = true;
        apply(
            &mut out,
            with_reason(
                "Möglicher Versuch, Anweisungen über Serverinhalt einzuschleusen, erkannt – \
                 erfordert Bestätigung"
                    .to_string(),
                facts.injection_reason.as_deref(),
            ),
            INJECTION_CODE,
        );
    }
    // 6. Injection check unavailable (consumes the flag).
    if auto(&out) && facts.injection_check_unavailable {
        out.consume_injection_check_unavailable = true;
        apply(
            &mut out,
            "Die Prüfung auf eingeschleuste Anweisungen konnte nicht ausgeführt werden – \
             erfordert Bestätigung"
                .to_string(),
            "FILTER_INJECTION_CHECK_UNAVAILABLE_REQUIRES_CONFIRM",
        );
    }
    // 7. MCP origin.
    if auto(&out) && facts.mcp_origin {
        apply(
            &mut out,
            "Über MCP (externes Tool) angefragt – erfordert immer Bestätigung".to_string(),
            "FILTER_MCP_ORIGIN_REQUIRES_CONFIRM",
        );
    }
    // 8. Stored sudo password.
    if auto(&out) && facts.uses_stored_sudo_password {
        apply(
            &mut out,
            "Verwendet das hinterlegte Sudo-Passwort – erfordert immer Bestätigung".to_string(),
            "FILTER_SUDO_PASSWORD_REQUIRES_CONFIRM",
        );
    }
    // 9. Outgoing transform inserts a stored secret (issue #815).
    if auto(&out) && facts.transform_marks_text {
        apply(
            &mut out,
            "Fügt vor dem Senden einen gespeicherten geheimen Wert ein – erfordert immer Bestätigung"
                .to_string(),
            "FILTER_INSERTED_SECRET_REQUIRES_CONFIRM",
        );
    }
    // 10. Earlier rejection in the same AI response.
    if auto(&out) && facts.earlier_rejection {
        apply(
            &mut out,
            "Eine vorherige Aktion dieser Antwort wurde abgelehnt – erfordert Bestätigung"
                .to_string(),
            "FILTER_EARLIER_ACTION_REJECTED_REQUIRES_CONFIRM",
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn finding() -> impl Strategy<Value = Option<CommandCheckFinding>> {
        prop_oneof![
            Just(None),
            Just(Some(CommandCheckFinding::Match("hit"))),
            Just(Some(CommandCheckFinding::NotAssessable("too long"))),
        ]
    }

    fn facts() -> impl Strategy<Value = EscalationFacts> {
        (
            (finding(), finding(), proptest::option::of("[a-z ]{0,8}")),
            proptest::array::uniform8(any::<bool>()),
            proptest::option::of("[a-z ]{0,8}"),
        )
            .prop_map(|((secret, sftp, red), b, inj_reason)| EscalationFacts {
                secret_path: secret,
                sftp_server: sftp,
                red_risk_reason: red,
                post_ingest_escalates: b[0],
                injection_suspected: b[1],
                injection_reason: inj_reason,
                injection_check_unavailable: b[2],
                mcp_origin: b[3],
                uses_stored_sudo_password: b[4],
                transform_marks_text: b[5],
                earlier_rejection: b[6],
            })
    }

    fn decision() -> impl Strategy<Value = Decision> {
        prop_oneof![
            Just(Decision::AutoExec),
            "[A-Z_]{1,6}".prop_map(|c| Decision::Confirm {
                reason: "r".into(),
                code: c
            }),
            "[A-Z_]{1,6}".prop_map(|c| Decision::Deny {
                reason: "r".into(),
                code: c
            }),
        ]
    }

    fn severity(d: &Decision) -> u8 {
        match d {
            Decision::AutoExec => 0,
            Decision::Confirm { .. } => 1,
            Decision::Deny { .. } => 2,
        }
    }

    proptest! {
        /// Never less strict than the input; `Confirm`/`Deny` come back
        /// unchanged, with no code and no flag consumed.
        #[test]
        fn escalate_never_weakens(engine in decision(), f in facts()) {
            let out = escalate(engine.clone(), &f);
            prop_assert!(severity(&out.decision) >= severity(&engine));
            if !matches!(engine, Decision::AutoExec) {
                prop_assert_eq!(&out.decision, &engine);
                prop_assert_eq!(out.code, None);
                prop_assert!(!out.consume_injection_suspected);
                prop_assert!(!out.consume_injection_check_unavailable);
            }
            // A named code means the decision left `AutoExec`, and vice versa.
            prop_assert_eq!(
                out.code.is_some(),
                matches!(engine, Decision::AutoExec) && !matches!(out.decision, Decision::AutoExec)
            );
        }
    }

    #[test]
    fn stricter_orders_and_first_wins_on_tie() {
        let c = |code: &str| Decision::Confirm {
            reason: String::new(),
            code: code.into(),
        };
        let d = |code: &str| Decision::Deny {
            reason: String::new(),
            code: code.into(),
        };
        assert_eq!(Decision::AutoExec.stricter(c("A")), c("A"));
        assert_eq!(c("A").stricter(Decision::AutoExec), c("A"));
        assert_eq!(c("A").stricter(c("B")), c("A"));
        assert_eq!(c("A").stricter(d("B")), d("B"));
        assert_eq!(d("A").stricter(c("B")), d("A"));
        assert_eq!(d("A").stricter(d("B")), d("A"));
    }

    /// A one-shot flag is consumed only by its own step: an earlier step
    /// that already escalated leaves it set.
    #[test]
    fn flag_consumed_only_when_its_step_escalated() {
        let both = EscalationFacts {
            injection_suspected: true,
            injection_check_unavailable: true,
            ..Default::default()
        };
        let out = escalate(Decision::AutoExec, &both);
        assert_eq!(out.code, Some(INJECTION_CODE));
        assert!(out.consume_injection_suspected);
        assert!(!out.consume_injection_check_unavailable);

        let earlier = EscalationFacts {
            secret_path: Some(CommandCheckFinding::Match("hit")),
            ..both.clone()
        };
        let out = escalate(Decision::AutoExec, &earlier);
        assert_eq!(out.code, Some(INJECTION_CODE)); // read-only injection reason
        assert!(!out.consume_injection_suspected);
        assert!(!out.consume_injection_check_unavailable);

        let mcp_first = EscalationFacts {
            mcp_origin: true,
            injection_check_unavailable: true,
            post_ingest_escalates: true,
            ..Default::default()
        };
        let out = escalate(Decision::AutoExec, &mcp_first);
        assert_eq!(out.code, Some("FILTER_POST_INGEST_REQUIRES_CONFIRM"));
        assert!(!out.consume_injection_check_unavailable);

        let deny = Decision::Deny {
            reason: "x".into(),
            code: "X".into(),
        };
        let out = escalate(deny.clone(), &both);
        assert_eq!(out.decision, deny);
        assert!(!out.consume_injection_suspected);
    }
}
