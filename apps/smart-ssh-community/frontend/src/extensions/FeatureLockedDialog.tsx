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
 * unsichtbar. `ModalBackdrop layer="app"` rendert nach `document.body` und behebt das. */

import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { tierLabel } from "./tiers";

import type { FeatureLockedPayload } from "./entitlements";
import { subscribeFeatureLocked } from "./featureLockedBus";
import { Button, DialogFrame } from "../components/ui";

export function FeatureLockedDialog() {
  const { t } = useTranslation();
  const [locked, setLocked] = useState<FeatureLockedPayload | null>(null);
  const closeRef = useRef<HTMLButtonElement>(null);

  useEffect(() => subscribeFeatureLocked(setLocked), []);

  if (!locked) return null;

  return (
    <DialogFrame
      layer="app"
      size="sm"
      title={t("featureLocked.title")}
      onClose={() => setLocked(null)}
      initialFocusRef={closeRef}
      actions={
        <Button ref={closeRef} variant="secondary" onClick={() => setLocked(null)}>
          {t("featureLocked.close")}
        </Button>
      }
    >
      <p>{t("featureLocked.body", { feature: locked.feature, tier: tierLabel(locked.tier, t) })}</p>
    </DialogFrame>
  );
}
