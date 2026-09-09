import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { getAppInfo } from "../api";
import type { AppInfoDto } from "../types";

/** Spec 0052, Abschnitt 3.2: "Über"-Kategorie der zweispaltigen Settings
 * (Spec 0050) — Version + Commit-Hash **sichtbar und kopierbar**, damit ein
 * Tester sie ohne manuelles Abtippen in einen Bug-Report übernehmen kann.
 * Ein Klick auf den Text kopiert ihn (`navigator.clipboard`, Standard-Web-
 * API, kein Tauri-Plugin nötig); der Text bleibt daneben immer selektierbar
 * — falls `navigator.clipboard` in einer älteren WebView fehlt oder der
 * Zugriff verweigert wird, ist Markieren+Kopieren als Fallback weiterhin
 * möglich (Spec: "ideal ein Klick-zum-Kopieren, mindestens aber
 * selektierbarer Text"). */
export function AboutSettings() {
  const { t } = useTranslation();
  const [info, setInfo] = useState<AppInfoDto | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    getAppInfo()
      .then(setInfo)
      .catch((err) => {
        console.warn("get_app_info fehlgeschlagen:", err);
        setError(t("about.loadFailed"));
      });
  }, [t]);

  const handleCopy = async () => {
    if (!info) return;
    try {
      await navigator.clipboard.writeText(info.versionDisplay);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch (err) {
      // Kein Absturz, kein blockierender Fehler — der Text bleibt daneben
      // ohnehin selektierbar (s. Komponenten-Kommentar oben).
      console.warn("Konnte Version nicht in die Zwischenablage kopieren:", err);
    }
  };

  return (
    <div className="space-y-4">
      {error && <p className="rounded bg-red-950 px-3 py-2 text-sm text-red-300">{error}</p>}
      {info && (
        <div className="space-y-2">
          <div>
            <span className="block text-xs text-slate-400">{t("about.version")}</span>
            <button
              type="button"
              onClick={handleCopy}
              title={t("about.copyHint")}
              className="select-text rounded border border-slate-600 px-3 py-1.5 text-left font-mono text-sm text-slate-100 hover:bg-slate-700"
            >
              {info.versionDisplay}
              {info.edition && ` · ${info.edition}`}
            </button>
            {copied && <span className="ml-2 text-xs text-emerald-400">{t("about.copied")}</span>}
          </div>
        </div>
      )}
    </div>
  );
}
