import { reasonList } from "./CoverageList";
import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { commandErrorMessage, explainConfirmation } from "../../api";
import { useMenuPlacement } from "../../hooks/useMenuPlacement";
import type { ExplainedSuggestionDto, SuggestionVerdictDto, SuggestionPatternDto, Scope } from "../../types";
import { Button } from "../ui";
import { TECHNICAL_INPUT_PROPS } from "../../technicalInputProps";

/** Spec 0011, Abschnitt 4: zusätzlicher Button neben Ausführen/Ablehnen,
 * öffnet ein kompaktes Dropdown mit Muster-Vorschlägen + Scope-Auswahl
 * (Default: aktueller Server). Klick auf einen Vorschlag ruft
 * `accept_and_create_rule` auf und schließt das Dropdown — der
 * Bestätigungsdialog selbst verschwindet dann über `onAccept`s
 * optimistisches `responded: true` (s. `ChatPanel.acceptWithRule`), genau
 * wie bei Ausführen/Ablehnen. */
export function QuickRuleButton({
  sessionId,
  actionId,
  command,
  wasEdited,
  serverId,
  onAccept,
}: {
  sessionId: string;
  actionId: string;
  command: string;
  /** Unabhängiger Review-Pass (Spec 0007/0008/0011): `true`, wenn `command`
   * vom ursprünglichen KI-Vorschlag abweicht — steuert, ob beim Akzeptieren
   * `editedCommand` mitgeschickt wird (s. `onAccept`-Doc-Kommentar an
   * `ConfirmActionForm.onAcceptWithRule`). */
  wasEdited: boolean;
  serverId: string;
  onAccept: (
    actionId: string,
    patterns: SuggestionPatternDto[],
    scope: Scope,
    editedCommand: string | null,
    verdict?: SuggestionVerdictDto,
  ) => void;
}) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  // Issue #627: the dropdown stays fully inside the window.
  const quickRuleWrapRef = useRef<HTMLDivElement>(null);
  const quickRuleMenuRef = useRef<HTMLDivElement>(null);
  const getQuickRuleAnchor = useCallback(() => quickRuleWrapRef.current, []);
  const quickRuleMenuStyle = useMenuPlacement(quickRuleMenuRef, getQuickRuleAnchor, open, {
    align: "end",
    gap: 4,
  });
  const [suggestions, setSuggestions] = useState<ExplainedSuggestionDto[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [scopeKind, setScopeKind] = useState<"server" | "global" | "tag">("server");
  const [tag, setTag] = useState("");

  const dropdownTriggerRef = useRef<HTMLButtonElement>(null);

  // Escape closes the popup and returns focus to its trigger (docs/ui-guidelines.md).
  useEffect(() => {
    if (!open) return;
    const onKeyDown = (e: globalThis.KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.stopPropagation();
      setOpen(false);
      dropdownTriggerRef.current?.focus();
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [open]);

  const toggleOpen = () => setOpen((o) => !o);

  const buildScope = (): Scope | null => {
    if (scopeKind === "global") return "Global";
    if (scopeKind === "server") return { Server: serverId };
    const trimmed = tag.trim();
    return trimmed ? { Tag: trimmed } : null;
  };

  // Issue #596: suggestions and their verdicts come from the backend and
  // depend on the command text and the chosen scope — reload on every change
  // (also covers a command edited after the first opening). A stale answer
  // is dropped.
  const scopeKey = JSON.stringify(buildScope());
  useEffect(() => {
    setSuggestions([]);
    const scope = buildScope();
    if (!open || !scope) return;
    let stale = false;
    setLoading(true);
    setError(null);
    explainConfirmation(sessionId, command, scope)
      .then((r) => {
        if (!stale) setSuggestions(r.suggestions);
      })
      .catch((err) => {
        if (!stale) setError(commandErrorMessage(err));
      })
      .finally(() => {
        if (!stale) setLoading(false);
      });
    return () => {
      stale = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, command, scopeKey, sessionId]);

  const handlePick = (suggestion: ExplainedSuggestionDto) => {
    const scope = buildScope();
    if (!scope) return;
    onAccept(
      actionId,
      suggestion.patterns,
      scope,
      wasEdited ? command : null,
      suggestion.verdict,
    );
    setOpen(false);
  };

  return (
    <div
      ref={quickRuleWrapRef}
      className="relative ml-auto inline-flex border border-accent/50 narrow:static narrow:ml-0 narrow:w-full"
    >
      <Button variant="ghost" size="sm" className="rounded-none px-3 narrow:min-h-11 narrow:flex-1" onClick={toggleOpen}>
        {t("confirmDialog.quickRule")}
      </Button>
      <Button
        ref={dropdownTriggerRef}
        variant="ghost"
        size="sm"
        onClick={toggleOpen}
        aria-label={t("confirmDialog.quickRuleDropdownAria")}
        aria-expanded={open}
        className="rounded-none border-l border-accent/40 px-2 font-mono narrow:min-h-11 narrow:min-w-11"
      >
        ▾
      </Button>
      {open && (
        <div ref={quickRuleMenuRef} style={quickRuleMenuStyle} className="fixed z-10 w-72 space-y-2 border border-accent/35 bg-surface p-2 shadow-lg narrow:fixed narrow:inset-x-0 narrow:top-auto narrow:bottom-0 narrow:mt-0 narrow:max-h-[70vh] narrow:w-auto narrow:overflow-y-auto narrow:p-3">
          <div className="font-heading border-b border-surface-raised pb-2 text-xs font-semibold tracking-wide text-fg-subtle uppercase">
            {t("confirmDialog.newRulePatternHeading")}
          </div>
          <div className="flex gap-3 text-xs text-fg-muted">
            <label className="flex items-center gap-1">
              <input
                type="radio"
                checked={scopeKind === "server"}
                onChange={() => setScopeKind("server")}
              />
              {t("confirmDialog.scopeThisServer")}
            </label>
            <label className="flex items-center gap-1">
              <input
                type="radio"
                checked={scopeKind === "global"}
                onChange={() => setScopeKind("global")}
              />
              {t("confirmDialog.scopeGlobal")}
            </label>
            <label className="flex items-center gap-1">
              <input
                type="radio"
                checked={scopeKind === "tag"}
                onChange={() => setScopeKind("tag")}
              />
              {t("confirmDialog.scopeTag")}
            </label>
          </div>
          {scopeKind === "tag" && (
            <input
              {...TECHNICAL_INPUT_PROPS}
              type="text"
              value={tag}
              onChange={(e) => setTag(e.target.value)}
              placeholder={t("confirmDialog.tagNamePlaceholder")}
              className="w-full border border-border-strong bg-surface px-2 py-1 font-mono text-xs text-fg"
            />
          )}
          {loading && <p className="text-xs text-fg-subtle">{t("confirmDialog.loadingSuggestions")}</p>}
          {error && <p className="text-xs text-danger-fg">{error}</p>}
          {!loading && !error && suggestions.length === 0 && (
            <p className="text-xs text-fg-subtle">{t("confirmDialog.noSuggestions")}</p>
          )}
          <ul className="space-y-1">
            {suggestions.map((suggestion) => (
              <li key={suggestion.patterns.map((p) => p.patternValue).join("\n")}>
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() => handlePick(suggestion)}
                  className="w-full justify-start text-left font-mono break-all text-fg-soft narrow:min-h-11 narrow:h-auto"
                >
                  {t(`confirmDialog.quickRuleLabel.${suggestion.kind}`, {
                    names: suggestion.names.join(", "),
                    patterns: suggestion.patterns.map((p) => p.patternValue).join(", "),
                  })}
                </Button>
                <p
                  className={`px-3 pb-1 text-xs ${suggestion.verdict.autoExec ? "text-success-fg-soft" : "text-warning-fg-soft"}`}
                >
                  {suggestion.verdict.autoExec
                    ? t("confirmDialog.explain.verdictAuto")
                    : t("confirmDialog.explain.verdictAsk", {
                        reasons: reasonList(
                          t,
                          suggestion.verdict.reasons.map((r) => r.code),
                        ),
                      })}
                </p>
              </li>
            ))}
          </ul>
        </div>
      )}
    </div>
  );
}

