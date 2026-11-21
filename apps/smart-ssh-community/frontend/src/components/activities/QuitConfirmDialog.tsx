// Issue #1327, Spec 0116: asks before the app quits while activities run.
// The backend holds the quit back and sends `quit-requested` with the
// running activities; "Quit anyway" confirms, "Keep running" does nothing.
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import type { TranslateFn, TranslationKey } from "../../i18nTypes";
import { confirmQuit, type Activity } from "../../api/activitiesApi";
import { onQuitRequested } from "../../api/events";
import { Button, DialogFrame, Icon } from "../ui";
import { kindIcon } from "./activityKind";

export function QuitConfirmDialog() {
  const { t } = useTranslation();
  const tx: TranslateFn = t as unknown as TranslateFn;
  const [running, setRunning] = useState<Activity[] | null>(null);
  const keepRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let disposed = false;
    onQuitRequested((p) => setRunning(p.activities))
      .then((un) => {
        if (disposed) un();
        else unlisten = un;
      })
      .catch((err) => console.warn("quit-requested listener failed:", err));
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  if (!running) return null;
  const resumable = running.some((a) => a.resumable);
  const close = () => setRunning(null);

  return (
    <DialogFrame
      layer="app"
      size="sm"
      role="alertdialog"
      title={
        <span className="inline-flex items-center gap-2">
          <Icon name="warn" className="text-warning-fg" />
          {t("activities.quitTitle")}
        </span>
      }
      onClose={close}
      initialFocusRef={keepRef}
      actions={
        <>
          <Button
            variant="danger"
            onClick={() => {
              void confirmQuit().catch((e) => {
                console.warn("quit_confirmed failed:", e);
                close();
              });
            }}
          >
            {t("activities.quitAnyway")}
          </Button>
          <Button ref={keepRef} variant="primary" onClick={close}>
            {t("activities.keepRunning")}
          </Button>
        </>
      }
    >
      <p>{t("activities.quitBody")}</p>
      <ul className="mt-2 space-y-1">
        {running.map((a) => (
          <li key={a.id} className="flex items-start gap-2 break-words">
            <Icon name={kindIcon(a.kind)} className="mt-0.5 shrink-0 text-fg-muted" />
            {tx(a.titleKey as TranslationKey, { ...a.titleParams })}
          </li>
        ))}
      </ul>
      {resumable && <p className="mt-2 text-fg-subtle">{t("activities.quitResumeHint")}</p>}
    </DialogFrame>
  );
}
