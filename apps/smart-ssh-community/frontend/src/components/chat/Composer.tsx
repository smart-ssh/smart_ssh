import { type KeyboardEvent, type RefObject, type FormEvent, useState } from "react";
import { useTranslation } from "react-i18next";
import { ContextUsageIndicator } from "../ContextUsageIndicator";
import { VoiceInputControl } from "../VoiceInputControl";
import { Button, ErrorBoundary } from "../ui";
import { useIsNarrow } from "../../hooks/useIsNarrow";
import { formatBytes } from "../../lib/format";
import type { ComposerAction, ComposerAttachment, ComposerContext } from "../../extensions/registry";
import type { VoiceHandlers, VoicePhase } from "../../hooks/useVoiceInput";

interface ComposerProps {
  draft: string;
  attachments: ComposerAttachment[];
  composerError: string | null;
  composerCtx: ComposerContext;
  composerActions: ComposerAction[];
  invokeComposerAction: (action: ComposerAction) => void;
  contextUsage: { inputTokens: number; windowTokens: number } | null | undefined;
  inputRef: RefObject<HTMLTextAreaElement | null>;
  chatAreaRef: RefObject<HTMLElement | null>;
  onSubmit: (event: FormEvent) => void;
  onDraftChange: (value: string) => void;
  onKeyDown: (e: KeyboardEvent<HTMLTextAreaElement>) => void;
  voiceEnabled: boolean;
  voiceLocked: boolean;
  voiceHandlers: VoiceHandlers;
  onVoicePhaseChange: (phase: VoicePhase) => void;
}

/** The chat input bar: error line, attachment chips, usage, textarea, actions, send. */
export function Composer({
  draft,
  attachments,
  composerError,
  composerCtx,
  composerActions,
  invokeComposerAction,
  contextUsage,
  inputRef,
  chatAreaRef,
  onSubmit,
  onDraftChange,
  onKeyDown,
  voiceEnabled,
  voiceLocked,
  voiceHandlers,
  onVoicePhaseChange,
}: ComposerProps) {
  const { t } = useTranslation();
  // Spec 0113 §6a: below 640 px the composer actions sit behind a "+" menu.
  const narrow = useIsNarrow();
  const [composerMenuOpen, setComposerMenuOpen] = useState(false);
  return (
    <div className="border-t border-border bg-surface-raised">
          {composerError !== null && (
            <p role="alert" className="px-3 pt-2 text-xs text-danger-fg">
              {composerError}
            </p>
          )}
          {attachments.length > 0 && (
            <ul aria-label={t("chat.attachmentsLabel")} className="flex flex-wrap gap-2 px-3 pt-2">
              {attachments.map((a) => (
                <li
                  key={a.id}
                  className="flex items-center gap-2 rounded border border-border-strong bg-border px-2 py-1 text-xs text-fg"
                >
                  <span>
                    {a.name} · {formatBytes(a.size)}
                  </span>
                  {a.unredactedHint && (
                    <span className="text-warning-fg-soft">{t("chat.attachmentUnredacted")}</span>
                  )}
                  <button
                    type="button"
                    aria-label={t("chat.attachmentRemove", { name: a.name })}
                    onClick={() => composerCtx.removeAttachment(a.id)}
                    className="text-fg-muted hover:text-fg-bright focus-ring-flush"
                  >
                    ×
                  </button>
                </li>
              ))}
            </ul>
          )}
        {contextUsage && (
          <ContextUsageIndicator
            inputTokens={contextUsage.inputTokens}
            windowTokens={contextUsage.windowTokens}
          />
        )}
        <form onSubmit={onSubmit} className="flex flex-wrap items-end gap-2 p-3 narrow:[&>textarea]:order-first narrow:[&>textarea]:basis-full">
          <span className="self-start narrow:hidden py-1.5 font-mono text-sm leading-5 text-accent-strong">&gt;</span>
          {/* Spec 0055, Teil 1: Enter sendet, Shift+Enter fügt einen
              Zeilenumbruch ein (s. `onKeyDown`); `rows={1}` +
              `resize-none` + die Höhen-Effekt oben ergeben das
              Auto-Grow-bis-`max-h-40`-dann-scrollbar-Verhalten, `align`
              bleibt sonst wie beim bisherigen `<input>`. */}
          <textarea
            ref={inputRef}
            rows={1}
            value={draft}
            readOnly={voiceLocked}
            data-provisional={voiceLocked ? "true" : undefined}
            onChange={(e) => onDraftChange(e.target.value)}
            onKeyDown={onKeyDown}
            placeholder={t("chat.inputPlaceholder")}
            className={`max-h-40 min-w-0 flex-1 resize-none overflow-y-auto bg-transparent py-1.5 text-sm leading-5 ${voiceLocked ? "italic text-fg-subtle" : "text-fg"} placeholder:text-fg-subtle focus-ring-flush`}
          />
          {narrow ? (
            composerActions.length > 0 && (
              <div className="relative">
                <Button
                  aria-label={t("chat.composerMore")}
                  aria-expanded={composerMenuOpen}
                  onClick={() => setComposerMenuOpen((o) => !o)}
                  className="min-h-11 min-w-11"
                >
                  +
                </Button>
                {composerMenuOpen && (
                  <div className="absolute bottom-full left-0 z-20 mb-1 flex w-56 max-w-[calc(100vw-1.5rem)] flex-col gap-1 rounded border border-border bg-surface p-2 shadow-lg">
                    {composerActions.map((action) => (
                      <ErrorBoundary key={action.id} id={`composer-action:${action.id}`}><Button
                        title={action.disabled ? (action.disabledReason ?? action.label) : action.label}
                        disabled={action.disabled}
                        onClick={() => {
                          setComposerMenuOpen(false);
                          invokeComposerAction(action);
                        }}
                        className="min-h-11 w-full justify-start gap-2 text-left"
                      >
                        <span aria-hidden="true">{action.icon}</span>
                        {action.label}
                      </Button></ErrorBoundary>
                    ))}
                  </div>
                )}
              </div>
            )
          ) : (
            composerActions.map((action) => (
              <ErrorBoundary key={action.id} id={`composer-action:${action.id}`}><button
                type="button"
                aria-label={action.label}
                title={action.disabled ? (action.disabledReason ?? action.label) : action.label}
                disabled={action.disabled}
                onClick={() => invokeComposerAction(action)}
                className="rounded p-1.5 text-fg-muted hover:bg-border hover:text-fg-bright focus-ring-flush disabled:opacity-50"
              >
                {action.icon}
              </button></ErrorBoundary>
            ))
          )}
          {voiceEnabled && (
            <VoiceInputControl
              handlers={voiceHandlers}
              onPhaseChange={onVoicePhaseChange}
              panelRef={chatAreaRef}
            />
          )}
          <Button
            type="submit"
            variant="primary"
            className="narrow:ml-auto narrow:min-h-11 narrow:min-w-20"
            disabled={draft.trim().length === 0 || voiceLocked}
          >
            {t("chat.send")}
          </Button>
        </form>
    </div>
  );
}
