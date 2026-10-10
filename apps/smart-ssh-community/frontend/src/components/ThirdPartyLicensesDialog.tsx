import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
// Spec 0099, A1.8/A4.3: die Kennzeile der erzeugten Datei — eine Kopie für
// das ganze Frontend, gegen das Generierungs-Skript geprüft (Issue #7).
import { THIRD_PARTY_NOTICES_MARKER } from "../thirdPartyNoticesMarker";
import { ModalBackdrop } from "./ModalBackdrop";

// A4.4: eine relative Adresse — die Datei liegt im eigenen Bundle
// (`public/third-party-notices.txt`, von Vite nach `dist/` kopiert), ein
// `fetch` dorthin verlässt den eigenen Origin nie.
const NOTICES_URL = "/third-party-notices.txt";

type LoadState =
  | { status: "loading" }
  | { status: "available"; text: string }
  | { status: "unavailable" };

interface ThirdPartyLicensesDialogProps {
  onClose: () => void;
}

/** Spec 0099, A4: zeigt die beim Release-Build erzeugte Drittlizenz-Liste
 * als reinen Text in einer scrollbaren Ansicht. Ob die Liste vorhanden ist,
 * entscheidet die Kennzeile im Inhalt (A4.3) — ein Dev-Build liefert auf
 * eine unbekannte Route oft die SPA-Startseite (HTML, `res.ok` wäre wahr)
 * statt eines Ladefehlers, ein reiner Fehlerfall-Check würde das also
 * nicht erkennen. */
export function ThirdPartyLicensesDialog({ onClose }: ThirdPartyLicensesDialogProps) {
  const { t } = useTranslation();
  const [state, setState] = useState<LoadState>({ status: "loading" });

  useEffect(() => {
    let cancelled = false;
    fetch(NOTICES_URL)
      .then((res) => (res.ok ? res.text() : null))
      .catch(() => null)
      .then((text) => {
        if (cancelled) return;
        if (text && text.startsWith(THIRD_PARTY_NOTICES_MARKER)) {
          setState({ status: "available", text });
        } else {
          setState({ status: "unavailable" });
        }
      });
    return () => {
      cancelled = true;
    };
  }, []);

  return (
    <ModalBackdrop layer="app" className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4">
      <div className="flex max-h-[85vh] w-full max-w-2xl flex-col rounded border border-slate-600 bg-slate-900 p-6 shadow-xl">
        <h2 className="font-heading mb-3 text-lg font-semibold text-slate-100">
          {t("about.thirdPartyLicenses.title")}
        </h2>

        <div className="min-h-0 flex-1 overflow-y-auto rounded border border-slate-700 bg-slate-950 p-3">
          {state.status === "loading" && (
            <p className="text-sm text-slate-400">{t("about.thirdPartyLicenses.loading")}</p>
          )}
          {state.status === "unavailable" && (
            <p className="text-sm text-slate-400">{t("about.thirdPartyLicenses.unavailable")}</p>
          )}
          {state.status === "available" && (
            // A4.2 (adversarial, s. Test zu A4.2): `<pre>` mit reinem Textinhalt —
            // React escaped Kind-Text automatisch, kein
            // `dangerouslySetInnerHTML`, kein Markdown-Renderer. Ein
            // `<img onerror=…>` oder `<script>` in einer fremden
            // Lizenzdatei kann so nie zu einem echten DOM-Element werden.
            <pre className="select-text whitespace-pre-wrap break-words font-mono text-xs text-slate-200">
              {state.text}
            </pre>
          )}
        </div>

        <div className="mt-4 flex justify-end">
          <button
            type="button"
            onClick={onClose}
            className="rounded bg-indigo-600 px-3 py-1.5 text-sm font-semibold text-slate-950 hover:bg-indigo-500"
          >
            {t("common.close")}
          </button>
        </div>
      </div>
    </ModalBackdrop>
  );
}
