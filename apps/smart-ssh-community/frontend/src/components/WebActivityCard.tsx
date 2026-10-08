import { useTranslation } from "react-i18next";
import type { WebActivityDto } from "../types";

/** Fehlercodes der Web-Werkzeuge, für die es einen eigenen Text gibt
 * (`chat.webErrors.<code>`); alle anderen zeigen den allgemeinen Text
 * samt Code. */
const KNOWN_WEB_ERROR_CODES = new Set([
  "max_uses_exceeded",
  "too_many_requests",
  "url_not_accessible",
  "url_not_allowed",
  "url_not_in_prior_context",
  "unsupported_content_type",
  "query_too_long",
  "url_too_long",
  "invalid_tool_input",
  "request_too_large",
  "unavailable",
]);

/** Spec 0105: eine serverseitige Web-Recherche des Providers im Chat —
 * Suchanfrage bzw. URL, die von der KI zitierten Quellen und ggf. ein
 * Fehlerhinweis. Quellen erscheinen als Text, nicht als Link: ein Klick
 * soll weder das App-Fenster wegnavigieren noch still eine Verbindung
 * öffnen. */
export function WebActivityCard({ activity }: { activity: WebActivityDto }) {
  const { t } = useTranslation();
  const title =
    activity.kind === "search"
      ? t("chat.webSearchTitle", { query: activity.input })
      : t("chat.webFetchTitle", { url: activity.input });
  const errorText =
    activity.errorCode === null
      ? null
      : KNOWN_WEB_ERROR_CODES.has(activity.errorCode)
        ? t(`chat.webErrors.${activity.errorCode}`)
        : t("chat.webErrors.other", { code: activity.errorCode });

  return (
    <div
      className="rounded border border-sky-800/50 bg-slate-900 px-3 py-2 text-xs text-slate-300"
      data-testid="web-activity-card"
    >
      <p className="font-medium break-all text-sky-300">🌐 {title}</p>
      {errorText !== null && <p className="mt-1 text-amber-300">⚠ {errorText}</p>}
      {errorText === null && activity.cited.length > 0 && (
        <div className="mt-1">
          <p className="text-slate-400">{t("chat.webCitedSources")}</p>
          <ul className="mt-0.5 list-disc pl-4">
            {activity.cited.map((source) => (
              <li key={source.url} className="break-all">
                {source.title} <span className="text-slate-500">— {source.url}</span>
              </li>
            ))}
          </ul>
        </div>
      )}
      {errorText === null && activity.cited.length === 0 && (
        <p className="mt-1 text-slate-500">
          {activity.kind === "search"
            ? t("chat.webNoCitedSearch", { count: activity.results.length })
            : t("chat.webNoCitedFetch")}
        </p>
      )}
      {activity.contentTruncated && (
        <p className="mt-1 text-slate-500">{t("chat.webContentTruncated")}</p>
      )}
    </div>
  );
}
