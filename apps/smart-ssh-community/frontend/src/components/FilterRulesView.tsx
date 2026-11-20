import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  commandErrorMessage,
  deleteRule,
  listHardBlacklist,
  listKnownTags,
  listRules,
  listServers,
  swapRulePriorities,
} from "../api/api";
import type { PatternDto, RuleDto, ServerDto } from "../api/types";
import { useIsNarrow } from "../hooks/useIsNarrow";
import { HardBlacklistSection } from "./filterRules/HardBlacklistSection";
import { RuleForm } from "./filterRules/RuleForm";
import { RuleTable, type RuleGroup } from "./filterRules/RuleTable";
import { RuleTester } from "./filterRules/RuleTester";
import { scopeKey, scopeKind, scopeServerId, scopeTag } from "./filterRules/scope";
import { Button } from "./ui";

/** Spec 0009, section 6: rule tables grouped by scope (up/down instead of
 * drag-and-drop), the hard blacklist block, the rule form dialog and the rule
 * tester (side panel on wide windows). The parts live in `filterRules/`. */
export function FilterRulesView({
  initialRuleId = null,
  onInitialRuleConsumed,
}: {
  /** Issue #696: rule to open for editing as soon as the list is loaded. */
  initialRuleId?: string | null;
  onInitialRuleConsumed?: () => void;
} = {}) {
  const { t } = useTranslation();
  const narrow = useIsNarrow();
  const testerRef = useRef<HTMLElement>(null);
  const [rules, setRules] = useState<RuleDto[]>([]);
  const [servers, setServers] = useState<ServerDto[]>([]);
  const [knownTags, setKnownTags] = useState<string[]>([]);
  const [hardBlacklist, setHardBlacklist] = useState<PatternDto[]>([]);
  const [selection, setSelection] = useState<{ kind: "rule"; id: string } | { kind: "new" } | null>(
    null,
  );
  const [error, setError] = useState<string | null>(null);
  const [loaded, setLoaded] = useState(false);

  const reload = () => {
    Promise.all([listRules(), listServers(), listKnownTags(), listHardBlacklist()])
      .then(([r, s, t, b]) => {
        setRules(r);
        setServers(s);
        setKnownTags(t);
        setHardBlacklist(b);
        setLoaded(true);
      })
      .catch((err) => setError(commandErrorMessage(err)));
  };

  useEffect(reload, []);

  // Issue #696: open the requested rule once the list has loaded. A rule that
  // no longer exists is ignored (the request is consumed either way).
  useEffect(() => {
    if (!initialRuleId || !loaded) return;
    if (rules.some((r) => r.id === initialRuleId)) {
      setSelection({ kind: "rule", id: initialRuleId });
    }
    onInitialRuleConsumed?.();
  }, [initialRuleId, loaded, rules, onInitialRuleConsumed]);

  const serverName = (id: string) => servers.find((s) => s.id === id)?.name ?? id;

  const scopeText = (rule: RuleDto): string => {
    const kind = scopeKind(rule.scope);
    if (kind === "global") return t("filterRules.scopeGlobal");
    if (kind === "server") {
      return t("filterRules.scopeServerPrefix", { name: serverName(scopeServerId(rule.scope)!) });
    }
    return t("filterRules.scopeTagPrefix", { tag: scopeTag(rule.scope) });
  };

  // Spec 0009, section 6: grouped by scope (Global first, then per server,
  // then per tag), sorted by priority within a group — the engine checks the
  // scope before the priority (Spec 0002, 3.2).
  const groups = useMemo(() => {
    const byKey = new Map<string, RuleGroup & { order: number }>();
    for (const rule of rules) {
      const key = scopeKey(rule.scope);
      if (!byKey.has(key)) {
        const kind = scopeKind(rule.scope);
        const order = kind === "global" ? 0 : kind === "server" ? 1 : 2;
        byKey.set(key, { key, label: scopeText(rule), order, rules: [] });
      }
      byKey.get(key)!.rules.push(rule);
    }
    for (const group of byKey.values()) {
      group.rules.sort((a, b) => b.priority - a.priority);
    }
    return [...byKey.values()].sort((a, b) => a.order - b.order || a.label.localeCompare(b.label));
    // eslint-disable-next-line react-hooks/exhaustive-deps -- grouping depends on rules, servers and the language only
  }, [rules, servers, t]);

  const movePriority = async (groupRules: RuleDto[], index: number, direction: -1 | 1) => {
    const otherIndex = index + direction;
    if (otherIndex < 0 || otherIndex >= groupRules.length) return;
    const a = groupRules[index];
    const b = groupRules[otherIndex];
    // Spec 0077, 3.2.3: nothing is written when one of the two rules has an
    // invalid pattern (the arrows are disabled anyway; the backend checks too).
    if (a.patternError || b.patternError) return;
    try {
      // One command swaps both priorities in one transaction.
      await swapRulePriorities(a.id, b.id);
      reload();
    } catch (err) {
      setError(commandErrorMessage(err));
    }
  };

  const handleDelete = async (id: string) => {
    try {
      await deleteRule(id);
      if (selection?.kind === "rule" && selection.id === id) setSelection(null);
      reload();
    } catch (err) {
      setError(commandErrorMessage(err));
    }
  };

  const selectedRule =
    selection?.kind === "rule" ? (rules.find((r) => r.id === selection.id) ?? null) : null;

  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-y-auto min-[1100px]:flex-row min-[1100px]:overflow-hidden">
      <div
        data-testid="filter-rules-list"
        className="min-w-0 flex-1 space-y-6 p-4 narrow:p-3 min-[1100px]:overflow-y-auto"
      >
        <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
          <h2 className="text-display text-fg">{t("filterRules.title")}</h2>
          <span className="font-mono text-sm text-fg-subtle">
            {t("filterRules.ruleCount", { count: rules.length })}
          </span>
          <div className="ml-auto flex items-center gap-2">
            {narrow && (
              <Button className="min-h-11" onClick={() => testerRef.current?.focus()}>
                {t("filterRules.testRules")}
              </Button>
            )}
            <Button
              variant="primary"
              className="narrow:min-h-11"
              onClick={() => setSelection({ kind: "new" })}
            >
              {t("filterRules.addRule")}
            </Button>
          </div>
        </div>

        {error && <p className="text-sm text-danger-fg">{error}</p>}

        <div className="space-y-4">
          {groups.length === 0 && <p className="text-sm text-fg-subtle">{t("filterRules.noRules")}</p>}
          {groups.map((group) => (
            <RuleTable
              key={group.key}
              group={group}
              scopeText={scopeText}
              onMove={movePriority}
              onEdit={(id) => setSelection({ kind: "rule", id })}
              onDelete={handleDelete}
            />
          ))}
        </div>

        {selection && (
          <RuleForm
            key={selection.kind === "rule" ? selection.id : "new"}
            rule={selectedRule}
            servers={servers}
            knownTags={knownTags}
            onSaved={() => {
              setSelection(null);
              reload();
            }}
            onCancel={() => setSelection(null)}
          />
        )}

        <HardBlacklistSection patterns={hardBlacklist} />
      </div>

      <RuleTester ref={testerRef} servers={servers} rules={rules} scopeText={scopeText} />
    </div>
  );
}
