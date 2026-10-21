//! Explanation of a pending confirmation (issue #596, Spec 0011): which
//! segments of a chain existing rules already cover, rule suggestions for the
//! segments that are still uncovered, and per suggestion a verdict whether
//! accepting it makes the command run automatically.
//!
//! Everything is decided by the real filter engine (per-segment trace, and
//! candidate rules for the verdict) plus the read-only post-engine
//! escalations; there is no second parser and no second matcher here. Nothing
//! is stored.

use ssh_manager_core::filter::{
    split_top_level_segments, Decision, EvalContext, EvaluationTrace, Rule, RuleAction, RuleId,
    RuleOrigin, Scope, TraceKind,
};

use crate::dto::{
    pattern_from_parts, ConfirmationExplanationDto, CoveringRuleDto, ExplainedSuggestionDto,
    RuleDto, SegmentCoverageDto, SegmentCoverageState, SuggestionVerdictDto, VerdictReasonDto,
};
use crate::rule_suggestions::suggest_rule_patterns_excluding;
use crate::session::CommandEvaluator;

/// Reason codes that are not engine codes.
pub const REASON_NO_RULE: &str = "FILTER_NO_RULE_MATCHED";
pub const REASON_RULE_CONFIRM: &str = "FILTER_RULE_CONFIRM";
pub const REASON_RULE_DENY: &str = "FILTER_RULE_DENY";

/// The traces of the top-level segments of `trace`. A command that is not
/// split (one segment, ambiguous, empty) is its own single segment.
fn segment_traces(trace: &EvaluationTrace) -> Vec<&EvaluationTrace> {
    if trace.segment_index.is_some() {
        return vec![trace];
    }
    let segs: Vec<&EvaluationTrace> = trace
        .sub_command_traces
        .iter()
        .filter(|t| t.kind == TraceKind::Segment)
        .collect();
    if segs.is_empty() {
        vec![trace]
    } else {
        segs
    }
}

fn decision_code(decision: &Decision) -> Option<&str> {
    match decision {
        Decision::AutoExec => None,
        Decision::Confirm { code, .. } | Decision::Deny { code, .. } => Some(code),
    }
}

/// State of one segment trace plus its reason codes.
fn classify(trace: &EvaluationTrace) -> (SegmentCoverageState, Vec<String>) {
    if trace.decision == Decision::AutoExec {
        return (SegmentCoverageState::Covered, Vec::new());
    }
    let mut reasons: Vec<String> = trace.checks.clone();
    if trace.matched_rule.is_some() {
        reasons.push(
            if matches!(trace.decision, Decision::Deny { .. }) {
                REASON_RULE_DENY
            } else {
                REASON_RULE_CONFIRM
            }
            .to_string(),
        );
    } else if reasons.is_empty() {
        if let Decision::Deny { code, .. } = &trace.decision {
            reasons.push(code.clone());
        }
    }
    if reasons.is_empty() {
        (SegmentCoverageState::NeedsRule, reasons)
    } else {
        (SegmentCoverageState::Blocked, reasons)
    }
}

/// Segment texts matching the traces; the whole command as one segment if the
/// splitter and the trace disagree on the count.
fn segment_texts(command: &str, count: usize) -> Vec<String> {
    match split_top_level_segments(command.trim()) {
        Some(texts) if texts.len() == count => texts,
        _ => vec![command.trim().to_string()],
    }
}

fn reasons_of_verdict(trace: &EvaluationTrace) -> Vec<VerdictReasonDto> {
    let segs = segment_traces(trace);
    let mut out: Vec<VerdictReasonDto> = Vec::new();
    for (i, t) in segs.iter().enumerate() {
        let index = (segs.len() > 1).then_some(i);
        let (state, reasons) = classify(t);
        let codes = match state {
            SegmentCoverageState::Covered => continue,
            SegmentCoverageState::NeedsRule => vec![REASON_NO_RULE.to_string()],
            SegmentCoverageState::Blocked => reasons,
        };
        for code in codes {
            out.push(VerdictReasonDto {
                code,
                segment_index: index,
            });
        }
    }
    if out.is_empty() {
        if let Some(code) = decision_code(&trace.decision) {
            out.push(VerdictReasonDto {
                code: code.to_string(),
                segment_index: None,
            });
        }
    }
    out
}

/// Core of the explanation, independent of `Session`/stores (testable).
/// `stored` is the list of stored rules (to show the covering rule);
/// `escalation` returns the code of a post-engine escalation for a command.
pub async fn explain(
    evaluator: &dyn CommandEvaluator,
    ctx: &EvalContext,
    command: &str,
    stored: &[RuleDto],
    scope: Scope,
    priority: Option<i32>,
    escalation: &(dyn Fn(&str) -> Option<&'static str> + Sync),
) -> ConfirmationExplanationDto {
    let trace = evaluator.evaluate_explained(command, ctx).await;
    let seg_traces = segment_traces(&trace);
    let texts = segment_texts(command, seg_traces.len());
    let multi = seg_traces.len() > 1;

    let mut segments = Vec::new();
    let mut covered_texts = Vec::new();
    for (i, t) in seg_traces.iter().enumerate() {
        let (state, reasons) = classify(t);
        let text = texts.get(i).cloned().unwrap_or_default();
        let rule = t
            .matched_rule
            .as_ref()
            .and_then(|id| covering_rule(stored, id));
        if state == SegmentCoverageState::Covered {
            covered_texts.push(text.clone());
        }
        if multi || state == SegmentCoverageState::Blocked {
            segments.push(SegmentCoverageDto {
                index: i,
                text,
                state,
                rule: if state == SegmentCoverageState::Covered {
                    rule
                } else {
                    None
                },
                reasons,
            });
        }
    }

    let mut suggestions = Vec::new();
    for suggestion in suggest_rule_patterns_excluding(command, &covered_texts) {
        let candidates: Vec<Rule> = suggestion
            .patterns
            .iter()
            .enumerate()
            .map(|(n, p)| Rule {
                id: RuleId(format!("candidate-{n}")),
                pattern: pattern_from_parts(p.pattern_type, p.pattern_value.clone()),
                action: RuleAction::Allow,
                scope: scope.clone(),
                priority: priority.unwrap_or(0),
                origin: RuleOrigin::User,
            })
            .collect();
        let with = evaluator
            .evaluate_explained_with_candidates(command, ctx, &candidates)
            .await;
        let mut reasons = reasons_of_verdict(&with);
        let mut auto_exec = with.decision == Decision::AutoExec;
        if auto_exec {
            if let Some(code) = escalation(command) {
                auto_exec = false;
                reasons.push(VerdictReasonDto {
                    code: code.to_string(),
                    segment_index: None,
                });
            }
        }
        suggestions.push(ExplainedSuggestionDto {
            suggestion,
            verdict: SuggestionVerdictDto { auto_exec, reasons },
        });
    }

    ConfirmationExplanationDto {
        segments,
        suggestions,
    }
}

fn covering_rule(stored: &[RuleDto], id: &RuleId) -> Option<CoveringRuleDto> {
    stored
        .iter()
        .find(|r| &r.id == id)
        .map(|r| CoveringRuleDto {
            pattern_type: r.pattern_type,
            pattern_value: r.pattern_value.clone(),
        })
}

/// Explanation for a pending confirmation of `command` on `session`.
pub async fn explain_confirmation(
    session: &crate::session::Session,
    profile_store: &dyn ssh_manager_core::profiles::ProfileStore,
    stored: &[RuleDto],
    command: &str,
    scope: Scope,
    priority: Option<i32>,
) -> ConfirmationExplanationDto {
    let tags = profile_store
        .get_server(&session.server_id)
        .await
        .map(|s| s.tags)
        .unwrap_or_else(|_| session.tags.clone());
    let ctx = EvalContext {
        server_id: session.server_id,
        tags,
    };
    explain(
        session.filter_engine.as_ref(),
        &ctx,
        command,
        stored,
        scope,
        priority,
        &|cmd| crate::orchestration::state_independent_escalation_code(session, cmd),
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use ssh_manager_core::filter::{
        scope_applies, EffectiveScope, FilterEngine, Pattern, PolicyStore, ServerId,
    };

    struct StaticStore(Vec<Rule>);

    #[async_trait]
    impl PolicyStore for StaticStore {
        async fn rules_for(&self, scope: &EffectiveScope) -> Vec<Rule> {
            self.0
                .iter()
                .filter(|r| scope_applies(&r.scope, scope))
                .cloned()
                .collect()
        }
    }

    fn allow(id: &str, glob: &str) -> Rule {
        Rule {
            id: RuleId(id.to_string()),
            pattern: Pattern::Glob(glob.to_string()),
            action: RuleAction::Allow,
            scope: Scope::Global,
            priority: 0,
            origin: RuleOrigin::User,
        }
    }

    fn ctx() -> EvalContext {
        EvalContext {
            server_id: ServerId(uuid::Uuid::from_u128(1)),
            tags: Vec::new(),
        }
    }

    fn dto(rule: &Rule) -> RuleDto {
        RuleDto {
            id: rule.id.clone(),
            pattern_type: PatternType::Glob,
            pattern_value: match &rule.pattern {
                Pattern::Glob(g) => g.clone(),
                _ => unreachable!(),
            },
            action: rule.action.clone(),
            scope: rule.scope.clone(),
            priority: 0,
            pattern_error: None,
        }
    }

    use crate::dto::PatternType;

    async fn run(
        rules: Vec<Rule>,
        command: &str,
        scope: Scope,
        escalation: &(dyn Fn(&str) -> Option<&'static str> + Sync),
    ) -> ConfirmationExplanationDto {
        let stored: Vec<RuleDto> = rules.iter().map(dto).collect();
        let engine = FilterEngine::new(StaticStore(rules));
        explain(&engine, &ctx(), command, &stored, scope, None, escalation).await
    }

    fn none(_: &str) -> Option<&'static str> {
        None
    }

    #[tokio::test]
    async fn covered_segments_and_uncovered_only_suggestions() {
        let r = run(
            vec![allow("r1", "grep *")],
            "grep a f | grep -v b | wc -l",
            Scope::Global,
            &none,
        )
        .await;
        assert_eq!(r.segments.len(), 3);
        for i in [0, 1] {
            assert_eq!(r.segments[i].state, SegmentCoverageState::Covered);
            assert_eq!(r.segments[i].rule.as_ref().unwrap().pattern_value, "grep *");
        }
        assert_eq!(r.segments[2].state, SegmentCoverageState::NeedsRule);
        assert!(!r.suggestions.is_empty());
        for s in &r.suggestions {
            assert!(
                s.suggestion
                    .patterns
                    .iter()
                    .all(|p| p.pattern_value.starts_with("wc")),
                "{:?}",
                s.suggestion.patterns
            );
        }
        let tool = r
            .suggestions
            .iter()
            .find(|s| {
                s.suggestion
                    .patterns
                    .iter()
                    .any(|p| p.pattern_value == "wc *")
            })
            .expect("tool suggestion");
        assert!(tool.verdict.auto_exec);
        assert!(tool.verdict.reasons.is_empty());
    }

    #[tokio::test]
    async fn redirection_is_blocked_regardless_of_rules() {
        let r = run(
            vec![allow("r1", "grep *")],
            "grep a f > /tmp/out",
            Scope::Global,
            &none,
        )
        .await;
        assert_eq!(r.segments.len(), 1);
        assert_eq!(r.segments[0].state, SegmentCoverageState::Blocked);
        assert!(r.segments[0]
            .reasons
            .contains(&"FILTER_OUTPUT_REDIRECTION".to_string()));
        assert!(!r.suggestions.is_empty());
        for s in &r.suggestions {
            assert!(!s.verdict.auto_exec);
            assert!(s
                .verdict
                .reasons
                .iter()
                .any(|x| x.code == "FILTER_OUTPUT_REDIRECTION"));
        }
    }

    #[tokio::test]
    async fn single_covered_segment_has_no_list_and_unrelated_rule_missing_says_no_rule() {
        let r = run(vec![], "wc -l", Scope::Global, &none).await;
        assert!(r.segments.is_empty());
        let tool = r
            .suggestions
            .iter()
            .find(|s| {
                s.suggestion
                    .patterns
                    .iter()
                    .any(|p| p.pattern_value == "wc *")
            })
            .unwrap();
        assert!(tool.verdict.auto_exec);
    }

    #[tokio::test]
    async fn escalation_makes_verdict_still_ask() {
        let esc = |_: &str| Some("FILTER_RED_RISK_REQUIRES_CONFIRM");
        let r = run(vec![], "ls /tmp", Scope::Global, &esc).await;
        assert!(!r.suggestions.is_empty());
        for s in &r.suggestions {
            assert!(!s.verdict.auto_exec);
            assert_eq!(
                s.verdict.reasons.last().unwrap().code,
                "FILTER_RED_RISK_REQUIRES_CONFIRM"
            );
        }
    }

    #[tokio::test]
    async fn candidate_scoped_to_another_server_is_not_counted() {
        let other = Scope::Server(ServerId(uuid::Uuid::from_u128(99)));
        let r = run(vec![], "ls /tmp", other, &none).await;
        assert!(!r.suggestions.is_empty());
        for s in &r.suggestions {
            assert!(!s.verdict.auto_exec);
            assert!(s.verdict.reasons.iter().any(|x| x.code == REASON_NO_RULE));
        }
        let own = Scope::Server(ctx().server_id);
        let r = run(vec![], "ls /tmp", own, &none).await;
        assert!(r.suggestions.iter().any(|s| s.verdict.auto_exec));
    }

    #[tokio::test]
    async fn adversarial_inputs_never_get_auto_verdict() {
        for command in [
            "echo $(whoami)",
            "echo `whoami`",
            "bash -c 'ls /tmp'",
            "eval ls",
            "ls | sh",
            "ls /tmp > /tmp/x",
            "cat /tmp/a $(id)",
        ] {
            let r = run(vec![], command, Scope::Global, &none).await;
            for s in &r.suggestions {
                assert!(
                    !s.verdict.auto_exec,
                    "{command}: {:?}",
                    s.suggestion.patterns
                );
                assert!(!s.verdict.reasons.is_empty(), "{command}");
            }
        }
    }

    #[tokio::test]
    async fn rule_that_asks_is_reported() {
        let mut ask = allow("r1", "rm *");
        ask.action = RuleAction::Confirm;
        let r = run(vec![ask], "ls | rm x", Scope::Global, &none).await;
        let seg = &r.segments[1];
        assert_eq!(seg.state, SegmentCoverageState::Blocked);
        assert!(seg.reasons.contains(&REASON_RULE_CONFIRM.to_string()));
    }
}
