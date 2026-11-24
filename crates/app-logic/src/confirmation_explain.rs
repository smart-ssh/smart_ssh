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
    scope_applies, split_top_level_segments, Decision, EffectiveScope, EvalContext,
    EvaluationTrace, Rule, RuleAction, RuleId, RuleOrigin, Scope, TraceKind,
};

use crate::dto::{
    AutoExecRuleDto, ConfirmationExplanationDto, CoveringRuleDto, ExplainedSuggestionDto, RuleDto,
    SegmentCoverageDto, SegmentCoverageState, SegmentRuleSuggestionsDto, SegmentSuggestionDto,
    SegmentVerdictDto, StagedRuleDto, SuggestionVerdictDto, VerdictReasonDto,
};
use crate::filter_rules::pattern_from_parts;
use crate::rule_suggestions::{suggest_rule_patterns_excluding, suggest_segment_patterns};
use crate::session::CommandEvaluator;

/// Reason codes that are not engine codes.
pub const REASON_NO_RULE: &str = "FILTER_NO_RULE_MATCHED";
pub const REASON_RULE_CONFIRM: &str = "FILTER_RULE_CONFIRM";
pub const REASON_RULE_DENY: &str = "FILTER_RULE_DENY";

/// The traces of the top-level segments of `trace`. A command that is not
/// split (one segment, ambiguous, empty) is its own single segment.
pub(crate) fn segment_traces(trace: &EvaluationTrace) -> Vec<&EvaluationTrace> {
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

    // Post-engine escalation, attributed per segment text; if no segment
    // hits but the whole command does, it applies to every covered segment.
    let mut esc: Vec<Option<&'static str>> = texts.iter().map(|t| escalation(t)).collect();
    if esc.iter().all(Option::is_none) {
        if let Some(code) = escalation(command) {
            esc = vec![Some(code); texts.len()];
        }
    }

    let mut segments = Vec::new();
    let mut covered_texts = Vec::new();
    for (i, t) in seg_traces.iter().enumerate() {
        let (mut state, mut reasons) = classify(t);
        let text = texts.get(i).cloned().unwrap_or_default();
        let escalated = if state == SegmentCoverageState::Covered {
            esc.get(i).copied().flatten()
        } else {
            None
        };
        let rule = t
            .matched_rule
            .as_ref()
            .and_then(|id| covering_rule(stored, id));
        if state == SegmentCoverageState::Covered {
            covered_texts.push(text.clone());
        }
        if let Some(code) = escalated {
            state = SegmentCoverageState::Blocked;
            reasons.push(code.to_string());
        }
        // Issue #1357: the list is shown for a single command too.
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

/// Reason code: the scope chosen for a suggestion does not apply to the
/// session (a tag the server does not have, another server), so the rule
/// would never match here.
pub const REASON_SCOPE_NOT_APPLICABLE: &str = "RULE_SCOPE_NOT_APPLICABLE";

/// Code of an unknown segment index.
pub const SEGMENT_INDEX_OUT_OF_RANGE: &str = "SEGMENT_INDEX_OUT_OF_RANGE";

fn candidate_rules(
    prefix: &str,
    patterns: &[crate::dto::SuggestionPatternDto],
    scope: &Scope,
    priority: i32,
) -> Vec<Rule> {
    patterns
        .iter()
        .enumerate()
        .map(|(n, p)| Rule {
            id: RuleId(format!("{prefix}-{n}")),
            pattern: pattern_from_parts(p.pattern_type, p.pattern_value.clone()),
            action: RuleAction::Allow,
            scope: scope.clone(),
            priority,
            origin: RuleOrigin::User,
        })
        .collect()
}

/// Issue #1356, Spec 0011: suggestions for ONE segment of `command` (index as
/// in [`SegmentCoverageDto::index`]) in `scope`, given the rules the user has
/// staged. Read-only; verdicts come only from candidate evaluation by the
/// engine plus the read-only post-engine escalations.
///
/// A segment that always asks (state `blocked`) gets no suggestions, only its
/// reasons. `Err(())` if the index does not exist or a staged rule has an
/// invalid pattern (nothing can be said then).
#[allow(clippy::too_many_arguments)]
pub async fn explain_segment_rules(
    evaluator: &dyn CommandEvaluator,
    ctx: &EvalContext,
    command: &str,
    segment_index: usize,
    scope: Scope,
    staged: &[StagedRuleDto],
    escalation: &(dyn Fn(&str) -> Option<&'static str> + Sync),
) -> Result<SegmentRuleSuggestionsDto, crate::error::CommandError> {
    let mut staged_candidates: Vec<Rule> = Vec::new();
    for (n, rule) in staged.iter().enumerate() {
        let cands = candidate_rules(&format!("staged-{n}"), &rule.patterns, &rule.scope, 0);
        for c in &cands {
            if c.pattern.validate().is_err() {
                return Err(crate::error::CommandError::with_code(
                    "invalid pattern in a staged rule",
                    "INVALID_PATTERN",
                ));
            }
        }
        staged_candidates.extend(cands.into_iter().enumerate().map(|(i, mut c)| {
            c.id = RuleId(format!("staged-{n}-{i}"));
            c
        }));
    }

    let base = evaluator
        .evaluate_explained_with_candidates(command, ctx, &staged_candidates)
        .await;
    let seg_traces = segment_traces(&base);
    let texts = segment_texts(command, seg_traces.len());
    let (Some(seg_trace), Some(text)) = (seg_traces.get(segment_index), texts.get(segment_index))
    else {
        return Err(crate::error::CommandError::with_code(
            format!("segment index {segment_index} out of range"),
            SEGMENT_INDEX_OUT_OF_RANGE,
        ));
    };

    let (state, mut blocked) = classify(seg_trace);
    // Escalation attributed to this segment text; if no segment hits but the
    // whole command does, it applies to every covered segment (as in
    // `explain`).
    let seg_escalation = |t: &str| -> Option<&'static str> {
        escalation(t).or_else(|| {
            let any_segment_hit = texts.iter().any(|x| escalation(x).is_some());
            if any_segment_hit {
                None
            } else {
                escalation(command)
            }
        })
    };
    let mut is_blocked = state == SegmentCoverageState::Blocked;
    if state == SegmentCoverageState::Covered {
        if let Some(code) = seg_escalation(text) {
            is_blocked = true;
            blocked.push(code.to_string());
        }
    }
    if is_blocked {
        return Ok(SegmentRuleSuggestionsDto {
            segment_index,
            segment_text: text.clone(),
            blocked_reasons: blocked,
            suggestions: Vec::new(),
        });
    }

    let effective = EffectiveScope::from(ctx);
    let scope_ok = scope_applies(&scope, &effective);
    let mut out = Vec::new();
    for suggestion in suggest_segment_patterns(text) {
        let mut candidates = staged_candidates.clone();
        candidates.extend(candidate_rules(
            "candidate",
            &suggestion.patterns,
            &scope,
            0,
        ));
        let with = evaluator
            .evaluate_explained_with_candidates(command, ctx, &candidates)
            .await;

        // Command verdict: only the engine's decision (plus escalation).
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

        // Segment verdict.
        let with_segs = segment_traces(&with);
        let (covered, seg_reasons) = if !scope_ok {
            (false, vec![REASON_SCOPE_NOT_APPLICABLE.to_string()])
        } else {
            match with_segs.get(segment_index) {
                None => (false, vec![REASON_NO_RULE.to_string()]),
                Some(t) => {
                    let (st, mut rs) = classify(t);
                    match st {
                        SegmentCoverageState::Covered => match seg_escalation(text) {
                            Some(code) => (false, vec![code.to_string()]),
                            None => (true, Vec::new()),
                        },
                        SegmentCoverageState::NeedsRule => {
                            (false, vec![REASON_NO_RULE.to_string()])
                        }
                        SegmentCoverageState::Blocked => {
                            rs.dedup();
                            (false, rs)
                        }
                    }
                }
            }
        };
        out.push(SegmentSuggestionDto {
            suggestion,
            segment_verdict: SegmentVerdictDto {
                covered,
                reasons: seg_reasons,
            },
            command_verdict: SuggestionVerdictDto { auto_exec, reasons },
        });
    }
    Ok(SegmentRuleSuggestionsDto {
        segment_index,
        segment_text: text.clone(),
        blocked_reasons: Vec::new(),
        suggestions: out,
    })
}

/// The rules that covered the segments of an auto-executed command (issue
/// #696), one entry per distinct rule in order of first use. Built from the
/// trace of the evaluation that made the decision; the stored rules are only
/// looked up to name pattern and scope, nothing is evaluated again. A segment
/// without a matched rule (or whose rule was deleted meanwhile) is omitted.
pub async fn auto_exec_rules(
    evaluator: &dyn CommandEvaluator,
    ctx: &EvalContext,
    trace: &EvaluationTrace,
) -> Vec<AutoExecRuleDto> {
    let segs = segment_traces(trace);
    let ids: Vec<RuleId> = segs.iter().filter_map(|t| t.matched_rule.clone()).collect();
    if ids.is_empty() {
        return Vec::new();
    }
    let rules = evaluator.rules_by_ids(ctx, &ids).await;
    let mut out: Vec<AutoExecRuleDto> = Vec::new();
    for (i, t) in segs.iter().enumerate() {
        let Some(id) = t.matched_rule.as_ref() else {
            continue;
        };
        if let Some(existing) = out.iter_mut().find(|r| &r.rule_id == id) {
            existing.segment_indices.push(i);
            continue;
        }
        if let Some(rule) = rules.iter().find(|r| &r.id == id) {
            out.push(AutoExecRuleDto {
                rule_id: rule.id.clone(),
                pattern_type: crate::dto::PatternType::from(&rule.pattern),
                pattern_value: rule.pattern.display_text().to_string(),
                scope: rule.scope.clone(),
                segment_indices: vec![i],
            });
        }
    }
    out
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

/// Issue #689: logs a failed coverage lookup. Never contains the command
/// text — only its length, the error code and the session id.
pub fn log_explain_failure(
    err: &crate::error::CommandError,
    session_id: crate::state::SessionId,
    command: &str,
    command_name: &str,
) {
    tracing::warn!(
        error_code = err.code.unwrap_or("none"),
        session_id = ?session_id,
        command_len = command.len(),
        "{command_name} failed"
    );
}

/// Looks up the session for a coverage lookup; an unknown session is logged
/// (see [`log_explain_failure`]) and returned as an error.
pub fn session_for_explain(
    sessions: &crate::session::SessionManager,
    session_id: crate::state::SessionId,
    command: &str,
    command_name: &str,
) -> Result<std::sync::Arc<crate::session::Session>, crate::error::CommandError> {
    sessions.get(session_id).ok_or_else(|| {
        let err = crate::error::CommandError::with_code(
            format!("unknown session {session_id:?}"),
            "SESSION_NOT_FOUND",
        );
        log_explain_failure(&err, session_id, command, command_name);
        err
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
    let transform_scope = if session.safety.outgoing_transform.is_some() {
        Some(crate::orchestration::outgoing_scope(session, profile_store).await)
    } else {
        None
    };
    explain(
        session.safety.filter_engine.as_ref(),
        &ctx,
        command,
        stored,
        scope,
        priority,
        &|cmd| {
            crate::orchestration::state_independent_escalation_code(
                session,
                cmd,
                transform_scope.as_ref(),
            )
        },
    )
    .await
}

/// [`explain_segment_rules`] for a pending command of `session`. Server and
/// tags come from the session, not from the frontend.
pub async fn explain_segment_rules_for_session(
    session: &crate::session::Session,
    profile_store: &dyn ssh_manager_core::profiles::ProfileStore,
    command: &str,
    segment_index: usize,
    scope: Scope,
    staged: &[StagedRuleDto],
) -> Result<SegmentRuleSuggestionsDto, crate::error::CommandError> {
    let tags = profile_store
        .get_server(&session.server_id)
        .await
        .map(|s| s.tags)
        .unwrap_or_else(|_| session.tags.clone());
    let ctx = EvalContext {
        server_id: session.server_id,
        tags,
    };
    let transform_scope = if session.safety.outgoing_transform.is_some() {
        Some(crate::orchestration::outgoing_scope(session, profile_store).await)
    } else {
        None
    };
    explain_segment_rules(
        session.safety.filter_engine.as_ref(),
        &ctx,
        command,
        segment_index,
        scope,
        staged,
        &|cmd| {
            crate::orchestration::state_independent_escalation_code(
                session,
                cmd,
                transform_scope.as_ref(),
            )
        },
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use ssh_manager_core::filter::PolicyRules;
    use ssh_manager_core::filter::{
        scope_applies, EffectiveScope, FilterEngine, Pattern, PolicyStore, ServerId,
    };

    struct StaticStore(Vec<Rule>);

    #[async_trait]
    impl PolicyStore for StaticStore {
        async fn rules_for(&self, scope: &EffectiveScope) -> PolicyRules {
            PolicyRules::complete(
                self.0
                    .iter()
                    .filter(|r| scope_applies(&r.scope, scope))
                    .cloned()
                    .collect(),
            )
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
    async fn single_needs_rule_segment_is_listed_and_unrelated_rule_missing_says_no_rule() {
        let r = run(vec![], "wc -l", Scope::Global, &none).await;
        assert_eq!(r.segments.len(), 1);
        assert_eq!(r.segments[0].state, SegmentCoverageState::NeedsRule);
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

    #[test]
    fn explain_failure_log_names_the_command() {
        use crate::test_support::log_capture;
        log_capture::start_recording();
        let sessions = crate::session::SessionManager::new();
        let id = uuid::Uuid::from_u128(8);
        let command = "cat /etc/secret-marker-abc";
        assert!(session_for_explain(&sessions, id, command, "explain_segment_rules").is_err());
        let lines = log_capture::recorded_lines_at_info_or_above();
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(
            lines[0].contains("explain_segment_rules failed"),
            "{}",
            lines[0]
        );
        assert!(!lines[0].contains("explain_confirmation"));
        assert!(!lines[0].contains("secret-marker-abc"));
    }

    #[test]
    fn unknown_session_logs_one_warn_without_command_text() {
        use crate::test_support::log_capture;
        log_capture::start_recording();
        let sessions = crate::session::SessionManager::new();
        let id = uuid::Uuid::from_u128(7);
        let command = "cat /etc/secret-marker-xyz";
        assert!(session_for_explain(&sessions, id, command, "explain_confirmation").is_err());
        let lines = log_capture::recorded_lines_at_info_or_above();
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(lines[0].contains("\"WARN\""), "{}", lines[0]);
        assert!(lines[0].contains("SESSION_NOT_FOUND"));
        assert!(lines[0].contains("command_len"));
        assert!(!lines[0].contains("secret-marker-xyz"));
    }

    const SECRET: &str = "FILTER_SECRET_PATH_READ_REQUIRES_CONFIRM";

    #[tokio::test]
    async fn single_covered_command_with_escalation_is_blocked() {
        let esc = |c: &str| c.contains("shadow").then_some(SECRET);
        let r = run(
            vec![allow("r1", "cat *")],
            "cat /etc//shadow",
            Scope::Global,
            &esc,
        )
        .await;
        assert_eq!(r.segments.len(), 1);
        assert_eq!(r.segments[0].state, SegmentCoverageState::Blocked);
        assert_eq!(r.segments[0].reasons, vec![SECRET.to_string()]);
        assert!(r.segments[0].rule.is_none());
    }

    #[tokio::test]
    async fn escalation_is_attributed_to_the_reading_segment() {
        let esc = |c: &str| c.contains("shadow").then_some(SECRET);
        let r = run(
            vec![allow("r1", "cat *")],
            "cd /etc && cat shadow",
            Scope::Global,
            &esc,
        )
        .await;
        assert_eq!(r.segments.len(), 2);
        assert_eq!(r.segments[0].text, "cd /etc");
        assert_eq!(r.segments[0].state, SegmentCoverageState::NeedsRule);
        assert_eq!(r.segments[1].state, SegmentCoverageState::Blocked);
        assert_eq!(r.segments[1].reasons, vec![SECRET.to_string()]);
    }

    #[tokio::test]
    async fn whole_command_escalation_falls_back_to_covered_segments() {
        let esc = |c: &str| (c.contains("&&")).then_some(SECRET);
        let r = run(
            vec![allow("r1", "cat *"), allow("r2", "cd *")],
            "cd /etc && cat shadow",
            Scope::Global,
            &esc,
        )
        .await;
        assert_eq!(r.segments.len(), 2);
        for seg in &r.segments {
            assert_eq!(seg.state, SegmentCoverageState::Blocked);
            assert_eq!(seg.reasons, vec![SECRET.to_string()]);
        }
    }

    #[tokio::test]
    async fn single_covered_command_without_escalation_is_listed_as_covered() {
        let r = run(
            vec![allow("r1", "cat *")],
            "cat /tmp/a",
            Scope::Global,
            &none,
        )
        .await;
        assert_eq!(r.segments.len(), 1);
        assert_eq!(r.segments[0].state, SegmentCoverageState::Covered);
    }

    async fn auto_rules(rules: Vec<Rule>, command: &str) -> (Decision, Vec<AutoExecRuleDto>) {
        let engine = FilterEngine::new(StaticStore(rules));
        let c = EvalContext {
            server_id: ctx().server_id,
            tags: vec!["web".to_string()],
        };
        let trace = engine.evaluate_explained(command, &c).await;
        let out = auto_exec_rules(&engine, &c, &trace).await;
        (trace.decision, out)
    }

    #[tokio::test]
    async fn auto_exec_rules_name_global_server_and_tag_rules() {
        let mut server = allow("s", "uptime");
        server.scope = Scope::Server(ctx().server_id);
        let mut tag = allow("t", "df *");
        tag.scope = Scope::Tag("web".to_string());
        let rules = vec![allow("g", "ls *"), server, tag];
        for (cmd, id, ty, value, scope) in [
            ("ls /tmp", "g", PatternType::Glob, "ls *", Scope::Global),
            (
                "uptime",
                "s",
                PatternType::Glob,
                "uptime",
                Scope::Server(ctx().server_id),
            ),
            (
                "df -h",
                "t",
                PatternType::Glob,
                "df *",
                Scope::Tag("web".to_string()),
            ),
        ] {
            let (decision, out) = auto_rules(rules.clone(), cmd).await;
            assert_eq!(decision, Decision::AutoExec, "{cmd}");
            assert_eq!(out.len(), 1, "{cmd}");
            assert_eq!(out[0].rule_id, RuleId(id.to_string()));
            assert_eq!(out[0].pattern_type, ty);
            assert_eq!(out[0].pattern_value, value);
            assert_eq!(out[0].scope, scope);
            assert_eq!(out[0].segment_indices, vec![0]);
        }
    }

    #[tokio::test]
    async fn auto_exec_chain_lists_both_rules_and_dedups_shared_one() {
        let rules = vec![allow("a", "cd *"), allow("b", "ls *")];
        let (decision, out) = auto_rules(rules, "cd /tmp && ls /x").await;
        assert_eq!(decision, Decision::AutoExec);
        let ids: Vec<&str> = out.iter().map(|r| r.rule_id.0.as_str()).collect();
        assert_eq!(ids, vec!["a", "b"]);

        let (_, out) = auto_rules(vec![allow("x", "*")], "cd /tmp && ls /x").await;
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].segment_indices, vec![0, 1]);
    }

    #[tokio::test]
    async fn rule_gone_from_store_is_omitted() {
        let engine = FilterEngine::new(StaticStore(vec![allow("g", "ls *")]));
        let trace = engine.evaluate_explained("ls /tmp", &ctx()).await;
        let empty = FilterEngine::new(StaticStore(Vec::new()));
        assert!(auto_exec_rules(&empty, &ctx(), &trace).await.is_empty());
    }

    // --- Issue #1356: suggestions per segment ------------------------------

    fn staged(globs: &[&str], scope: Scope) -> StagedRuleDto {
        StagedRuleDto {
            patterns: globs
                .iter()
                .map(|g| crate::dto::SuggestionPatternDto {
                    pattern_type: PatternType::Glob,
                    pattern_value: (*g).to_string(),
                })
                .collect(),
            scope,
        }
    }

    async fn seg(
        rules: Vec<Rule>,
        command: &str,
        index: usize,
        scope: Scope,
        staged: &[StagedRuleDto],
    ) -> Result<SegmentRuleSuggestionsDto, crate::error::CommandError> {
        let engine = FilterEngine::new(StaticStore(rules));
        explain_segment_rules(&engine, &ctx(), command, index, scope, staged, &none).await
    }

    const CHAIN: &str = "echo a && ls -la /tmp && git status";

    #[tokio::test]
    async fn segment_suggestions_stay_within_their_segment() {
        let words = ["echo", "ls", "git"];
        for (i, own) in words.iter().enumerate() {
            let r = seg(vec![], CHAIN, i, Scope::Global, &[]).await.unwrap();
            assert_eq!(r.segment_index, i);
            assert!(r.blocked_reasons.is_empty());
            assert!(!r.suggestions.is_empty(), "segment {i}");
            assert!(r.suggestions.len() <= 6);
            for s in &r.suggestions {
                for p in &s.suggestion.patterns {
                    assert!(p.pattern_value.starts_with(own), "{p:?} for segment {i}");
                    for other in words.iter().filter(|w| *w != own) {
                        assert!(!p.pattern_value.contains(other), "{p:?} for segment {i}");
                    }
                }
            }
        }
    }

    #[tokio::test]
    async fn command_verdict_counts_staged_rules() {
        let with = seg(
            vec![],
            CHAIN,
            2,
            Scope::Global,
            &[
                staged(&["echo *"], Scope::Global),
                staged(&["ls *"], Scope::Global),
            ],
        )
        .await
        .unwrap();
        let tool = with
            .suggestions
            .iter()
            .find(|s| {
                s.suggestion
                    .patterns
                    .iter()
                    .any(|p| p.pattern_value == "git *")
            })
            .expect("tool suggestion");
        assert!(tool.command_verdict.auto_exec);
        assert!(tool.segment_verdict.covered);

        let without = seg(vec![], CHAIN, 2, Scope::Global, &[]).await.unwrap();
        for s in &without.suggestions {
            assert!(!s.command_verdict.auto_exec);
            assert!(s.segment_verdict.covered);
            assert!(!s.command_verdict.reasons.is_empty());
        }
    }

    #[tokio::test]
    async fn hard_blacklist_and_blocked_segments_get_no_suggestions() {
        let r = seg(
            vec![allow("r", "rm *")],
            "echo a && rm -rf /",
            1,
            Scope::Global,
            &[],
        )
        .await
        .unwrap();
        assert!(r.suggestions.is_empty());
        assert!(
            r.blocked_reasons
                .contains(&"FILTER_HARD_BLACKLIST".to_string()),
            "{:?}",
            r.blocked_reasons
        );

        let r = seg(vec![], "grep a f > /tmp/out", 0, Scope::Global, &[])
            .await
            .unwrap();
        assert!(r.suggestions.is_empty());
        assert!(r
            .blocked_reasons
            .contains(&"FILTER_OUTPUT_REDIRECTION".to_string()));
    }

    #[tokio::test]
    async fn escalated_segment_is_blocked_and_never_auto() {
        let engine = FilterEngine::new(StaticStore(vec![allow("r", "ls *")]));
        let esc = |c: &str| c.starts_with("ls").then_some("ESCALATED");
        let r = explain_segment_rules(
            &engine,
            &ctx(),
            "echo a && ls /tmp",
            1,
            Scope::Global,
            &[],
            &esc,
        )
        .await
        .unwrap();
        assert!(r.suggestions.is_empty());
        assert_eq!(r.blocked_reasons, vec!["ESCALATED".to_string()]);
    }

    #[tokio::test]
    async fn tag_scope_the_session_lacks_still_asks_with_scope_reason() {
        let r = seg(vec![], CHAIN, 0, Scope::Tag("prod".into()), &[])
            .await
            .unwrap();
        assert!(!r.suggestions.is_empty());
        for s in &r.suggestions {
            assert!(!s.segment_verdict.covered);
            assert_eq!(
                s.segment_verdict.reasons,
                vec![REASON_SCOPE_NOT_APPLICABLE.to_string()]
            );
            assert!(!s.command_verdict.auto_exec);
        }
    }

    #[tokio::test]
    async fn out_of_range_index_and_invalid_staged_pattern_are_errors() {
        let e = seg(vec![], CHAIN, 3, Scope::Global, &[]).await.unwrap_err();
        assert_eq!(e.code, Some(SEGMENT_INDEX_OUT_OF_RANGE));
        let e = seg(
            vec![],
            CHAIN,
            0,
            Scope::Global,
            &[staged(&["ls [abc *"], Scope::Global)],
        )
        .await
        .unwrap_err();
        assert_eq!(e.code, Some("INVALID_PATTERN"));
    }
}
