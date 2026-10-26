import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { listRules, listServers } from "../api";
import { requestOpenFilterRule } from "../navigationBus";
import type { AutoExecRule, Scope } from "../types";

/** Issue #696, Spec 0011: muted line under the command of an automatically
 * run action — "Ran automatically · rule `<pattern>` · <scope>", one entry per
 * rule. The pattern is a link into the filter rules tab while the rule still
 * exists, plain text otherwise. Display only. */
export function AutoExecRulesLine({ rules }: { rules: AutoExecRule[] }) {
  const { t } = useTranslation();
  const [existing, setExisting] = useState<Set<string>>(new Set());
  const [serverNames, setServerNames] = useState<Record<string, string>>({});
  const needsServers = rules.some((r) => typeof r.scope === "object" && "Server" in r.scope);

  useEffect(() => {
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
  }, [needsServers]);

  const scopeText = (scope: Scope): string => {
    if (scope === "Global") return t("actionCard.autoRanScopeGlobal");
    if ("Server" in scope) {
      return t("actionCard.autoRanScopeServer", { name: serverNames[scope.Server] ?? scope.Server });
    }
    return t("actionCard.autoRanScopeTag", { tag: scope.Tag });
  };

  return (
    <p className="mt-1 text-xs text-fg-subtle">
      {t("actionCard.autoRanLead")}
      {rules.map((rule) => (
        <span key={rule.ruleId}>
          {" · "}
          {t("actionCard.autoRanRule")}{" "}
          {existing.has(rule.ruleId) ? (
            <button
              type="button"
              className="font-mono underline hover:text-fg-soft"
              title={t("actionCard.autoRanOpenRule")}
              onClick={() => requestOpenFilterRule(rule.ruleId)}
            >
              {rule.patternValue}
            </button>
          ) : (
            <span className="font-mono">{rule.patternValue}</span>
          )}
          {" · "}
          {scopeText(rule.scope)}
        </span>
      ))}
    </p>
  );
}
