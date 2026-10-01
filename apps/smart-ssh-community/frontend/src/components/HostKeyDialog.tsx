import { useEffect, useId, useRef } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import type { HostKeyInfo, HostKeyUserDecision } from "../types";
import { useDialogFocusTrap } from "../useDialogFocusTrap";

interface HostKeyDialogProps {
  event: HostKeyInfo;
  onDecision: (decision: HostKeyUserDecision) => void;
}

/**
 * Spec 0007 Teil 2, Punkt 1 / Spec 0005 Abschnitt 6, letzter Absatz: der
 * `Mismatch`-Fall bekommt bewusst einen visuell abweichenden, strengeren
 * Dialog (rot, Warnsymbol, expliziter MITM-Hinweis, andere Button-
 * Beschriftung) statt derselben Optik wie `Unknown` — ein geänderter
 * Host-Key ist ein deutlich ernsteres Signal als ein neuer, unbekannter.
 *
 * Spec 0100: Der Dialog ist allein mit der Tastatur sicher bedienbar und
 * wird von Screenreadern als modal angesagt — der Fokus-Fang/Escape-Teil
 * steckt im Hook `useDialogFocusTrap`, hier bleibt nur, was spezifisch zu
 * dieser Entscheidung gehört: welche Schaltfläche den Anfangsfokus trägt
 * und was Escape bedeutet (immer `reject`, nie `trust`).
 */
export function HostKeyDialog({ event, onDecision }: HostKeyDialogProps) {
  const { t } = useTranslation();
  const isMismatch = event.kind === "mismatch";
  const dialogRole = isMismatch ? "alertdialog" : "dialog";
  const headingId = useId();
  const descriptionId = useId();

  const containerRef = useRef<HTMLDivElement>(null);
  const rejectButtonRef = useRef<HTMLButtonElement>(null);
  // A4/A5-Invariante: `onDecision` wird für dieses Ereignis höchstens
  // einmal gerufen — schützt vor doppeltem Escape (T5) und vor
  // Klick-nach-Escape in derselben Lebensdauer, bevor der Aufrufer den
  // Dialog aus dem Baum entfernt. Bei einem neuen Ereignis (neue
  // Verbindungsanfrage) wird die Sperre zurückgesetzt.
  const decidedRef = useRef(false);
  useEffect(() => {
    decidedRef.current = false;
  }, [event]);

  function decide(decision: HostKeyUserDecision["decision"]) {
    if (decidedRef.current) return;
    decidedRef.current = true;
    onDecision({ decision });
  }

  const { onKeyDown } = useDialogFocusTrap({
    containerRef,
    initialFocusRef: rejectButtonRef,
    // A4: Escape lehnt ab, nie `trust`.
    onEscape: () => decide("reject"),
    resetKey: event,
  });

  // Unabhängiger Review-Pass (Spec 0014/0017): dieser Dialog kann von
  // `ServerList` ausgelöst werden, während gerade ein Session-Tab aktiv ist
  // (z. B. ein MCP-initiierter `connect()` auf einen neuen Server) — der
  // gesamte `MainScreen`-Zweig, in dem `ServerList` hängt, steht dann unter
  // `App.tsx`s `className="hidden"` (`display:none`). `display:none` auf
  // einem Vorfahren blendet auch `fixed`-positionierte Nachfahren aus, ein
  // sicherheitskritischer Host-Key-Mismatch-Dialog wäre also unsichtbar,
  // der Verbindungsaufbau bliebe unbestätigt hängen. Ein Portal nach
  // `document.body` umgeht das, ohne `ServerList`s Zustand/Logik verschieben
  // zu müssen.
  return createPortal(
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4">
      {/* A6.3: begründete `jsx-a11y`-Ausnahme, zeilengenau statt global —
       * `role` ist hier zweigabhängig (`alertdialog`/`dialog`, A1) und
       * damit kein String-Literal; die statische Analyse von
       * `no-static-element-interactions` erkennt nur Literale und verlangt
       * deshalb eine Rolle, die längst da ist (gemessen: ein Literal löst
       * stattdessen `no-noninteractive-element-interactions` aus — das
       * Zweig-Rendern über zwei fast identische Teilbäume nur für ein
       * statisches `role`-Literal würde die Komponente ohne Nutzen
       * duplizieren). */}
      {/* eslint-disable-next-line jsx-a11y/no-static-element-interactions */}
      <div
        ref={containerRef}
        // A1: `alertdialog` im geänderten Zweig (höheres Dringlichkeits-
        // signal für Screenreader), `dialog` im unbekannten Zweig.
        role={dialogRole}
        aria-modal="true"
        aria-labelledby={headingId}
        aria-describedby={descriptionId}
        onKeyDown={onKeyDown}
        className={`w-full max-w-md border p-0 shadow-xl ${
          isMismatch ? "border-red-600 bg-red-950/95" : "border-slate-600 bg-slate-800"
        }`}
      >
        {isMismatch ? (
          <>
            {/* Gefahren-Warnstreifen — Design-Import, Abschnitt "HOST-KEY
             * GEÄNDERT"-Dialog: diagonale Rot-Schwarz-Schraffur als
             * unübersehbarer Alarm-Kopf. */}
            <div
              className="h-3"
              style={{
                background:
                  "repeating-linear-gradient(135deg, var(--color-red-600) 0 10px, var(--color-red-950) 10px 20px)",
              }}
            />
            <div className="p-6">
              <div className="font-mono text-[11px] tracking-[0.18em] text-red-400 uppercase">
                {t("hostKeyDialog.mismatchLabel")}
              </div>
              <h2 id={headingId} className="font-heading mt-1 mb-2 text-2xl leading-tight font-bold text-red-100">
                {t("hostKeyDialog.mismatchHeading")}
              </h2>
              <p id={descriptionId} className="mb-4 text-sm text-red-200/90">
                {t("hostKeyDialog.mismatchBodyBeforeHost")}
                <strong>{event.host}:{event.port}</strong>
                {t("hostKeyDialog.mismatchBodyAfterHost")}
              </p>
              <div className="mb-4 grid grid-cols-2 gap-px border border-red-700/40 bg-red-700/25">
                <div className="flex flex-col gap-1 bg-red-950 p-3">
                  <span className="font-heading text-[11px] font-semibold tracking-wide text-red-400/80 uppercase">
                    {t("hostKeyDialog.known")}
                  </span>
                  <span className="font-mono text-xs break-all text-emerald-300">
                    {event.expectedFingerprint}
                  </span>
                </div>
                <div className="flex flex-col gap-1 bg-red-950 p-3">
                  <span className="font-heading text-[11px] font-semibold tracking-wide text-red-300 uppercase">
                    {t("hostKeyDialog.offeredNow")}
                  </span>
                  <span className="font-mono text-xs break-all text-red-300">
                    {event.fingerprint}
                  </span>
                </div>
              </div>

              <div className="flex gap-2 border-t border-red-700/30 pt-4">
                <button
                  ref={rejectButtonRef}
                  type="button"
                  onClick={() => decide("reject")}
                  className="font-heading flex-1 bg-red-600 px-3 py-2 text-sm font-bold tracking-wide text-red-50 hover:bg-red-500"
                >
                  {t("hostKeyDialog.cancelConnection")}
                </button>
                <button
                  type="button"
                  onClick={() => decide("trust")}
                  className="font-heading flex-1 border border-white/15 px-3 py-2 text-sm font-semibold tracking-wide text-red-200 hover:bg-white/6"
                >
                  {t("hostKeyDialog.trustAnyway")}
                </button>
              </div>
            </div>
          </>
        ) : (
          <div className="p-6">
            <h2 id={headingId} className="font-heading mb-2 text-lg font-semibold text-slate-100">
              {t("hostKeyDialog.unknownHeading")}
            </h2>
            <p id={descriptionId} className="mb-4 text-sm text-slate-300">
              {t("hostKeyDialog.unknownBodyBeforeHost")}
              <strong>{event.host}:{event.port}</strong>
              {t("hostKeyDialog.unknownBodyAfterHost")}
            </p>
            <div className="mb-4 border border-slate-700 bg-slate-950 p-2 font-mono text-xs text-slate-300">
              {event.fingerprint}
            </div>

            <div className="flex gap-2">
              <button
                ref={rejectButtonRef}
                type="button"
                onClick={() => decide("reject")}
                className="font-heading flex-1 border border-slate-600 px-3 py-2 text-sm font-semibold tracking-wide text-slate-100 hover:bg-slate-700"
              >
                {t("hostKeyDialog.reject")}
              </button>
              <button
                type="button"
                onClick={() => decide("trust")}
                className="font-heading flex-1 bg-indigo-600 px-3 py-2 text-sm font-semibold tracking-wide text-slate-950 hover:bg-indigo-500"
              >
                {t("hostKeyDialog.trust")}
              </button>
            </div>
          </div>
        )}
      </div>
    </div>,
    document.body,
  );
}
