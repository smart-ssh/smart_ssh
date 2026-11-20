import { useTranslation } from "react-i18next";
import type { RuleAction, RuleDto } from "../../api/types";
import { useIsNarrow } from "../../hooks/useIsNarrow";
import { Badge, Button, Icon } from "../ui";
import type { BadgeTone } from "../ui";
import type { IconName } from "../ui";
import { scopeKind } from "./scope";

export interface RuleGroup {
  key: string;
  label: string;
  rules: RuleDto[];
}

const ACTION_TONE: Record<RuleAction, BadgeTone> = {
  Allow: "success",
  Confirm: "warning",
  Deny: "danger",
};

export function ActionBadge({ action }: { action: RuleAction }) {
  return (
    <Badge tone={ACTION_TONE[action]} square>
      {action}
    </Badge>
  );
}

const SCOPE_ICON = { global: "globe", server: "server", tag: "tag" } as const satisfies Record<
  string,
  IconName
>;

/** Scope as icon + text ("Global", "Server: web-01", "Tag: production"). */
export function ScopeLabel({ rule, label }: { rule: RuleDto; label: string }) {
  return (
    <span className="inline-flex min-w-0 items-center gap-1.5 text-fg-soft">
      <Icon name={SCOPE_ICON[scopeKind(rule.scope)]} className="shrink-0 text-fg-subtle" />
      <span className="min-w-0 break-words">{label}</span>
    </span>
  );
}

interface RuleTableProps {
  group: RuleGroup;
  /** Scope text of one rule (the group label of the rule's own scope). */
  scopeText: (rule: RuleDto) => string;
  onMove: (groupRules: RuleDto[], index: number, direction: -1 | 1) => void;
  onEdit: (id: string) => void;
  onDelete: (id: string) => void;
}

/** Spec 0009, section 6: one table per scope group (the engine checks the
 * scope before the priority, so rules stay grouped). Wide: real table;
 * narrow (Spec 0113, 5d): rows with a 44 px priority cell. */
export function RuleTable({ group, scopeText, onMove, onEdit, onDelete }: RuleTableProps) {
  const { t } = useTranslation();
  const narrow = useIsNarrow();
  const { rules } = group;

  // Spec 0077, 3.2.3: reason why an arrow is disabled — the rule itself or
  // its swap neighbour carries an invalid pattern.
  const blockedTitle = (rule: RuleDto, neighbour: RuleDto | undefined) => {
    if (rule.patternError) return t("filterRules.priorityDisabledPatternError");
    if (neighbour?.patternError) return t("filterRules.priorityDisabledNeighbourPatternError");
    return undefined;
  };

  const rowButtons = (rule: RuleDto, index: number) => {
    const size = narrow ? "min-h-11 min-w-11" : "";
    return (
      <div className="flex items-center justify-end gap-1">
        <Button
          size="sm"
          className={size}
          onClick={() => onMove(rules, index, -1)}
          disabled={index === 0 || !!rule.patternError || !!rules[index - 1]?.patternError}
          title={blockedTitle(rule, rules[index - 1])}
          aria-label={t("filterRules.increasePriority")}
        >
          <Icon name="parent" />
        </Button>
        <Button
          size="sm"
          className={size}
          onClick={() => onMove(rules, index, 1)}
          disabled={
            index === rules.length - 1 || !!rule.patternError || !!rules[index + 1]?.patternError
          }
          title={blockedTitle(rule, rules[index + 1])}
          aria-label={t("filterRules.decreasePriority")}
        >
          <Icon name="arrow-down" />
        </Button>
        <Button
          size="sm"
          className={size}
          onClick={() => onEdit(rule.id)}
          aria-label={t("common.edit")}
        >
          <Icon name="edit" />
        </Button>
        <Button
          size="sm"
          variant="danger"
          className={size}
          onClick={() => onDelete(rule.id)}
          aria-label={t("common.delete")}
        >
          <Icon name="trash" />
        </Button>
      </div>
    );
  };

  // Spec 0077, 3.2.3: a rule whose pattern does not translate cannot take
  // effect — the row says so. Edit and Delete stay possible.
  const invalidLine = (rule: RuleDto) =>
    rule.patternError ? (
      <p className="mt-1.5 text-xs text-warning-fg-soft" role="alert">
        <Icon name="warn" className="mr-1 inline align-text-bottom" />{" "}
        {t("errors.FILTER_RULE_PATTERN_INVALID")}{" "}
        <span className="font-mono text-warning-fg-softer/70">{rule.patternError}</span>
      </p>
    ) : null;

  const heading = (
    <h3 className="text-label mb-1 text-fg-subtle">{group.label}</h3>
  );

  if (narrow) {
    return (
      <section>
        {heading}
        <ul className="border border-border">
          {rules.map((rule, index) => (
            <li key={rule.id} className="border-b border-border px-0 py-0 last:border-b-0">
              <div className="flex items-stretch gap-2">
                <span className="flex min-h-11 w-11 shrink-0 items-center justify-center bg-surface-sunken font-mono text-sm text-fg-soft">
                  {rule.priority}
                </span>
                <div className="min-w-0 flex-1 py-1.5 pr-2">
                  <code
                    className="block truncate font-mono text-sm text-fg"
                    title={`${rule.patternType}: ${rule.patternValue}`}
                  >
                    {rule.patternValue}
                  </code>
                  <div className="mt-1 flex flex-wrap items-center gap-x-2 gap-y-1 text-xs">
                    <ActionBadge action={rule.action} />
                    {rule.patternError && <Badge tone="danger">{t("filterRules.invalidPattern")}</Badge>}
                    <span className="font-mono text-fg-subtle">{rule.patternType}</span>
                    <ScopeLabel rule={rule} label={scopeText(rule)} />
                  </div>
                  {invalidLine(rule)}
                  <div className="mt-1">{rowButtons(rule, index)}</div>
                </div>
              </div>
            </li>
          ))}
        </ul>
      </section>
    );
  }

  return (
    <section>
      {heading}
      <table className="w-full table-fixed border border-border text-sm">
        <colgroup>
          <col className="w-16" />
          <col className="w-20" />
          <col />
          <col className="w-32" />
          <col className="w-44" />
          <col className="w-[10.5rem]" />
        </colgroup>
        <thead>
          <tr className="border-b border-border text-left">
            <th scope="col" className="text-label px-3 py-2 font-normal text-fg-subtle">
              {t("filterRules.priorityLabel")}
            </th>
            <th scope="col" className="text-label px-2 py-2 font-normal text-fg-subtle">
              {t("filterRules.patternType")}
            </th>
            <th scope="col" className="text-label px-2 py-2 font-normal text-fg-subtle">
              {t("filterRules.pattern")}
            </th>
            <th scope="col" className="text-label px-2 py-2 font-normal text-fg-subtle">
              {t("filterRules.action")}
            </th>
            <th scope="col" className="text-label px-2 py-2 font-normal text-fg-subtle">
              {t("filterRules.scope")}
            </th>
            <th scope="col" className="px-2 py-2" aria-label={t("filterRules.rowActions")} />
          </tr>
        </thead>
        <tbody>
          {rules.map((rule, index) => (
            <tr key={rule.id} className="border-b border-border last:border-b-0 align-top">
              <td className="px-3 py-2 font-mono text-fg-soft">{rule.priority}</td>
              <td className="px-2 py-2 font-mono text-fg-subtle">{rule.patternType}</td>
              <td className="px-2 py-2">
                <code className="font-mono break-all text-fg">{rule.patternValue}</code>
                {rule.patternError && (
                  <span className="ml-2 inline-block align-middle">
                    <Badge tone="danger">{t("filterRules.invalidPattern")}</Badge>
                  </span>
                )}
                {invalidLine(rule)}
              </td>
              <td className="px-2 py-2">
                <ActionBadge action={rule.action} />
              </td>
              <td className="px-2 py-2">
                <ScopeLabel rule={rule} label={scopeText(rule)} />
              </td>
              <td className="px-2 py-1.5">{rowButtons(rule, index)}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </section>
  );
}

