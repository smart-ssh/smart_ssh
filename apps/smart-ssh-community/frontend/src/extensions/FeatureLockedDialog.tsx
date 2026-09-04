/** Einfacher Hinweis-Dialog für `FeatureLocked`-Fehler (Spec 0038,
 * Abschnitt 4) — kein vollständiges Upgrade-Flow-Design in diesem Schritt
 * (folgt, sobald es mehr als ein gegatetes Feature gibt), nur die
 * Infrastruktur: einmalig in `App.tsx` gemountet, abonniert
 * `featureLockedBus` und zeigt den zuletzt gemeldeten Sperr-Grund an.
 *
 * Unabhängiger Review-Pass: `App.tsx` mountet diesen Dialog innerhalb von
 * `MainScreen`, dessen Wrapper `display:none` bekommt, sobald ein
 * Session-Tab aktiv ist (`App.tsx`s `className={activeSessionId === null ?
 * … : "hidden"}`) — `display:none` blendet auch `fixed`-positionierte
 * Nachfahren aus (dasselbe Problem, das `HostKeyDialog`/
 * `FirstRunNoticeScreen` bereits per Portal lösen, s. dortige Kommentare).
 * Ein `FeatureLocked`-Fehler tritt aber gerade bei den meisten Commands
 * (`send_chat_message`, `sftp_*`, `export_document`, …) typischerweise bei
 * aktivem Session-Tab auf — ohne Portal wäre dieser Dialog also fast immer
 * unsichtbar. `createPortal` nach `document.body` behebt das. */

import { createPortal } from "react-dom";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import type { FeatureLockedPayload } from "./entitlements";
import { subscribeFeatureLocked } from "./featureLockedBus";

export function FeatureLockedDialog() {
  const { t } = useTranslation();
  const [locked, setLocked] = useState<FeatureLockedPayload | null>(null);

  useEffect(() => subscribeFeatureLocked(setLocked), []);

  if (!locked) return null;

  return createPortal(
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4">
      <div className="w-full max-w-sm space-y-3 rounded border border-slate-700 bg-slate-900 p-5 text-slate-100">
        <h2 className="font-heading text-sm font-semibold tracking-wide">
          {t("featureLocked.title")}
        </h2>
        <p className="text-sm text-slate-300">
          {t("featureLocked.body", { feature: locked.feature, tier: locked.tier })}
        </p>
        <button
          type="button"
          onClick={() => setLocked(null)}
          className="w-full rounded border border-slate-600 bg-slate-800 px-3 py-1.5 text-sm font-semibold hover:bg-slate-700"
        >
          {t("featureLocked.close")}
        </button>
      </div>
    </div>,
    document.body,
  );
}
