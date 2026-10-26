// Issue #633: modal dialog for "Test connection" in the server form. Shows the
// live step list (same component as the connecting tab), a Cancel button while
// the test runs and a Close button with the final result once it is done.
import { useTranslation } from "react-i18next";
import type { ConnectStepSnapshot, TestConnectionReport } from "../types";
import { ConnectProgressList } from "./ConnectProgressList";
import { TestResultBadge } from "./ServerForm";
import { Button, DialogFrame, Spinner } from "./ui";

interface Props {
  steps: ConnectStepSnapshot[];
  /** `null` while the test is running. */
  result: TestConnectionReport | null;
  /** Cancels the running test and closes the dialog. */
  onCancel: () => void;
  onClose: () => void;
}

export function TestConnectionDialog({ steps, result, onCancel, onClose }: Props) {
  const { t } = useTranslation();
  const running = result === null;
  const dismiss = running ? onCancel : onClose;
  return (
    <DialogFrame
      layer="app"
      title={
        <span className="flex items-center gap-2">
          {running && <Spinner />}
          {t("serverForm.testConnection")}
        </span>
      }
      onClose={dismiss}
      actions={
        running ? (
          <Button onClick={onCancel}>{t("common.cancel")}</Button>
        ) : (
          <Button variant="primary" onClick={onClose}>
            {t("connectTab.close")}
          </Button>
        )
      }
    >
      <div className="space-y-3">
        {steps.length > 0 ? (
          <ConnectProgressList steps={steps} />
        ) : (
          running && <p className="text-fg-subtle">{t("connectTab.stepsRunning")}</p>
        )}
        {result && (
          <output className="block" data-testid="test-connection-result">
            <TestResultBadge result={result} />
          </output>
        )}
      </div>
    </DialogFrame>
  );
}
