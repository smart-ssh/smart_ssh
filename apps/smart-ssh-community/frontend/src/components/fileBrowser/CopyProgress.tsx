import { useTranslation } from "react-i18next";
import { sftpCancelCopyContent } from "../../api/api";
import { Button } from "../ui";
import { StatusSquare } from "../ui/StatusSquare";
import { formatMb } from "./shared";
import type { CopyJob } from "./useFileClipboard";

/** Issue #357: progress and cancel button of a running "copy content/file". */
export function CopyProgress({ job }: { job: CopyJob }) {
  const { t } = useTranslation();
  return (
    <output className="flex items-center gap-2 border-b border-border px-3 py-1.5 text-xs text-fg-soft">
      <StatusSquare tone="accent" pulse />
      <span className="min-w-0 flex-1 truncate font-mono tabular-nums">
        {job.total !== null
          ? t("fileBrowser.copyProgress", { done: formatMb(job.done), total: formatMb(job.total) })
          : t("fileBrowser.copyProgressUnknownTotal", { done: formatMb(job.done) })}
      </span>
      <Button
        size="sm"
        onClick={() => {
          void sftpCancelCopyContent(job.requestId).catch(() => {});
        }}
      >
        {t("common.cancel")}
      </Button>
    </output>
  );
}
