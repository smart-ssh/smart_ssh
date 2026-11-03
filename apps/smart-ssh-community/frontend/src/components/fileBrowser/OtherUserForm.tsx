import type { CSSProperties, Ref } from "react";
import { useTranslation } from "react-i18next";
import { TECHNICAL_INPUT_PROPS } from "../../technicalInputProps";
import { Button, FIELD_CONTROL_CLASS } from "../ui";

interface OtherUserFormProps {
  formRef: Ref<HTMLFormElement>;
  inputRef: Ref<HTMLInputElement>;
  style: CSSProperties | undefined;
  narrow: boolean;
  draft: string;
  setDraft: (value: string) => void;
  enablingElevation: boolean;
  /** Called with the trimmed, non-empty user name. */
  onSubmit: (user: string) => void;
}

/** Target-user form of the elevation menu (Spec 0067). */
export function OtherUserForm({
  formRef,
  inputRef,
  style,
  narrow,
  draft,
  setDraft,
  enablingElevation,
  onSubmit,
}: OtherUserFormProps) {
  const { t } = useTranslation();
  return (
    <form
      ref={formRef}
      style={style}
      className="fixed z-20 flex w-64 flex-col gap-1.5 rounded border border-border-strong bg-surface-raised p-2 shadow-lg narrow:fixed narrow:inset-x-0 narrow:bottom-0 narrow:left-0 narrow:top-auto narrow:mt-0 narrow:w-full narrow:rounded-b-none narrow:p-3"
      onSubmit={(e) => {
        e.preventDefault();
        const user = draft.trim();
        if (!user) return;
        onSubmit(user);
      }}
    >
      <label className="text-xs" htmlFor="file-elevation-target-user">
        {t("fileElevation.targetUser")}
      </label>
      <input
        {...TECHNICAL_INPUT_PROPS}
        id="file-elevation-target-user"
        ref={inputRef}
        value={draft}
        onChange={(e) => setDraft(e.target.value)}
        aria-describedby="file-elevation-target-user-help"
        className={`${FIELD_CONTROL_CLASS} px-1.5 py-1 font-mono text-xs`}
      />
      <p id="file-elevation-target-user-help" className="m-0 text-xs text-fg-muted">
        {t("fileElevation.targetUserHelp")}
      </p>
      <Button size={narrow ? "md" : "sm"} type="submit" disabled={enablingElevation || !draft.trim()}>
        {t("fileElevation.confirm")}
      </Button>
    </form>
  );
}
