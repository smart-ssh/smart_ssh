import { useEffect, useId, useRef } from "react";
import { useTranslation } from "react-i18next";
import type { HostKeyInfo, HostKeyUserDecision } from "../types";
import { useDialogFocusTrap } from "../useDialogFocusTrap";
import { ModalBackdrop } from "./ModalBackdrop";
import { Button } from "./ui";

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
  // Seit Issue #12 (ADR 0104) rendert das stets gemountete
  // `HostKeyPromptHost` an der `App`-Wurzel den Verbindungs-Dialog statt
  // `ServerList`; das Portal bleibt trotzdem nötig — `ServerForm` zeigt
  // diesen Dialog für die Test-Verbindung aus seinem eigenen (ausblendbaren)
  // Teilbaum heraus.
  return (
    <ModalBackdrop layer="app" className="fixed inset-0 z-50 flex items-center justify-center bg-overlay/70 p-4 narrow:items-stretch narrow:p-0 narrow:pt-9">
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
        tabIndex={-1}
        onKeyDown={onKeyDown}
        className={`flex max-h-[85vh] w-full max-w-md flex-col border p-0 shadow-xl narrow:max-h-none narrow:max-w-none narrow:border-0 focus:outline-none ${
          isMismatch ? "border-danger bg-danger-subtle/95" : "border-border-strong bg-surface-raised"
        }`}
      >
        {isMismatch ? (
          <>
            {/* Gefahren-Warnstreifen — Design-Import, Abschnitt "HOST-KEY
             * GEÄNDERT"-Dialog: diagonale Rot-Schwarz-Schraffur als
             * unübersehbarer Alarm-Kopf. */}
            <div
              className="h-3 shrink-0"
              style={{
                background:
                  "repeating-linear-gradient(135deg, var(--color-danger) 0 10px, var(--color-danger-subtle) 10px 20px)",
              }}
            />
            <div className="flex min-h-0 flex-1 flex-col p-6">
              <div className="min-h-0 flex-1 overflow-y-auto">
              <div className="font-mono text-xs tracking-[0.18em] text-danger-fg uppercase">
                {t("hostKeyDialog.mismatchLabel")}
              </div>
              <h2 id={headingId} className="font-heading mt-1 mb-2 text-2xl leading-tight font-bold text-danger-fg-faint">
                {t("hostKeyDialog.mismatchHeading")}
              </h2>
              <p id={descriptionId} className="mb-4 text-sm text-danger-fg-softer/90">
                {t("hostKeyDialog.mismatchBodyBeforeHost")}
                <strong className="break-all">{event.host}:{event.port}</strong>
                {t("hostKeyDialog.mismatchBodyAfterHost")}
              </p>
              {event.keyType ? (
                <p className="mb-4 text-sm text-danger-fg-softer/90" data-testid="host-key-type">
                  {t("hostKeyDialog.keyType")}: <span className="font-mono">{event.keyType}</span>
                </p>
              ) : null}
              <div className="mb-4 grid grid-cols-2 gap-px narrow:grid-cols-1 border border-danger-border-strong/40 bg-danger-border-strong/25">
                <div className="flex flex-col gap-1 bg-danger-subtle p-3">
                  <span className="font-heading text-xs font-semibold tracking-wide text-danger-fg-soft uppercase">
                    {t("hostKeyDialog.known")}
                  </span>
                  <span className="font-mono text-xs break-all text-success-fg-soft">
                    {event.expectedFingerprint}
                  </span>
                </div>
                <div className="flex flex-col gap-1 bg-danger-subtle p-3">
                  <span className="font-heading text-xs font-semibold tracking-wide text-danger-fg-soft uppercase">
                    {t("hostKeyDialog.offeredNow")}
                  </span>
                  <span className="font-mono text-xs break-all text-danger-fg-soft">
                    {event.fingerprint}
                  </span>
                </div>
              </div>

              </div>

              <div className="mt-4 flex shrink-0 flex-wrap justify-end gap-2 border-t border-danger-border-strong/30 pt-4">
                <button
                  ref={rejectButtonRef}
                  type="button"
                  onClick={() => decide("reject")}
                  className="font-heading bg-danger-border-strong px-3 py-2 text-sm font-bold tracking-wide text-danger-fg-faintest hover:bg-danger focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent-fg focus-visible:ring-offset-1 focus-visible:ring-offset-danger-subtle"
                >
                  {t("hostKeyDialog.cancelConnection")}
                </button>
                <button
                  type="button"
                  onClick={() => decide("trust")}
                  className="font-heading border border-fg-bright/15 px-3 py-2 text-sm font-semibold tracking-wide text-danger-fg-softer hover:bg-fg-bright/6 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent-fg focus-visible:ring-offset-1 focus-visible:ring-offset-danger-subtle"
                >
                  {t("hostKeyDialog.trustAnyway")}
                </button>
              </div>
            </div>
          </>
        ) : (
          <div className="flex min-h-0 flex-1 flex-col p-6">
            <h2 id={headingId} className="font-heading mb-2 text-lg font-semibold text-fg">
              {t("hostKeyDialog.unknownHeading")}
            </h2>
            <div className="min-h-0 flex-1 overflow-y-auto">
            <p id={descriptionId} className="mb-4 text-sm text-fg-muted">
              {t("hostKeyDialog.unknownBodyBeforeHost")}
              <strong className="break-all">{event.host}:{event.port}</strong>
              {t("hostKeyDialog.unknownBodyAfterHost")}
            </p>
            {event.keyType ? (
              <p className="mb-2 text-sm text-fg-muted" data-testid="host-key-type">
                {t("hostKeyDialog.keyType")}: <span className="font-mono">{event.keyType}</span>
              </p>
            ) : null}
            <div className="mb-4 border border-border bg-surface-sunken p-2 font-mono text-xs break-all text-fg-muted">
              {event.fingerprint}
            </div>

            </div>

            <div className="flex shrink-0 flex-wrap justify-end gap-2">
              <Button
                ref={rejectButtonRef}
                onClick={() => decide("reject")}
              >
                {t("hostKeyDialog.reject")}
              </Button>
              <Button variant="primary" onClick={() => decide("trust")}>
                {t("hostKeyDialog.trust")}
              </Button>
            </div>
          </div>
        )}
      </div>
    </ModalBackdrop>
  );
}
