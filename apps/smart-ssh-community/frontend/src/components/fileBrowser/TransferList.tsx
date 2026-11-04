import type { TranslationKey } from "../../i18nTypes";
import { useTranslation } from "react-i18next";
import { sftpCancelTransfer } from "../../api/api";
import { formatBytes } from "../../lib/format";
import { formatDuration, formatSpeed } from "../../lib/transferRate";
import { Button } from "../ui";
import type { Transfer } from "./shared";

interface TransferListProps {
  transfers: Transfer[];
  sessionId: string;
  /** Removes a finished transfer row. */
  onDismiss: (id: string) => void;
}

/** Running and finished transfers with progress, cancel and result display. */
export function TransferList({ transfers, sessionId, onDismiss }: TransferListProps) {
  const { t } = useTranslation();
  return (
    <div className="space-y-1 border-b border-surface-raised bg-surface-sunken/60 px-2 py-1.5">
      {transfers.map((transfer) => (
        <div key={transfer.id} className="flex items-center gap-2 text-xs">
          {transfer.doneElevationEnded ? (
            <>
              <span className="text-success-fg">✓</span>
              <span className="min-w-0 flex-1 truncate text-fg-muted">
                {t("fileBrowser.transferDone", { name: transfer.fileName })}{" "}
                <span className="text-fg-subtle">{t("fileBrowser.transferElevationEnded")}</span>
              </span>
              <Button
                size="sm"
                variant="ghost"
                onClick={() => onDismiss(transfer.id)}
                aria-label={t("common.close")}
                title={t("common.close")}
              >
                ✕
              </Button>
            </>
          ) : transfer.stopped ? (
            <>
              <span className="text-warning-fg">■</span>
              <span className="min-w-0 flex-1 truncate text-fg-muted">
                {t(
                  transfer.stopped === "cancelled"
                    ? "fileBrowser.transferCancelled"
                    : "fileBrowser.transferStopped",
                  { name: transfer.fileName },
                )}
                {transfer.entriesDone !== undefined && (
                  <span className="text-fg-subtle">
                    {" — "}
                    {t("fileBrowser.entriesChanged", { count: transfer.entriesDone })}
                  </span>
                )}
                {transfer.incompleteRemotePath && (
                  <span className="text-warning-fg-soft">
                    {" "}
                    {t("fileBrowser.transferIncomplete", { path: transfer.incompleteRemotePath })}
                  </span>
                )}
              </span>
              <Button
                size="sm"
                variant="ghost"
                onClick={() => onDismiss(transfer.id)}
                aria-label={t("common.close")}
                title={t("common.close")}
              >
                ✕
              </Button>
            </>
          ) : transfer.error ? (
            <>
              <span className="text-danger-fg">✗</span>
              <span className="min-w-0 flex-1 truncate text-danger-fg-soft">
                {transfer.fileName}: {transfer.error}
                {transfer.entriesDone !== undefined &&
                  ` — ${t("fileBrowser.entriesChanged", { count: transfer.entriesDone })}`}
              </span>
              <Button
                size="sm"
                variant="ghost"
                onClick={() => onDismiss(transfer.id)}
                aria-label={t("common.close")}
                title={t("common.close")}
              >
                ✕
              </Button>
            </>
          ) : (
            <>
              <span className="inline-block h-2 w-2 shrink-0 animate-pulse rounded-full bg-accent-fg" />
              <span className="min-w-0 flex-1 truncate text-fg-muted">
                {t(
                  ({
                    upload: "fileBrowser.transferUpload",
                    download: "fileBrowser.transferDownload",
                    delete: "fileBrowser.transferDelete",
                    chmod: "fileBrowser.transferChmod",
                    chown: "fileBrowser.transferChown",
                  } satisfies Record<typeof transfer.kind, TranslationKey>)[transfer.kind],
                  { name: transfer.fileName },
                )}
                {transfer.totalBytes !== null && ` (${formatBytes(transfer.totalBytes)})`}
                {transfer.folder &&
                  ` — ${t("fileBrowser.transferFiles", {
                    done: transfer.folder.filesDone,
                    total: transfer.folder.filesTotal,
                  })}`}
              </span>
              {transfer.kind === "delete" || transfer.kind === "chmod" || transfer.kind === "chown" ? (
                <>
                  {transfer.entries?.total != null && transfer.entries.total > 0 ? (
                    <progress
                      className="h-1.5 w-24 shrink-0"
                      max={transfer.entries.total}
                      value={Math.min(transfer.entries.done, transfer.entries.total)}
                      aria-label={transfer.fileName}
                    />
                  ) : null}
                  <span className="shrink-0 whitespace-nowrap text-right tabular-nums text-fg-subtle">
                    {transfer.entries?.total != null
                      ? t("fileBrowser.entriesProgress", {
                          done: transfer.entries.done,
                          total: transfer.entries.total,
                        })
                      : (transfer.entries?.done ?? 0)}
                  </span>
                </>
              ) : transfer.totalBytes !== null && transfer.totalBytes > 0 ? (
                <progress
                  className="h-1.5 w-24 shrink-0"
                  max={transfer.totalBytes}
                  value={Math.min(transfer.bytesDone ?? 0, transfer.totalBytes)}
                  aria-label={transfer.fileName}
                />
              ) : null}
              {transfer.kind !== "delete" && transfer.kind !== "chmod" && transfer.kind !== "chown" && (
              <span className="shrink-0 whitespace-nowrap text-right tabular-nums text-fg-subtle">
                {formatBytes(transfer.bytesDone ?? 0)}
                {transfer.totalBytes !== null && ` / ${formatBytes(transfer.totalBytes)}`}
                {" · "}
                <span className="inline-block min-w-[5.5rem]">{formatSpeed(transfer.rate?.speed ?? null)}</span>
                {" · "}
                <span className="inline-block min-w-[6.5rem]">
                  {transfer.rate?.etaSeconds != null
                    ? t("fileBrowser.transferRemaining", { time: formatDuration(transfer.rate.etaSeconds) })
                    : formatDuration(null)}
                </span>
              </span>
              )}
              <Button
                size="sm"
                variant="ghost"
                onClick={() =>
                  sftpCancelTransfer(sessionId, transfer.id).catch((err) =>
                    console.warn("Konnte Transfer nicht abbrechen:", err),
                  )
                }
                aria-label={t("fileBrowser.transferCancel")}
                title={t("fileBrowser.transferCancel")}
              >
                ✕
              </Button>
            </>
          )}
        </div>
      ))}
    </div>
  );
}
