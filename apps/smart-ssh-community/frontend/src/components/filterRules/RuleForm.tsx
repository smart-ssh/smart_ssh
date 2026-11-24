import { type FormEvent, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { commandErrorCode, commandErrorMessage, createRule, updateRule } from "../../api/api";
import { translateErrorCode } from "../../api/errorCodes";
import type { PatternType, RuleAction, RuleDto, RuleInput, Scope, ServerDto } from "../../api/types";
import { TECHNICAL_INPUT_PROPS } from "../../lib/technicalInputProps";
import { Button, FIELD_CONTROL_CLASS, Field } from "../ui";
import { DialogFrame } from "../ui/DialogFrame";
import { type ScopeKind, scopeKind, scopeServerId, scopeTag } from "./scope";

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
export function RuleForm({ rule, servers, knownTags, onSaved, onCancel }: RuleFormProps) {
  const { t } = useTranslation();
  const isCreate = rule === null;
  const patternTypeRef = useRef<HTMLSelectElement>(null);
  const [patternType, setPatternType] = useState<PatternType>(rule?.patternType ?? "glob");
  const [patternValue, setPatternValue] = useState(rule?.patternValue ?? "");
  const [action, setAction] = useState<RuleAction>(rule?.action ?? "Confirm");
  const [kind, setKind] = useState<ScopeKind>(rule ? scopeKind(rule.scope) : "global");
  const [serverId, setServerId] = useState<string>(rule ? (scopeServerId(rule.scope) ?? "") : "");
  const [tag, setTag] = useState<string>(rule ? (scopeTag(rule.scope) ?? "") : "");
  const [priority, setPriority] = useState(rule?.priority ?? 0);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  /** Spec 0077, 3.1.4: Fehlertext der Bibliothek zu einem ungültigen
   * Muster — steht **zusätzlich** unter dem übersetzten Satz, weil er die
   * Stelle im Muster nennt. `translateErrorCode` ersetzt die `message` bei
   * bekanntem Code vollständig, sie ginge sonst verloren. */
  const [patternErrorDetail, setPatternErrorDetail] = useState<string | null>(null);
  /** Translated sentence for a rejected pattern; shown in the pattern
   * field's error slot (linked via aria-describedby). Other errors stay in
   * `error` (form-level alert). */
  const [patternError, setPatternError] = useState<string | null>(null);

  const buildScope = (): Scope | null => {
    if (kind === "global") return "Global";
    if (kind === "server") return serverId ? { Server: serverId } : null;
    const trimmed = tag.trim();
    return trimmed ? { Tag: trimmed } : null;
  };

  const handleSubmit = async (e: FormEvent) => {
    e.preventDefault();
    const scope = buildScope();
    setPatternErrorDetail(null);
    setPatternError(null);
    if (!scope) {
      setError(kind === "server" ? t("filterRules.serverRequired") : t("filterRules.tagRequired"));
      return;
    }
    const input: RuleInput = { patternType, patternValue, action, scope, priority };
    setSaving(true);
    setError(null);
    setPatternErrorDetail(null);
    setPatternError(null);
    try {
      if (isCreate) {
        await createRule(input);
      } else {
        await updateRule(rule.id, input);
      }
      onSaved();
    } catch (err) {
      // Spec 0077, 3.1.4: Für `FILTER_RULE_PATTERN_INVALID` den übersetzten
      // Satz zeigen UND darunter den Fehlertext der Bibliothek. Andere Codes
      // bleiben wie bisher beim rohen `message`-Text — diese Spec ändert nur
      // das Verhalten für den einen neuen Code.
      const code = commandErrorCode(err);
      const message = commandErrorMessage(err);
      if (code === "FILTER_RULE_PATTERN_INVALID") {
        setPatternError(translateErrorCode(t, code, message));
        setPatternErrorDetail(message);
      } else {
        setError(message);
      }
    } finally {
      setSaving(false);
    }
  };

  const patternPlaceholder =
    patternType === "glob" ? "ls *" : patternType === "regex" ? "^systemctl\\s" : "ls -la";

  return (
    <DialogFrame
      layer="app"
      size="lg"
      title={isCreate ? t("filterRules.newRule") : t("filterRules.editRule")}
      onClose={onCancel}
      onSubmit={handleSubmit}
      initialFocusRef={patternTypeRef}
      actions={
        <>
          <Button className="narrow:min-h-11" onClick={onCancel}>{t("common.cancel")}</Button>
          <Button type="submit" variant="primary" className="narrow:min-h-11" disabled={saving}>
            {saving ? t("common.saving") : isCreate ? t("common.create") : t("common.save")}
          </Button>
        </>
      }
    >
      <div className="space-y-3">
      <div className="grid grid-cols-2 gap-3 narrow:grid-cols-1">
        <Field label={t("filterRules.patternType")}>
          {(c) => (
            <select
              {...c}
              ref={patternTypeRef}
              value={patternType}
              onChange={(e) => setPatternType(e.target.value as PatternType)}
              className={`w-full ${FIELD_CONTROL_CLASS}`}
            >
              <option value="glob">Glob</option>
              <option value="regex">Regex</option>
              <option value="exact">Exact</option>
            </select>
          )}
        </Field>
        <Field
          label={t("filterRules.pattern")}
          error={
            patternError ? (
              <>
                {patternError}
                {/* Spec 0077, 3.1.4: library detail names the position in the pattern. */}
                {patternErrorDetail && (
                  <span className="mt-1 block font-mono text-xs text-danger-fg-soft/80">
                    {patternErrorDetail}
                  </span>
                )}
              </>
            ) : undefined
          }
        >
          {(c) => (
            <input
              {...c}
              {...TECHNICAL_INPUT_PROPS}
              type="text"
              required
              value={patternValue}
              onChange={(e) => setPatternValue(e.target.value)}
              placeholder={patternPlaceholder}
              className={`w-full font-mono ${FIELD_CONTROL_CLASS}`}
            />
          )}
        </Field>
      </div>

      <div className="grid grid-cols-2 gap-3 narrow:grid-cols-1">
        <Field label={t("filterRules.action")}>
          {(c) => (
            <select
              {...c}
              value={action}
              onChange={(e) => setAction(e.target.value as RuleAction)}
              className={`w-full ${FIELD_CONTROL_CLASS}`}
            >
              <option value="Allow">Allow</option>
              <option value="Confirm">Confirm</option>
              <option value="Deny">Deny</option>
            </select>
          )}
        </Field>
        <Field label={t("filterRules.priorityLabel")}>
          {(c) => (
            <input
              {...c}
              type="number"
              value={priority}
              onChange={(e) => setPriority(Number(e.target.value))}
              className={`w-full ${FIELD_CONTROL_CLASS}`}
            />
          )}
        </Field>
      </div>

      <div className="space-y-2">
        <span id="filter-rule-scope-label" className="block text-label">
          {t("filterRules.scope")}
        </span>
        <div role="radiogroup" aria-labelledby="filter-rule-scope-label" className="flex flex-wrap gap-x-4 gap-y-1 text-sm text-fg">
          <label className="flex items-center gap-1">
            <input
              type="radio"
              name="filter-rule-scope"
              checked={kind === "global"}
              onChange={() => setKind("global")}
            />
            {t("filterRules.scopeGlobal")}
          </label>
          <label className="flex items-center gap-1">
            <input type="radio" name="filter-rule-scope" checked={kind === "server"} onChange={() => setKind("server")} />
            {t("filterRules.scopeServerOption")}
          </label>
          <label className="flex items-center gap-1">
            <input type="radio" name="filter-rule-scope" checked={kind === "tag"} onChange={() => setKind("tag")} />
            {t("filterRules.scopeTagOption")}
          </label>
        </div>
        {kind === "server" && (
          <select
            value={serverId}
            onChange={(e) => setServerId(e.target.value)}
            aria-label={t("filterRules.scopeServerOption")}
            className={`w-full ${FIELD_CONTROL_CLASS}`}
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
              {...TECHNICAL_INPUT_PROPS}
              list="known-tags"
              type="text"
              value={tag}
              onChange={(e) => setTag(e.target.value)}
              aria-label={t("filterRules.scopeTagOption")}
              placeholder={t("filterRules.tagInputPlaceholder")}
              className={`w-full ${FIELD_CONTROL_CLASS}`}
            />
            <datalist id="known-tags">
              {knownTags.map((t) => (
                <option key={t} value={t} aria-label={t} />
              ))}
            </datalist>
          </>
        )}
      </div>

      {error && <p role="alert" className="text-sm text-danger-fg">{error}</p>}

      </div>
    </DialogFrame>
  );
}
