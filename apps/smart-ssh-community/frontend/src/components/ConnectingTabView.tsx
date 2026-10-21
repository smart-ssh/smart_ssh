// Issue #548: body of a tab whose connect attempt is running or has failed.
// On success the tab turns into the normal session view in place.
import { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import type { SessionTab } from "../useSessionTabs";
import { ConnectProgressList } from "./ConnectProgressList";
import { ConnectStepLog } from "./ConnectStepLog";
import { Button, Spinner } from "./ui";

interface Props {
  tab: SessionTab;
  isActiveTab: boolean;
  onRetry: () => void;
  onClose: () => void;
}

export function ConnectingTabView({ tab, isActiveTab, onRetry, onClose }: Props) {
  const { t } = useTranslation();
  const retryRef = useRef<HTMLButtonElement>(null);
  const failed = tab.phase === "failed";

  // Focus moves to "Retry" when the attempt fails (only if the tab is shown).
  useEffect(() => {
    if (failed && isActiveTab) retryRef.current?.focus();
  }, [failed, isActiveTab]);

  return (
    <main className="mx-auto w-full max-w-3xl flex-1 space-y-4 overflow-y-auto px-6 py-8">
      <h2 className="flex items-center gap-2 text-lg font-semibold">
        {failed ? (
          <span aria-hidden="true" className="text-red-400">
            ✗
          </span>
        ) : (
          <Spinner />
        )}
        {t(failed ? "connectTab.failed" : "connectTab.connecting", { name: tab.serverName })}
      </h2>
      {tab.progress.length > 0 ? (
        <ConnectProgressList steps={tab.progress} />
      ) : (
        !failed && <p className="text-sm text-slate-400">{t("connectTab.stepsRunning")}</p>
      )}
      {failed && tab.failure && (
        <div>
          <p role="alert" className="text-sm text-red-400">
            {tab.failure.message}
          </p>
          <ConnectStepLog steps={tab.failure.steps} />
        </div>
      )}
      <div className="flex gap-2">
        {failed ? (
          <>
            <Button ref={retryRef} variant="primary" onClick={onRetry}>
              {t("connectTab.retry")}
            </Button>
            <Button onClick={onClose}>{t("connectTab.close")}</Button>
          </>
        ) : (
          <Button onClick={onClose}>{t("connectTab.cancel")}</Button>
        )}
      </div>
    </main>
  );
}
