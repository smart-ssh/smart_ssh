import { useState } from "react";
import { useTranslation } from "react-i18next";
import { TECHNICAL_INPUT_PROPS } from "../../lib/technicalInputProps";

export interface TagInputProps {
  value: string[];
  onChange: (next: string[]) => void;
  placeholder?: string;
  "aria-label"?: string;
  /** Id of an element that labels the input (alternative to `aria-label`). */
  labelledBy?: string;
  /** Chips shown first that cannot be removed (e.g. tags inherited from the server). */
  readOnlyTags?: string[];
  /** Tooltip of the read-only chips. */
  readOnlyTitle?: string;
  disabled?: boolean;
}

/**
 * Chip input for free-form tags: Enter or comma adds the trimmed draft (no
 * empties, no duplicates), blur adds too, Backspace on an empty draft removes
 * the last tag, each chip has a remove button.
 */
export function TagInput({
  value,
  onChange,
  placeholder,
  "aria-label": ariaLabel,
  labelledBy,
  readOnlyTags = [],
  readOnlyTitle,
  disabled = false,
}: TagInputProps) {
  const { t } = useTranslation();
  const [draft, setDraft] = useState("");

  const commit = () => {
    const tag = draft.trim();
    if (tag && !value.includes(tag) && !readOnlyTags.includes(tag)) onChange([...value, tag]);
    setDraft("");
  };

  return (
    <div className="mt-1 flex flex-wrap items-center gap-1 rounded border border-border-strong bg-surface p-1.5">
      {readOnlyTags
        .filter((tag) => !value.includes(tag))
        .map((tag) => (
          <span
            key={`readonly-${tag}`}
            title={readOnlyTitle}
            className="flex items-center gap-1 rounded bg-accent-subtle-hover/60 px-2 py-0.5 text-xs text-accent-fg-softer"
          >
            {tag}
          </span>
        ))}
      {value.map((tag) => (
        <span key={tag} className="flex items-center gap-1 rounded bg-border px-2 py-0.5 text-xs text-fg">
          {tag}
          <button
            type="button"
            disabled={disabled}
            onClick={() => onChange(value.filter((other) => other !== tag))}
            className="rounded text-fg-subtle hover:text-fg-bright focus-ring-flush"
            aria-label={t("serverForm.removeTagAria", { tag })}
          >
            ✕
          </button>
        </span>
      ))}
      <input
        {...TECHNICAL_INPUT_PROPS}
        type="text"
        value={draft}
        disabled={disabled}
        onChange={(e) => setDraft(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter" || e.key === ",") {
            e.preventDefault();
            commit();
          } else if (e.key === "Backspace" && draft === "" && value.length > 0) {
            onChange(value.slice(0, -1));
          }
        }}
        onBlur={commit}
        placeholder={placeholder}
        aria-label={ariaLabel}
        aria-labelledby={labelledBy}
        className="min-w-24 flex-1 rounded bg-transparent px-1 py-0.5 text-sm text-fg focus-ring-flush"
      />
    </div>
  );
}
