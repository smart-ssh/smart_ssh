import { useEffect, useId, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  commandErrorCode,
  commandErrorMessage,
  createQuickRulesBatch,
  explainConfirmation,
  explainSegmentRules,
} from "../../api/api";
import { translateErrorCode } from "../../api/errorCodes";
import type {
  ConfirmationExplanationDto,
  Scope,
  SegmentCoverageDto,
  SegmentRuleSuggestionsDto,
  StagedRuleDto,
} from "../../api/types";
import type { TranslateFn } from "../../i18nTypes";
import { TECHNICAL_INPUT_PROPS } from "../../lib/technicalInputProps";
import { Button, Icon } from "../ui";
import { reasonList } from "./reasonList";

/** Issue #1357: a rule picked for one sub-command; `saved` once the batch
 * save went through (the row then shows a green "covered" mark). */
export interface StagedEntry {
  rule: StagedRuleDto;
  saved: boolean;
}

const rowKey = (s: SegmentCoverageDto) => `${s.index}:${s.text}`;

/** Issue #1357, Spec 0011: loads the coverage of the pending command and
 * holds the rules staged per sub-command. Nothing here approves or runs the
 * command; the staged rules are only saved through `save()`. */
export function useRuleStaging(sessionId: string, command: string, enabled = true) {
  const { t } = useTranslation();
  const [explanation, setExplanation] = useState<ConfirmationExplanationDto | null>(null);
  const [failed, setFailed] = useState(false);
  const [staged, setStaged] = useState<Record<string, StagedEntry>>({});
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);

  useEffect(() => {
    if (!enabled) return;
    let stale = false;
    explainConfirmation(sessionId, command, "Global")
      .then((r) => {
        if (stale) return;
        setExplanation(r);
        setFailed(false);
        // An edited command can drop or change sub-commands: forget rules
        // staged for rows that no longer exist.
        const keys = new Set(r.segments.map(rowKey));
        setStaged((prev) =>
          Object.fromEntries(Object.entries(prev).filter(([k]) => keys.has(k))),
        );
      })
      .catch(() => {
        // The backend logs the error; the card says the lookup failed.
        if (!stale) {
          setExplanation(null);
          setFailed(true);
        }
      });
    return () => {
      stale = true;
    };
  }, [sessionId, command, enabled]);

  const stage = (key: string, rule: StagedRuleDto) => {
    setSaveError(null);
    setStaged((prev) => ({ ...prev, [key]: { rule, saved: false } }));
  };
  const unstage = (key: string) => {
    setSaveError(null);
    setStaged((prev) => {
      const { [key]: _removed, ...rest } = prev;
      return rest;
    });
  };

  const unsaved = Object.entries(staged).filter(([, e]) => !e.saved);
  const save = () => {
    if (unsaved.length === 0 || saving) return;
    const keys = unsaved.map(([k]) => k);
    setSaving(true);
    setSaveError(null);
    createQuickRulesBatch(unsaved.map(([, e]) => e.rule))
      .then(() =>
        setStaged((prev) =>
          Object.fromEntries(
            Object.entries(prev).map(([k, e]) => [k, keys.includes(k) ? { ...e, saved: true } : e]),
          ),
        ),
      )
      // All or nothing in the backend: the staging stays as it was.
      .catch((err) =>
        // Spec 0077, 3.1.4: a coded error is shown translated, without the library text.
        setSaveError(translateErrorCode(t, commandErrorCode(err), commandErrorMessage(err))),
      )
      .finally(() => setSaving(false));
  };

  return { explanation, failed, staged, unsavedCount: unsaved.length, saving, saveError, stage, unstage, save };
}

export type RuleStaging = ReturnType<typeof useRuleStaging>;

function scopeText(t: TranslateFn, scope: Scope): string {
  if (scope === "Global") return t("confirmDialog.scopeGlobal");
  if ("Tag" in scope) return `${t("confirmDialog.scopeTag")}: ${scope.Tag}`;
  return t("confirmDialog.scopeThisServer");
}

const ruleText = (t: TranslateFn, rule: StagedRuleDto) =>
  `${rule.patterns.map((p) => p.patternValue).join(", ")} · ${scopeText(t, rule.scope)}`;

/** Issue #1357, Spec 0011 §4/§6: already allowed sub-commands on top as a
 * compact summary, below the ones that still need a decision. */
export function SegmentRules({
  staging,
  sessionId,
  command,
  serverId,
}: {
  staging: RuleStaging;
  sessionId: string;
  command: string;
  serverId: string;
}) {
  const { t } = useTranslation();
  const { explanation, failed, staged } = staging;
  if (failed) {
    return <div className="text-xs text-fg-subtle">{t("confirmDialog.explain.loadFailed")}</div>;
  }
  if (!explanation || explanation.segments.length === 0) return null;
  const allowed = explanation.segments.filter((s) => s.state === "covered");
  const rows = explanation.segments.filter((s) => s.state !== "covered");
  return (
    <div className="space-y-2 text-xs">
      {allowed.length > 0 && (
        <div className="space-y-1">
          <div className="text-label text-fg-subtle">
            {t("confirmDialog.explain.summaryHeading")}
          </div>
          <ul className="space-y-0.5">
            {allowed.map((s) => (
              <li
                key={rowKey(s)}
                className="flex flex-wrap gap-x-2"
                title={
                  s.rule
                    ? t("confirmDialog.explain.covered", { rule: s.rule.patternValue })
                    : t("confirmDialog.explain.coveredUnknown")
                }
              >
                <Icon name="check" className="text-success-fg-soft" />
                <code className="font-mono break-all text-fg-soft">{s.text}</code>
              </li>
            ))}
          </ul>
        </div>
      )}
      {rows.length > 0 && (
        <div className="space-y-1">
          <div className="text-label text-fg-subtle">
            {rows.length === explanation.segments.length
              ? t("confirmDialog.explain.heading")
              : t("confirmDialog.explain.toDecideHeading")}
          </div>
          <ul className="divide-y divide-border border-y border-border">
            {rows.map((s) => {
              const key = rowKey(s);
              const others = Object.entries(staged)
                .filter(([k, e]) => k !== key && !e.saved)
                .map(([, e]) => e.rule);
              return (
                <SegmentRow
                  key={key}
                  segment={s}
                  entry={staged[key]}
                  others={others}
                  sessionId={sessionId}
                  command={command}
                  serverId={serverId}
                  onStage={(rule) => staging.stage(key, rule)}
                  onUnstage={() => staging.unstage(key)}
                />
              );
            })}
          </ul>
          {rows.some((s) => s.state === "needsRule") && (
            <div className="flex flex-wrap items-center gap-2 border-t border-border pt-2">
              <Button
                size="sm"
                className="narrow:min-h-11"
                disabled={staging.unsavedCount < 1 || staging.saving}
                onClick={staging.save}
              >
                {t("confirmDialog.explain.saveRules", { count: staging.unsavedCount })}
              </Button>
              {staging.saveError && (
                <span role="alert" className="text-danger-fg">
                  {t("confirmDialog.explain.saveFailed", { message: staging.saveError })}
                </span>
              )}
            </div>
          )}
        </div>
      )}
    </div>
  );
}

function SegmentRow({
  segment,
  entry,
  others,
  sessionId,
  command,
  serverId,
  onStage,
  onUnstage,
}: {
  segment: SegmentCoverageDto;
  entry: StagedEntry | undefined;
  /** Rules staged on the other rows; the verdicts consider them. */
  others: StagedRuleDto[];
  sessionId: string;
  command: string;
  serverId: string;
  onStage: (rule: StagedRuleDto) => void;
  onUnstage: () => void;
}) {
  const { t } = useTranslation();
  const panelId = useId();
  const [open, setOpen] = useState(false);
  const [scopeKind, setScopeKind] = useState<"server" | "global" | "tag">("server");
  const [tag, setTag] = useState("");
  const [result, setResult] = useState<SegmentRuleSuggestionsDto | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const scope: Scope | null =
    scopeKind === "global"
      ? "Global"
      : scopeKind === "server"
        ? { Server: serverId }
        : tag.trim()
          ? { Tag: tag.trim() }
          : null;
  const inputKey = JSON.stringify([scope, others]);
  useEffect(() => {
    if (!open || !scope) {
      setResult(null);
      return;
    }
    let stale = false;
    setLoading(true);
    setError(null);
    explainSegmentRules(sessionId, command, segment.index, scope, others)
      .then((r) => {
        if (!stale) setResult(r);
      })
      .catch((err) => {
        if (!stale) {
          setResult(null);
          setError(commandErrorMessage(err));
        }
      })
      .finally(() => {
        if (!stale) setLoading(false);
      });
    return () => {
      stale = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- reload only when the inputs listed here change
  }, [open, sessionId, command, segment.index, inputKey]);

  const needsRule = segment.state === "needsRule";
  return (
    <li className="space-y-1 px-1 py-1.5">
      <div className="flex flex-wrap items-start gap-x-2 gap-y-1">
        {needsRule && (
          <Button
            variant="bare"
            type="button"
            aria-expanded={open}
            aria-controls={panelId}
            aria-label={t("confirmDialog.explain.expandAria", { segment: segment.text })}
            onClick={() => setOpen((o) => !o)}
            className="focus-ring w-5 shrink-0 font-mono text-fg-subtle narrow:min-h-11 narrow:w-11"
          >
            <Icon name={open ? "chev-d" : "chev-r"} />
          </Button>
        )}
        <code className="font-mono break-all text-fg-soft">{segment.text}</code>
        {!entry && needsRule && (
          <span className="text-warning-fg-soft">{t("confirmDialog.explain.needsRule")}</span>
        )}
        {!needsRule && (
          <span className="text-warning-fg-soft">
            {t("confirmDialog.explain.blocked", { reasons: reasonList(t, segment.reasons) })}
          </span>
        )}
        {entry && !entry.saved && (
          <>
            <span className="text-accent" data-testid="staged-rule">
              <Icon name="check" className="mr-1 inline align-text-bottom" /> {ruleText(t, entry.rule)} ({t("confirmDialog.explain.stagedBadge")})
            </span>
            <Button variant="ghost" size="sm" onClick={onUnstage}>
              {t("confirmDialog.explain.removeStaged")}
            </Button>
          </>
        )}
        {entry?.saved && (
          <span className="text-success-fg-soft" data-testid="saved-rule">
            <Icon name="check" className="mr-1 inline align-text-bottom" /> {t("confirmDialog.explain.covered", { rule: ruleText(t, entry.rule) })}
          </span>
        )}
      </div>
      {needsRule && open && (
        <div id={panelId} className="space-y-2 border border-border bg-surface-sunken p-2">
          <div className="flex flex-wrap gap-3 text-fg-muted">
            {(["server", "global", "tag"] as const).map((kind) => (
              <label key={kind} className="flex items-center gap-1 narrow:min-h-11">
                <input
                  type="radio"
                  name={`${panelId}-scope`}
                  checked={scopeKind === kind}
                  onChange={() => setScopeKind(kind)}
                />
                {t(
                  kind === "server"
                    ? "confirmDialog.scopeThisServer"
                    : kind === "global"
                      ? "confirmDialog.scopeGlobal"
                      : "confirmDialog.scopeTag",
                )}
              </label>
            ))}
          </div>
          {scopeKind === "tag" && (
            <input
              {...TECHNICAL_INPUT_PROPS}
              type="text"
              value={tag}
              onChange={(e) => setTag(e.target.value)}
              placeholder={t("confirmDialog.tagNamePlaceholder")}
              aria-label={t("confirmDialog.tagNamePlaceholder")}
              className="w-full border border-border-strong bg-surface px-2 py-1 font-mono text-xs text-fg"
            />
          )}
          {loading && <p className="text-fg-subtle">{t("confirmDialog.loadingSuggestions")}</p>}
          {error && <p className="text-danger-fg">{error}</p>}
          {!loading && !error && scope && result && result.suggestions.length === 0 && (
            <p className="text-fg-subtle">{t("confirmDialog.noSuggestions")}</p>
          )}
          <ul className="space-y-1">
            {(result?.suggestions ?? []).map((s) => {
              const label = t(`confirmDialog.quickRuleLabel.${s.kind}`, {
                names: s.names.join(", "),
                patterns: s.patterns.map((p) => p.patternValue).join(", "),
              });
              // The verdicts come from the backend only; nothing is computed here.
              const verdict = s.commandVerdict.autoExec
                ? t("confirmDialog.explain.verdictAuto")
                : s.segmentVerdict.covered
                  ? t("confirmDialog.explain.segmentCovered", {
                      reasons: reasonList(t, s.commandVerdict.reasons.map((r) => r.code)),
                    })
                  : t("confirmDialog.explain.verdictAsk", {
                      reasons: reasonList(t, s.segmentVerdict.reasons),
                    });
              return (
                <li key={s.patterns.map((p) => p.patternValue).join("\n")}>
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() => {
                      if (!scope) return;
                      onStage({ patterns: s.patterns, scope });
                      setOpen(false);
                    }}
                    className="w-full justify-start text-left font-mono break-all text-fg-soft narrow:h-auto narrow:min-h-11"
                  >
                    {label}
                  </Button>
                  <p
                    className={`px-3 pb-1 ${s.commandVerdict.autoExec ? "text-success-fg-soft" : "text-warning-fg-soft"}`}
                  >
                    {verdict}
                  </p>
                </li>
              );
            })}
          </ul>
        </div>
      )}
    </li>
  );
}
