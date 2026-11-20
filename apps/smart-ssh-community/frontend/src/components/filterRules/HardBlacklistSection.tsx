import { useTranslation } from "react-i18next";
import type { PatternDto } from "../../api/types";
import { Badge, Icon } from "../ui";

/** Spec 0009, section 6: read-only, clearly marked as not editable — the raw
 * patterns from `list_hard_blacklist`, one chip each, no Edit/Delete. */
export function HardBlacklistSection({ patterns }: { patterns: PatternDto[] }) {
  const { t } = useTranslation();
  return (
    <section className="space-y-3 border border-danger-border bg-danger-subtle p-3">
      <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
        <h3 className="text-label flex items-center gap-2 text-danger-fg">
          <Icon name="lock" />
          {t("filterRules.hardBlacklist")}
        </h3>
        <Badge tone="danger">{t("filterRules.hardBlacklistBadge")}</Badge>
        <p className="min-w-0 text-xs text-fg-muted sm:ml-auto">
          {t("filterRules.hardBlacklistHint")}
        </p>
      </div>
      <ul className="flex flex-wrap gap-1.5">
        {patterns.map((p, i) => (
          <li
            key={i}
            className="min-w-0 border border-danger-border bg-surface-sunken px-2 py-1 font-mono text-xs break-all text-fg-soft"
          >
            <span className="text-fg-subtle">{p.kind}:</span> {p.value}
          </li>
        ))}
      </ul>
    </section>
  );
}
