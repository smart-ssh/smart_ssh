import { type FormEvent, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  commandErrorMessage,
  createRule,
  deleteRule,
  evaluateExplained,
  listHardBlacklist,
  listKnownTags,
  listRules,
  listServers,
  updateRule,
} from "../api";
import { translateErrorCode } from "../errorCodes";
import type {
  EvaluationTraceDto,
  PatternDto,
  PatternType,
  RuleAction,
  RuleDto,
  RuleInput,
  Scope,
  ServerDto,
} from "../types";

type ScopeKind = "global" | "server" | "tag";

function scopeKind(scope: Scope): ScopeKind {
  if (scope === "Global") return "global";
  if ("Server" in scope) return "server";
  return "tag";
}

function scopeServerId(scope: Scope): string | null {
  return typeof scope === "object" && "Server" in scope ? scope.Server : null;
}

function scopeTag(scope: Scope): string | null {
  return typeof scope === "object" && "Tag" in scope ? scope.Tag : null;
}

function scopeKey(scope: Scope): string {
  const kind = scopeKind(scope);
  if (kind === "server") return `server:${scopeServerId(scope)}`;
  if (kind === "tag") return `tag:${scopeTag(scope)}`;
  return "global";
}

function ruleToInput(rule: RuleDto): RuleInput {
  return {
    patternType: rule.patternType,
    patternValue: rule.patternValue,
    action: rule.action,
    scope: rule.scope,
    priority: rule.priority,
  };
}

const ACTION_COLORS: Record<RuleAction, string> = {
  Allow: "bg-emerald-900 text-emerald-300",
  Confirm: "bg-amber-900 text-amber-300",
  Deny: "bg-red-900 text-red-300",
};

/** Ampel-Randfarbe je Regel-Aktion (Spec 0009) — Design-Import, Regeln-
 * Bildschirm: farbig markierter linker Kartenrand statt nur der Text-Badge. */
const ACTION_BORDER: Record<RuleAction, string> = {
  Allow: "border-l-emerald-600",
  Confirm: "border-l-amber-600",
  Deny: "border-l-red-600",
};

/** Spec 0009, Abschnitt 6: Regel-Liste (gruppiert nach Scope, Auf/Ab statt
 * Drag-and-Drop), Hard-Blacklist-Sektion, Regel-Formular, Testen-Panel — in
 * einer Datei wie schon `ServerForm.tsx` (Spec 0008), um das Zusammenspiel
 * von Liste/Formular/Test nicht über mehrere Dateien zu verteilen. */
export function FilterRulesView() {
  const { t } = useTranslation();
  const [rules, setRules] = useState<RuleDto[]>([]);
  const [servers, setServers] = useState<ServerDto[]>([]);
  const [knownTags, setKnownTags] = useState<string[]>([]);
  const [hardBlacklist, setHardBlacklist] = useState<PatternDto[]>([]);
  const [selection, setSelection] = useState<{ kind: "rule"; id: string } | { kind: "new" } | null>(
    null,
  );
  const [error, setError] = useState<string | null>(null);

  const reload = () => {
    Promise.all([listRules(), listServers(), listKnownTags(), listHardBlacklist()])
      .then(([r, s, t, b]) => {
        setRules(r);
        setServers(s);
        setKnownTags(t);
        setHardBlacklist(b);
      })
      .catch((err) => setError(commandErrorMessage(err)));
  };

  useEffect(reload, []);

  const serverName = (id: string) => servers.find((s) => s.id === id)?.name ?? id;

  // Spec 0009, Abschnitt 6: "gruppiert nach Scope (Global zuerst, dann pro
  // Server, dann pro Tag) — innerhalb einer Gruppe sortiert nach priority".
  const groups = useMemo(() => {
    const byKey = new Map<string, { label: string; order: number; rules: RuleDto[] }>();
    for (const rule of rules) {
      const key = scopeKey(rule.scope);
      if (!byKey.has(key)) {
        const kind = scopeKind(rule.scope);
        const label =
          kind === "global"
            ? t("filterRules.scopeGlobal")
            : kind === "server"
              ? t("filterRules.scopeServerPrefix", { name: serverName(scopeServerId(rule.scope)!) })
              : t("filterRules.scopeTagPrefix", { tag: scopeTag(rule.scope) });
        const order = kind === "global" ? 0 : kind === "server" ? 1 : 2;
        byKey.set(key, { label, order, rules: [] });
      }
      byKey.get(key)!.rules.push(rule);
    }
    for (const group of byKey.values()) {
      group.rules.sort((a, b) => b.priority - a.priority);
    }
    return [...byKey.values()].sort((a, b) => a.order - b.order || a.label.localeCompare(b.label));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [rules, servers, t]);

  const movePriority = async (groupRules: RuleDto[], index: number, direction: -1 | 1) => {
    const otherIndex = index + direction;
    if (otherIndex < 0 || otherIndex >= groupRules.length) return;
    const a = groupRules[index];
    const b = groupRules[otherIndex];
    try {
      // Tauscht die Prioritätswerte zweier benachbarter Regeln, statt sie
      // nur um 1 zu verschieben — vermeidet, dass wiederholtes Klicken
      // Prioritätswerte "aufbraucht"/kollidieren lässt, und ergibt eine
      // stabile Neusortierung innerhalb der Gruppe.
      await updateRule(a.id, { ...ruleToInput(a), priority: b.priority });
      await updateRule(b.id, { ...ruleToInput(b), priority: a.priority });
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
    <div className="min-h-0 flex-1 overflow-y-auto p-4">
      <div className="max-w-3xl space-y-6">
        <div className="flex items-center justify-between">
          <h2 className="font-heading text-2xl font-bold tracking-wide text-slate-100">
            {t("filterRules.title")}
          </h2>
          <button
            type="button"
            onClick={() => setSelection({ kind: "new" })}
            className="font-heading bg-indigo-600 px-3 py-1.5 text-sm font-semibold tracking-wide text-slate-950 hover:bg-indigo-500"
          >
            {t("filterRules.addRule")}
          </button>
        </div>

        {error && <p className="text-sm text-red-400">{error}</p>}

        <div className="space-y-4">
          {groups.length === 0 && (
            <p className="text-sm text-slate-400">{t("filterRules.noRules")}</p>
          )}
          {groups.map((group) => (
            <div key={group.label}>
              <h3 className="font-heading mb-1 text-sm font-semibold tracking-[0.14em] text-slate-400 uppercase">
                {group.label}
              </h3>
              <ul className="space-y-1">
                {group.rules.map((rule, index) => (
                  <li
                    key={rule.id}
                    className={`flex items-center gap-2 border border-l-4 border-slate-700 bg-slate-800/60 px-3 py-2 text-sm ${ACTION_BORDER[rule.action]}`}
                  >
                    <span
                      className={`font-heading w-[70px] shrink-0 px-2 py-0.5 text-xs font-semibold tracking-wide uppercase ${ACTION_COLORS[rule.action]}`}
                    >
                      {rule.action}
                    </span>
                    <code className="flex-1 truncate font-mono text-slate-200">
                      {rule.patternType}: {rule.patternValue}
                    </code>
                    <span className="font-mono text-xs text-slate-500">
                      {t("filterRules.priority", { value: rule.priority })}
                    </span>
                    <button
                      type="button"
                      onClick={() => movePriority(group.rules, index, -1)}
                      disabled={index === 0}
                      className="bg-slate-700 px-1.5 py-0.5 text-xs hover:bg-slate-600 disabled:opacity-30"
                      aria-label={t("filterRules.increasePriority")}
                    >
                      ↑
                    </button>
                    <button
                      type="button"
                      onClick={() => movePriority(group.rules, index, 1)}
                      disabled={index === group.rules.length - 1}
                      className="bg-slate-700 px-1.5 py-0.5 text-xs hover:bg-slate-600 disabled:opacity-30"
                      aria-label={t("filterRules.decreasePriority")}
                    >
                      ↓
                    </button>
                    <button
                      type="button"
                      onClick={() => setSelection({ kind: "rule", id: rule.id })}
                      className="bg-slate-700 px-2 py-1 text-xs hover:bg-slate-600"
                    >
                      {t("common.edit")}
                    </button>
                    <button
                      type="button"
                      onClick={() => handleDelete(rule.id)}
                      className="bg-red-900 px-2 py-1 text-xs text-red-200 hover:bg-red-800"
                    >
                      {t("common.delete")}
                    </button>
                  </li>
                ))}
              </ul>
            </div>
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

        <TestPanel servers={servers} rules={rules} />
      </div>
    </div>
  );
}

interface RuleFormProps {
  /** `null` = Neuanlage. */
  rule: RuleDto | null;
  servers: ServerDto[];
  knownTags: string[];
  onSaved: () => void;
  onCancel: () => void;
}

/** Spec 0009, Abschnitt 6: Pattern-Typ mit passendem Eingabefeld, Aktion,
 * Scope-Auswahl (Global/Server/Tag — Tag als freie Eingabe mit
 * Vorschlägen aus `list_known_tags`), Priorität. */
function RuleForm({ rule, servers, knownTags, onSaved, onCancel }: RuleFormProps) {
  const { t } = useTranslation();
  const isCreate = rule === null;
  const [patternType, setPatternType] = useState<PatternType>(rule?.patternType ?? "glob");
  const [patternValue, setPatternValue] = useState(rule?.patternValue ?? "");
  const [action, setAction] = useState<RuleAction>(rule?.action ?? "Confirm");
  const [kind, setKind] = useState<ScopeKind>(rule ? scopeKind(rule.scope) : "global");
  const [serverId, setServerId] = useState<string>(rule ? (scopeServerId(rule.scope) ?? "") : "");
  const [tag, setTag] = useState<string>(rule ? (scopeTag(rule.scope) ?? "") : "");
  const [priority, setPriority] = useState(rule?.priority ?? 0);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const buildScope = (): Scope | null => {
    if (kind === "global") return "Global";
    if (kind === "server") return serverId ? { Server: serverId } : null;
    const trimmed = tag.trim();
    return trimmed ? { Tag: trimmed } : null;
  };

  const handleSubmit = async (e: FormEvent) => {
    e.preventDefault();
    const scope = buildScope();
    if (!scope) {
      setError(kind === "server" ? t("filterRules.serverRequired") : t("filterRules.tagRequired"));
      return;
    }
    const input: RuleInput = { patternType, patternValue, action, scope, priority };
    setSaving(true);
    setError(null);
    try {
      if (isCreate) {
        await createRule(input);
      } else {
        await updateRule(rule.id, input);
      }
      onSaved();
    } catch (err) {
      setError(commandErrorMessage(err));
    } finally {
      setSaving(false);
    }
  };

  const patternPlaceholder =
    patternType === "glob" ? "ls *" : patternType === "regex" ? "^systemctl\\s" : "ls -la";

  return (
    <form
      onSubmit={handleSubmit}
      className="space-y-3 rounded border border-slate-700 bg-slate-800/40 p-4"
    >
      <h3 className="font-heading text-sm font-semibold tracking-wide text-slate-100">
        {isCreate ? t("filterRules.newRule") : t("filterRules.editRule")}
      </h3>

      <div className="grid grid-cols-2 gap-3">
        <label className="block text-sm text-slate-300">
          {t("filterRules.patternType")}
          <select
            value={patternType}
            onChange={(e) => setPatternType(e.target.value as PatternType)}
            className="mt-1 w-full rounded border border-slate-600 bg-slate-900 px-2 py-1.5 text-slate-100"
          >
            <option value="glob">Glob</option>
            <option value="regex">Regex</option>
            <option value="exact">Exact</option>
          </select>
        </label>
        <label className="block text-sm text-slate-300">
          {t("filterRules.pattern")}
          <input
            type="text"
            required
            value={patternValue}
            onChange={(e) => setPatternValue(e.target.value)}
            placeholder={patternPlaceholder}
            className="mt-1 w-full rounded border border-slate-600 bg-slate-900 px-2 py-1.5 font-mono text-slate-100"
          />
        </label>
      </div>

      <div className="grid grid-cols-2 gap-3">
        <label className="block text-sm text-slate-300">
          {t("filterRules.action")}
          <select
            value={action}
            onChange={(e) => setAction(e.target.value as RuleAction)}
            className="mt-1 w-full rounded border border-slate-600 bg-slate-900 px-2 py-1.5 text-slate-100"
          >
            <option value="Allow">Allow</option>
            <option value="Confirm">Confirm</option>
            <option value="Deny">Deny</option>
          </select>
        </label>
        <label className="block text-sm text-slate-300">
          {t("filterRules.priorityLabel")}
          <input
            type="number"
            value={priority}
            onChange={(e) => setPriority(Number(e.target.value))}
            className="mt-1 w-full rounded border border-slate-600 bg-slate-900 px-2 py-1.5 text-slate-100"
          />
        </label>
      </div>

      <div className="space-y-2">
        <span className="block text-sm text-slate-300">{t("filterRules.scope")}</span>
        <div className="flex gap-4 text-sm text-slate-300">
          <label className="flex items-center gap-1">
            <input
              type="radio"
              checked={kind === "global"}
              onChange={() => setKind("global")}
            />
            {t("filterRules.scopeGlobal")}
          </label>
          <label className="flex items-center gap-1">
            <input type="radio" checked={kind === "server"} onChange={() => setKind("server")} />
            {t("filterRules.scopeServerOption")}
          </label>
          <label className="flex items-center gap-1">
            <input type="radio" checked={kind === "tag"} onChange={() => setKind("tag")} />
            {t("filterRules.scopeTagOption")}
          </label>
        </div>
        {kind === "server" && (
          <select
            value={serverId}
            onChange={(e) => setServerId(e.target.value)}
            className="w-full rounded border border-slate-600 bg-slate-900 px-2 py-1.5 text-slate-100"
          >
            <option value="">{t("filterRules.selectServer")}</option>
            {servers.map((s) => (
              <option key={s.id} value={s.id}>
                {s.name}
              </option>
            ))}
          </select>
        )}
        {kind === "tag" && (
          <>
            <input
              list="known-tags"
              type="text"
              value={tag}
              onChange={(e) => setTag(e.target.value)}
              placeholder={t("filterRules.tagInputPlaceholder")}
              className="w-full rounded border border-slate-600 bg-slate-900 px-2 py-1.5 text-slate-100"
            />
            <datalist id="known-tags">
              {knownTags.map((t) => (
                <option key={t} value={t} />
              ))}
            </datalist>
          </>
        )}
      </div>

      {error && <p className="text-sm text-red-400">{error}</p>}

      <div className="flex gap-2">
        <button
          type="button"
          onClick={onCancel}
          className="rounded bg-slate-700 px-3 py-1.5 text-sm hover:bg-slate-600"
        >
          {t("common.cancel")}
        </button>
        <button
          type="submit"
          disabled={saving}
          className="rounded bg-indigo-600 px-3 py-1.5 text-sm font-medium text-white hover:bg-indigo-500 disabled:opacity-50"
        >
          {saving ? t("common.saving") : isCreate ? t("common.create") : t("common.save")}
        </button>
      </div>
    </form>
  );
}

/** Spec 0009, Abschnitt 6: read-only, deutlich als nicht bearbeitbar
 * gekennzeichnet — kein Bearbeiten-/Löschen-Button an diesen Einträgen. */
function HardBlacklistSection({ patterns }: { patterns: PatternDto[] }) {
  const { t } = useTranslation();
  return (
    <div className="space-y-2 border-t border-slate-700 pt-4">
      <h3 className="font-heading flex items-center gap-2 text-sm font-semibold tracking-wide text-slate-100">
        {t("filterRules.hardBlacklist")}
        <span className="rounded bg-slate-700 px-2 py-0.5 text-xs text-slate-300">
          {t("filterRules.hardBlacklistBadge")}
        </span>
      </h3>
      <p className="text-xs text-slate-400">{t("filterRules.hardBlacklistHint")}</p>
      <ul className="space-y-1">
        {patterns.map((p, i) => (
          <li
            key={i}
            className="rounded border border-slate-800 bg-slate-900/60 px-3 py-1.5 text-xs text-slate-300"
          >
            <span className="text-slate-500">{p.kind}:</span> <code>{p.value}</code>
          </li>
        ))}
      </ul>
    </div>
  );
}

type Decision = EvaluationTraceDto["decision"];

function decisionInfo(
  decision: Decision,
): { text: string; className: string; reason?: string; code?: string } {
  if (decision === "AutoExec") {
    return { text: "AutoExec", className: "bg-emerald-900 text-emerald-300" };
  }
  if ("Confirm" in decision) {
    return {
      text: "Confirm",
      className: "bg-amber-900 text-amber-300",
      reason: decision.Confirm.reason,
      code: decision.Confirm.code,
    };
  }
  return {
    text: "Deny",
    className: "bg-red-900 text-red-300",
    reason: decision.Deny.reason,
    code: decision.Deny.code,
  };
}

function DecisionBadge({ decision, big }: { decision: Decision; big?: boolean }) {
  const { text, className } = decisionInfo(decision);
  return (
    <span
      className={`font-heading inline-block px-2 py-0.5 tracking-wide uppercase ${big ? "text-sm font-semibold" : "text-xs font-medium"} ${className}`}
    >
      {text}
    </span>
  );
}

function TraceDetails({ trace, rules }: { trace: EvaluationTraceDto; rules: RuleDto[] }) {
  const { t } = useTranslation();
  const { reason, code } = decisionInfo(trace.decision);
  const matchedRule = trace.matchedRule ? rules.find((r) => r.id === trace.matchedRule) : null;
  return (
    <div className="mt-1 space-y-0.5 text-xs text-slate-400">
      {reason && <p>{translateErrorCode(t, code, reason)}</p>}
      {matchedRule && (
        <p>
          {t("filterRules.matchedRuleLabel")} <code>{matchedRule.patternValue}</code> (
          {matchedRule.action})
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

/** Rekursiv: ein Trace pro Teilkommando (Chaining) bzw. Command-
 * Substitution, s. `EvaluationTraceDto`-Doc-Kommentar. */
function TraceView({ trace, rules }: { trace: EvaluationTraceDto; rules: RuleDto[] }) {
  return (
    <div className="rounded border border-slate-700 bg-slate-900/60 p-2">
      <DecisionBadge decision={trace.decision} />
      <TraceDetails trace={trace} rules={rules} />
      {trace.subCommandTraces.length > 0 && (
        <div className="mt-2 space-y-1 border-l border-slate-700 pl-2">
          {trace.subCommandTraces.map((sub, i) => (
            <TraceView key={i} trace={sub} rules={rules} />
          ))}
        </div>
      )}
    </div>
  );
}

/** Spec 0009, Abschnitt 6: Beispielkommando + optionale Scope-Simulation,
 * `evaluate_explained` — bei Chaining jedes Teilkommando einzeln plus
 * hervorgehobene Gesamt-Entscheidung. */
function TestPanel({ servers, rules }: { servers: ServerDto[]; rules: RuleDto[] }) {
  const { t } = useTranslation();
  const [command, setCommand] = useState("");
  const [serverId, setServerId] = useState("");
  const [tags, setTags] = useState<string[]>([]);
  const [tagDraft, setTagDraft] = useState("");
  const [result, setResult] = useState<EvaluationTraceDto | null>(null);
  const [testing, setTesting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleAddTag = () => {
    const value = tagDraft.trim();
    if (value && !tags.includes(value)) setTags([...tags, value]);
    setTagDraft("");
  };

  const handleTest = async () => {
    if (!command.trim()) return;
    setTesting(true);
    setError(null);
    try {
      setResult(await evaluateExplained(command, { serverId: serverId || null, tags }));
    } catch (err) {
      setError(commandErrorMessage(err));
    } finally {
      setTesting(false);
    }
  };

  return (
    <div className="space-y-3 border-t border-slate-700 pt-4">
      <h3 className="font-heading text-sm font-semibold tracking-wide text-slate-100">
        {t("filterRules.testRules")}
      </h3>

      <label className="block text-sm text-slate-300">
        {t("filterRules.exampleCommand")}
        <textarea
          value={command}
          onChange={(e) => setCommand(e.target.value)}
          rows={2}
          placeholder="ls -la && rm -rf /tmp/x"
          className="mt-1 w-full rounded border border-slate-600 bg-slate-900 px-2 py-1.5 font-mono text-sm text-slate-100"
        />
      </label>

      <div className="grid grid-cols-2 gap-3">
        <label className="block text-sm text-slate-300">
          {t("filterRules.simulateServer")}
          <select
            value={serverId}
            onChange={(e) => setServerId(e.target.value)}
            className="mt-1 w-full rounded border border-slate-600 bg-slate-900 px-2 py-1.5 text-slate-100"
          >
            <option value="">{t("filterRules.noServerOption")}</option>
            {servers.map((s) => (
              <option key={s.id} value={s.id}>
                {s.name}
              </option>
            ))}
          </select>
        </label>
        <div className="block text-sm text-slate-300">
          {t("filterRules.simulateTags")}
          <div className="mt-1 flex flex-wrap items-center gap-1 rounded border border-slate-600 bg-slate-900 p-1.5">
            {tags.map((tag) => (
              <span
                key={tag}
                className="flex items-center gap-1 rounded bg-slate-700 px-2 py-0.5 text-xs text-slate-100"
              >
                {tag}
                <button
                  type="button"
                  onClick={() => setTags(tags.filter((tagValue) => tagValue !== tag))}
                  className="text-slate-400 hover:text-white"
                  aria-label={t("serverForm.removeTagAria", { tag })}
                >
                  ✕
                </button>
              </span>
            ))}
            <input
              type="text"
              value={tagDraft}
              onChange={(e) => setTagDraft(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter" || e.key === ",") {
                  e.preventDefault();
                  handleAddTag();
                }
              }}
              onBlur={handleAddTag}
              placeholder={t("serverForm.tagPlaceholder")}
              className="min-w-[6rem] flex-1 bg-transparent text-sm text-slate-100 outline-none"
            />
          </div>
        </div>
      </div>

      <button
        type="button"
        onClick={handleTest}
        disabled={testing || !command.trim()}
        className="rounded bg-indigo-600 px-3 py-1.5 text-sm font-medium text-white hover:bg-indigo-500 disabled:opacity-50"
      >
        {testing ? t("common.testing") : t("filterRules.test")}
      </button>

      {error && <p className="text-sm text-red-400">{error}</p>}

      {result && (
        <div className="space-y-3 rounded border border-slate-700 bg-slate-800/60 p-3">
          <div>
            <p className="mb-1 text-xs uppercase tracking-wide text-slate-400">
              {t("filterRules.overallDecision")}
            </p>
            <DecisionBadge decision={result.decision} big />
            {result.subCommandTraces.length === 0 && (
              <TraceDetails trace={result} rules={rules} />
            )}
          </div>
          {result.subCommandTraces.length > 0 && (
            <div className="space-y-1">
              <p className="text-xs uppercase tracking-wide text-slate-400">
                {t("filterRules.subCommands")}
              </p>
              {result.subCommandTraces.map((sub, i) => (
                <TraceView key={i} trace={sub} rules={rules} />
              ))}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
