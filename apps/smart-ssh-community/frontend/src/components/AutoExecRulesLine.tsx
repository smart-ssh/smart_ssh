import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { listRules, listServers } from "../api/api";
import { requestOpenFilterRule } from "../buses/navigationBus";
import type { AutoExecRule, Scope } from "../api/types";
import { Button, Icon } from "./ui";

/** Issue #696/#1371, Spec 0011: left part of the executed-action card header —
 * bolt icon, "Ran automatically" and per rule "rule `<pattern>` · <scope>". The pattern is a link into the filter rules tab while the rule still
 * exists, plain text otherwise. Display only. */
export function AutoExecRulesLine({ rules }: { rules: AutoExecRule[] }) {
  const { t } = useTranslation();
  const [existing, setExisting] = useState<Set<string>>(new Set());
  const [serverNames, setServerNames] = useState<Record<string, string>>({});
  const needsServers = rules.some((r) => typeof r.scope === "object" && "Server" in r.scope);

  useEffect(() => {
    if (rules.length === 0) return;
    let cancelled = false;
    listRules()
      .then((all) => {
        if (!cancelled) setExisting(new Set(all.map((r) => r.id)));
      })
      .catch(() => {
        // Without the list the patterns simply stay plain text.
      });
    if (needsServers) {
      listServers()
        .then((all) => {
          if (!cancelled) setServerNames(Object.fromEntries(all.map((s) => [s.id, s.name])));
        })
        .catch(() => {
          // The server id is shown instead of its name.
        });
    }
    return () => {
      cancelled = true;
    };
  }, [needsServers, rules.length]);

  const scopeText = (scope: Scope): string => {
    if (scope === "Global") return t("actionCard.autoRanScopeGlobal");
    if ("Server" in scope) {
      return t("actionCard.autoRanScopeServer", { name: serverNames[scope.Server] ?? scope.Server });
    }
    return t("actionCard.autoRanScopeTag", { tag: scope.Tag });
  };

  return (
    <div className="flex min-w-0 flex-1 flex-wrap items-center gap-x-1.5 gap-y-1 text-xs text-fg-subtle">
      <Icon name="bolt" className="text-success-fg" />
      <span className="text-label text-success-fg">{t("actionCard.autoRanLead")}</span>
      {rules.map((rule) => (
        <span key={rule.ruleId} className="inline-flex flex-wrap items-center gap-1.5">
          <span aria-hidden="true">·</span>
          <span>{t("actionCard.autoRanRule")}</span>
          {existing.has(rule.ruleId) ? (
            <Button
              variant="bare"
              type="button"
              className="border border-border-strong px-1.5 font-mono text-fg-soft underline hover:text-fg"
              title={t("actionCard.autoRanOpenRule")}
              onClick={() => requestOpenFilterRule(rule.ruleId)}
            >
              {rule.patternValue}
            </Button>
          ) : (
            <span className="border border-border-strong px-1.5 font-mono text-fg-soft">{rule.patternValue}</span>
          )}
          <span aria-hidden="true">·</span>
          <span>{scopeText(rule.scope)}</span>
        </span>
      ))}
    </div>
  );
}
