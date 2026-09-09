import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { getAppInfo } from "../api";
import type { AppInfoDto } from "../types";

/** Spec 0052, Abschnitt 3.2: "Über"-Kategorie der zweispaltigen Settings
 * (Spec 0050) — Version + Commit-Hash **sichtbar und kopierbar**, damit ein
 * Tester sie ohne manuelles Abtippen in einen Bug-Report übernehmen kann.
 *
 * Spec-Reviewer-Fund (Spec 0052, Review dieses Schritts): eine frühere
 * Fassung legte den Text *in* den Kopieren-Button — ein Mousedown auf
 * einem `<button>` startet in mehreren Browser-Engines aber keine
 * Textselektion, womit der von der Spec verlangte Minimalfall ("mind.
 * selektierbarer Text") am Ende weder per Klick noch per Markieren
 * funktioniert hätte. Text (`<code>`, `select-text`) und Kopieren-Button
 * sind deshalb jetzt getrennte Elemente: Markieren+⌘C/Strg+C funktioniert
 * unabhängig vom Button, der Button ist ein zusätzlicher Komfort-Weg.
 * Ein fehlgeschlagenes `navigator.clipboard.writeText` (Secure-Context-
 * gebunden, hier die erste Clipboard-Nutzung im Projekt) zeigt jetzt
 * sichtbar eine Fehlermeldung statt nur in der Konsole zu verschwinden —
 * der Text bleibt in jedem Fall markierbar. */
export function AboutSettings() {
  const { t } = useTranslation();
  const [info, setInfo] = useState<AppInfoDto | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [copyState, setCopyState] = useState<"idle" | "copied" | "failed">("idle");

  useEffect(() => {
    getAppInfo()
      .then(setInfo)
      .catch((err) => {
        console.warn("get_app_info fehlgeschlagen:", err);
        setLoadError(t("about.loadFailed"));
      });
  }, [t]);

  const handleCopy = async () => {
    if (!info) return;
    try {
      await navigator.clipboard.writeText(info.versionDisplay);
      setCopyState("copied");
    } catch (err) {
      console.warn("Konnte Version nicht in die Zwischenablage kopieren:", err);
      setCopyState("failed");
    } finally {
      setTimeout(() => setCopyState("idle"), 2000);
    }
  };

  return (
    <div className="space-y-4">
      {loadError && (
        <p className="rounded bg-red-950 px-3 py-2 text-sm text-red-300">{loadError}</p>
      )}
      {info && (
        <div>
          <span className="block text-xs text-slate-400">{t("about.version")}</span>
          <div className="mt-1 flex items-center gap-2">
            <code className="select-text rounded border border-slate-600 bg-slate-900 px-3 py-1.5 font-mono text-sm text-slate-100">
              {info.versionDisplay}
              {info.edition && ` · ${info.edition}`}
            </code>
            <button
              type="button"
              onClick={handleCopy}
              title={t("about.copyHint")}
              className="rounded border border-slate-600 px-2 py-1.5 text-xs text-slate-300 hover:bg-slate-700"
            >
              {t("about.copyButton")}
            </button>
            {copyState === "copied" && (
              <span className="text-xs text-emerald-400">{t("about.copied")}</span>
            )}
            {copyState === "failed" && (
              <span className="text-xs text-red-300">{t("about.copyFailed")}</span>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
