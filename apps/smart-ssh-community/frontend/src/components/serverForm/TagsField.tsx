import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { TECHNICAL_INPUT_PROPS } from "../../technicalInputProps";

interface Props {
  tags: string[];
  tagDraft: string;
  setTagDraft: (value: string) => void;
  addTag: () => void;
  removeTag: (tag: string) => void;
  /** Rendered below the tag input (the local pseudo-server's own Save button). */
  children?: ReactNode;
}

/** The tag editor shared by the General tab of the local and remote form. */
export function TagsField({ tags, tagDraft, setTagDraft, addTag: handleAddTag, removeTag, children }: Props) {
  const { t } = useTranslation();
  return (
        <div className="block text-sm text-fg-muted">
          {t("serverForm.tags")}
          <div className="mt-1 flex flex-wrap items-center gap-1 rounded border border-border-strong bg-surface p-1.5">
            {tags.map((tag) => (
              <span
                key={tag}
                className="flex items-center gap-1 rounded bg-border px-2 py-0.5 text-xs text-fg"
              >
                {tag}
                <button
                  type="button"
                  onClick={() => removeTag(tag)}
                  className="rounded text-fg-subtle hover:text-fg-bright focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent-fg"
                  aria-label={t("serverForm.removeTagAria", { tag })}
                >
                  ✕
                </button>
              </span>
            ))}
            <input
              {...TECHNICAL_INPUT_PROPS}
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
              className="min-w-24 flex-1 rounded bg-transparent px-1 py-0.5 text-sm text-fg focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent-fg"
            />
          </div>
          {children}
        </div>
  );
}
