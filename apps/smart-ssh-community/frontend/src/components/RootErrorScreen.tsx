import { useTranslation } from "react-i18next";
import { Button } from "./ui";

/** Full-window recovery screen shown by the root error boundary (issue #880,
 * Spec 0045): a render error outside every extension slot. */
export function RootErrorScreen() {
  const { t } = useTranslation();
  return (
    <main
      role="alert"
      className="flex min-h-screen w-full items-center justify-center bg-surface p-4 text-fg"
    >
      <div className="flex w-full max-w-md min-w-0 flex-col gap-3 break-words">
        <h1 className="text-lg font-semibold text-fg-bright">{t("errorBoundary.title")}</h1>
        <p className="text-sm text-fg-muted">{t("errorBoundary.body")}</p>
        <Button variant="primary" onClick={() => window.location.reload()} className="narrow:min-h-11">
          {t("errorBoundary.reload")}
        </Button>
      </div>
    </main>
  );
}
