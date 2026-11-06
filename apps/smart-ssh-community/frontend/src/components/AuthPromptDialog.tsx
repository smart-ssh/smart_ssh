import { useRef } from "react";
import { useTranslation } from "react-i18next";
import type { AuthPromptNeededEvent } from "../api/types";
import { TECHNICAL_INPUT_PROPS } from "../lib/technicalInputProps";
import { Button, DialogFrame, Field, FIELD_CONTROL_CLASS } from "./ui";

interface AuthPromptDialogProps {
  event: AuthPromptNeededEvent;
  /** Called once with the answers, in prompt order. */
  onSubmit: (answers: string[]) => void;
  onCancel: () => void;
}

/**
 * Issue #1185 (Spec 0005): the server's login questions (keyboard-interactive
 * or a password asked in a multi-step login). The name, the instructions and
 * the question texts come from the server and are shown as plain text only
 * (React text nodes, never HTML).
 *
 * The answers are secrets: the fields are uncontrolled, so no keystroke goes
 * through React state. On submit the values are read once, the fields are
 * cleared and the callback fires; the component keeps no copy.
 */
export function AuthPromptDialog({ event, onSubmit, onCancel }: AuthPromptDialogProps) {
  const { t } = useTranslation();
  const inputs = useRef<(HTMLInputElement | null)[]>([]);
  const firstRef = useRef<HTMLInputElement | null>(null);
  const doneRef = useRef(false);

  const fields =
    event.kind === "password"
      ? [{ text: t("authPrompt.password"), echo: false }]
      : event.prompts;

  function submit() {
    if (doneRef.current) return;
    doneRef.current = true;
    const answers = fields.map((_, i) => inputs.current[i]?.value ?? "");
    for (const input of inputs.current) {
      if (input) input.value = "";
    }
    onSubmit(answers);
  }

  function cancel() {
    if (doneRef.current) return;
    doneRef.current = true;
    for (const input of inputs.current) {
      if (input) input.value = "";
    }
    onCancel();
  }

  return (
    <DialogFrame
      layer="app"
      size="md"
      title={t("authPrompt.title", { hop: event.hop })}
      onClose={cancel}
      initialFocusRef={firstRef}
      onSubmit={(e) => {
        e.preventDefault();
        submit();
      }}
      actions={
        <>
          <Button type="button" onClick={cancel}>
            {t("common.cancel")}
          </Button>
          <Button type="submit" variant="primary">
            {t("authPrompt.submit")}
          </Button>
        </>
      }
    >
      <div className="space-y-3">
        {event.name ? (
          <p className="text-sm font-medium text-fg" data-testid="auth-prompt-name">
            {event.name}
          </p>
        ) : null}
        {event.instructions ? (
          <p className="text-sm whitespace-pre-line text-fg-muted" data-testid="auth-prompt-instructions">
            {event.instructions}
          </p>
        ) : null}
        {fields.map((field, i) => (
          <Field key={i} label={field.text}>
            {(p) => (
              <input
                {...p}
                {...TECHNICAL_INPUT_PROPS}
                ref={(el) => {
                  inputs.current[i] = el;
                  if (i === 0) firstRef.current = el;
                }}
                type={field.echo ? "text" : "password"}
                autoComplete={field.echo ? "one-time-code" : "current-password"}
                className={FIELD_CONTROL_CLASS}
              />
            )}
          </Field>
        ))}
        <p className="text-xs text-fg-subtle">{t("authPrompt.notStored")}</p>
      </div>
    </DialogFrame>
  );
}
