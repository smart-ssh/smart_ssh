import { type KeyboardEvent, forwardRef, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { commandErrorMessage, evaluateExplained } from "../../api/api";
import { translateErrorCode } from "../../api/errorCodes";
import type { EvaluationTraceDto, RuleDto, ServerDto } from "../../api/types";
import { TECHNICAL_INPUT_PROPS } from "../../lib/technicalInputProps";
import { Badge, Button, FIELD_CONTROL_CLASS, Icon, TagInput } from "../ui";
import type { BadgeTone } from "../ui";

type Decision = EvaluationTraceDto["decision"];

function decisionInfo(decision: Decision): {
  text: string;
  tone: BadgeTone;
  tint: string;
  reason?: string;
  code?: string;
} {
  if (decision === "AutoExec") {
    return { text: "AutoExec", tone: "success", tint: "bg-success-subtle border-success-border" };
  }
  if ("Confirm" in decision) {
    return {
      text: "Confirm",
      tone: "warning",
      tint: "bg-warning-subtle border-warning-border",
      reason: decision.Confirm.reason,
      code: decision.Confirm.code,
    };
  }
  return {
    text: "Deny",
    tone: "danger",
    tint: "bg-danger-subtle border-danger-border",
    reason: decision.Deny.reason,
    code: decision.Deny.code,
  };
}

function DecisionBadge({ decision }: { decision: Decision }) {
  const { text, tone } = decisionInfo(decision);
  return (
    <Badge tone={tone} square>
      {text}
    </Badge>
  );
}

interface TraceProps {
  trace: EvaluationTraceDto;
  rules: RuleDto[];
  scopeText: (rule: RuleDto) => string;
}

function TraceDetails({ trace, rules, scopeText }: TraceProps) {
  const { t } = useTranslation();
  const { reason, code } = decisionInfo(trace.decision);
  const matchedRule = trace.matchedRule ? rules.find((r) => r.id === trace.matchedRule) : null;
  return (
    <div className="mt-1 space-y-0.5 text-xs text-fg-subtle">
      {reason && <p>{translateErrorCode(t, code, reason)}</p>}
      {matchedRule && (
        <p>
          {t("filterRules.matchedRuleLabel")} <code className="font-mono text-fg-soft">{matchedRule.patternValue}</code> (
          {matchedRule.action}) · {scopeText(matchedRule)}
        </p>
      )}
      {trace.checks.length > 0 && (
        <p data-testid="trace-checks">
          {t("filterRules.checksLabel")}{" "}
          {trace.checks.map((c) => translateErrorCode(t, c, c)).join("; ")}
        </p>
      )}
      {trace.matchedHardBlacklistEntry && (
        <p>
          {t("filterRules.hardBlacklistLabel")} <code>{trace.matchedHardBlacklistEntry}</code>
        </p>
      )}
    </div>
  );
}

/** Recursive: one trace per sub-command (chaining) or command substitution,
 * see the `EvaluationTraceDto` doc comment. */
function TraceView({ trace, rules, scopeText }: TraceProps) {
  const { t } = useTranslation();
  return (
    <div className="border border-border bg-surface p-2">
      <div className="flex items-center justify-between gap-2">
        {trace.kind === "Segment" && trace.segmentIndex !== null ? (
          <span className="text-xs text-fg-subtle">
            {t("filterRules.segmentLabel", { index: trace.segmentIndex + 1 })}
          </span>
        ) : (
          <span />
        )}
        <DecisionBadge decision={trace.decision} />
      </div>
      <TraceDetails trace={trace} rules={rules} scopeText={scopeText} />
      {trace.subCommandTraces.length > 0 && (
        <div className="mt-2 space-y-1 border-l border-border pl-2">
          {trace.subCommandTraces.map((sub, i) => (
            <TraceView key={i} trace={sub} rules={rules} scopeText={scopeText} />
          ))}
        </div>
      )}
    </div>
  );
}

interface RuleTesterProps {
  servers: ServerDto[];
  rules: RuleDto[];
  scopeText: (rule: RuleDto) => string;
}

/** Spec 0009, section 6: example command + optional scope simulation,
 * `evaluate_explained` — each sub-command of a chain with its own decision,
 * plus the overall decision. Side panel on wide windows, below the list on
 * narrow ones; the ref is the panel so the header button can focus it. */
export const RuleTester = forwardRef<HTMLElement, RuleTesterProps>(function RuleTester(
  { servers, rules, scopeText },
  ref,
) {
  const { t } = useTranslation();
  const [command, setCommand] = useState("");
  const [serverId, setServerId] = useState("");
  const [tags, setTags] = useState<string[]>([]);
  const [result, setResult] = useState<EvaluationTraceDto | null>(null);
  const [testing, setTesting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Spec 0046, finding 2: the chosen server brings its own tags into the
  // simulation, as it would in real operation.
  const selectedServerTags = useMemo(
    () => servers.find((s) => s.id === serverId)?.tags ?? [],
    [servers, serverId],
  );

  // Spec 0077, 3.2.3: a rule with an invalid pattern never matches fully;
  // mark it as in the rule list.
  const invalidRules = useMemo(() => rules.filter((r) => !!r.patternError), [rules]);

  const handleTest = async () => {
    if (!command.trim()) return;
    setTesting(true);
    setError(null);
    try {
      const effectiveTags = Array.from(new Set([...selectedServerTags, ...tags]));
      setResult(
        await evaluateExplained(command, { serverId: serverId || null, tags: effectiveTags }),
      );
    } catch (err) {
      setError(commandErrorMessage(err));
    } finally {
      setTesting(false);
    }
  };

  // Single-line look; Enter tests, Shift+Enter and pasted line breaks keep
  // working (the field is a one-row textarea, not an <input>, so pasted
  // multi-line commands are not flattened).
  const onKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
      e.preventDefault();
      void handleTest();
    }
  };

  const overall = result ? decisionInfo(result.decision) : null;

  return (
    <section
      ref={ref}
      tabIndex={-1}
      aria-labelledby="filter-rules-tester-title"
      className="min-w-0 space-y-3 border-t border-border bg-surface-sunken p-4 outline-none narrow:p-3 min-[1100px]:min-h-0 min-[1100px]:w-[460px] min-[1100px]:shrink-0 min-[1100px]:overflow-y-auto min-[1100px]:border-t-0 min-[1100px]:border-l"
    >
      <h3 id="filter-rules-tester-title" className="text-title text-fg">
        {t("filterRules.testRules")}
      </h3>

      <label className="text-label block text-fg-subtle">
        {t("filterRules.exampleCommand")}
        <textarea
          {...TECHNICAL_INPUT_PROPS}
          value={command}
          onChange={(e) => setCommand(e.target.value)}
          onKeyDown={onKeyDown}
          rows={1}
          placeholder="ls -la && rm -rf /tmp/x"
          className={`mt-1 w-full resize-none font-mono normal-case ${FIELD_CONTROL_CLASS}`}
        />
      </label>

      <div className="grid grid-cols-2 gap-3 narrow:grid-cols-1">
        <label className="text-label block text-fg-subtle">
          {t("filterRules.simulateServer")}
          <select
            value={serverId}
            onChange={(e) => setServerId(e.target.value)}
            className={`mt-1 w-full normal-case ${FIELD_CONTROL_CLASS}`}
          >
            <option value="">{t("filterRules.noServerOption")}</option>
            {servers.map((s) => (
              <option key={s.id} value={s.id}>
                {s.name}
              </option>
            ))}
          </select>
        </label>
        <div className="text-label block text-fg-subtle">
          {t("filterRules.simulateTags")}
          <TagInput
            value={tags}
            onChange={setTags}
            readOnlyTags={selectedServerTags}
            readOnlyTitle={t("filterRules.tagFromServer")}
            placeholder={t("serverForm.tagPlaceholder")}
            aria-label={t("filterRules.simulateTags")}
          />
        </div>
      </div>
      <p className="text-xs text-fg-subtle">{t("filterRules.tagFromServer")}</p>

      <Button
        variant="primary"
        className="narrow:min-h-11"
        onClick={handleTest}
        disabled={testing || !command.trim()}
      >
        <Icon name="play" /> {testing ? t("common.testing") : t("filterRules.test")}
      </Button>

      {error && <p className="text-sm text-danger-fg">{error}</p>}

      {result && invalidRules.length > 0 && (
        <div className="space-y-1" data-testid="test-invalid-pattern-rules">
          <p className="text-label text-fg-subtle">{t("filterRules.testInvalidPatternRules")}</p>
          <ul className="space-y-1">
            {invalidRules.map((rule) => (
              <li key={rule.id} className="text-xs text-warning-fg-soft" role="alert">
                <code className="text-fg-soft">
                  {rule.patternType}: {rule.patternValue}
                </code>{" "}
                <Icon name="warn" className="mr-1 inline align-text-bottom" />{" "}
                {t("errors.FILTER_RULE_PATTERN_INVALID")}{" "}
                <span className="font-mono text-warning-fg-softer/70">{rule.patternError}</span>
              </li>
            ))}
          </ul>
        </div>
      )}

      {result && overall && (
        <div className={`border ${overall.tint}`}>
          <div className="flex items-center justify-between gap-2 px-3 py-2">
            <p className="text-label text-fg-subtle">{t("filterRules.overallDecision")}</p>
            <DecisionBadge decision={result.decision} />
          </div>
          <div className="space-y-2 px-3 pb-3">
            {result.subCommandTraces.length === 0 && (
              <TraceDetails trace={result} rules={rules} scopeText={scopeText} />
            )}
            {result.subCommandTraces.length > 0 && (
              <div className="space-y-1">
                <p className="text-label text-fg-subtle">{t("filterRules.subCommands")}</p>
                {result.subCommandTraces.map((sub, i) => (
                  <TraceView key={i} trace={sub} rules={rules} scopeText={scopeText} />
                ))}
              </div>
            )}
          </div>
        </div>
      )}
    </section>
  );
});
