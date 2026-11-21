import { useId } from "react";
import { useTranslation } from "react-i18next";
import type { AuthFormState } from "./serverFormModel";

export interface AuthKindOption {
  kind: AuthFormState["kind"];
  label: string;
}

/** Authentication choice as a segmented radio group. Native radio inputs
 * give the arrow-key behaviour (and the roving tab stop) for free; a
 * change goes through the same `onChange` as the former select. */
export function AuthKindSegments({
  value,
  options,
  onChange,
}: {
  value: AuthFormState["kind"];
  options: AuthKindOption[];
  onChange: (kind: AuthFormState["kind"]) => void;
}) {
  const { t } = useTranslation();
  const name = useId();
  return (
    <div
      role="radiogroup"
      aria-label={t("serverForm.authFieldset")}
      className="flex flex-wrap border border-border-strong"
    >
      {options.map((o) => (
        <label
          key={o.kind}
          className={`label-condensed relative flex min-h-8 flex-auto shrink-0 whitespace-nowrap cursor-pointer items-center justify-center px-3 text-xs narrow:min-h-11 has-[:focus-visible]:outline-2 has-[:focus-visible]:-outline-offset-2 has-[:focus-visible]:outline-focus ${
            value === o.kind
              ? "bg-accent-subtle text-accent-fg"
              : "text-fg-muted hover:bg-surface-hover"
          }`}
        >
          <input
            type="radio"
            name={name}
            value={o.kind}
            checked={value === o.kind}
            onChange={() => onChange(o.kind)}
            className="sr-only"
          />
          {o.label}
        </label>
      ))}
    </div>
  );
}
