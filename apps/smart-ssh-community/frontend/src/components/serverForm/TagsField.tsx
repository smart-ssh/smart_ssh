import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { TagInput } from "../ui";

interface Props {
  tags: string[];
  setTags: (next: string[]) => void;
  /** Rendered below the tag input (the local pseudo-server's own Save button). */
  children?: ReactNode;
}

/** The tag editor shared by the General tab of the local and remote form. */
export function TagsField({ tags, setTags, children }: Props) {
  const { t } = useTranslation();
  return (
    <div className="flex flex-col gap-1">
      <span className="text-label">{t("serverForm.tags")}</span>
      <TagInput value={tags} onChange={setTags} placeholder={t("serverForm.tagPlaceholder")} aria-label={t("serverForm.tags")} />
      {children}
    </div>
  );
}
