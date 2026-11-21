import { useEffect, useId, useRef } from "react";
import { useTranslation } from "react-i18next";
import type { HostKeyInfo, HostKeyUserDecision } from "../api/types";
import { Button, DialogFrame, Icon } from "./ui";

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
  const rejectLabel = isMismatch ? t("hostKeyDialog.cancelConnection") : t("hostKeyDialog.reject");
  const trustLabel = isMismatch ? t("hostKeyDialog.trustAnyway") : t("hostKeyDialog.trust");
  const rowClass = "grid grid-cols-[6rem_1fr] gap-3 border-t border-border px-3 py-2 first:border-t-0 narrow:grid-cols-1 narrow:gap-1";
  const labelClass = "label-condensed text-xs text-fg-subtle";
  return (
    <DialogFrame
      layer="app"
      bare
      // A1: `alertdialog` for a changed key (higher urgency for screen readers), `dialog` for an unknown one.
      role={dialogRole}
      labelledBy={headingId}
      describedBy={descriptionId}
      // A4: Escape rejects, never trusts. There is no backdrop click handler: a stray click decides nothing.
      onClose={() => decide("reject")}
      initialFocusRef={rejectButtonRef}
      resetKey={event}
      tone={isMismatch ? "danger" : undefined}
      ticks={isMismatch ? "danger" : undefined}
      panelClassName={`relative flex max-h-[85vh] w-full max-w-lg flex-col border ${isMismatch ? "" : "border-border-strong"} bg-surface-raised p-0 shadow-offset-lg narrow:max-h-none narrow:max-w-none narrow:border-0 narrow:shadow-none`}
    >
      {isMismatch ? (
        <div
          className="flex shrink-0 items-center gap-2 border-b border-danger-border bg-danger-subtle px-5 py-3 text-danger-fg narrow:px-4"
          data-testid="host-key-header"
        >
          <Icon name="warn" />
          <div className="label-condensed text-xs">{t("hostKeyDialog.mismatchLabel")}</div>
        </div>
      ) : null}
      <div className="min-h-0 flex-1 overflow-y-auto px-5 py-4 narrow:px-4">
        <h2
          id={headingId}
          className="text-title mb-2 break-words text-fg-strong"
        >
          {isMismatch ? t("hostKeyDialog.mismatchHeading") : t("hostKeyDialog.unknownHeading")}
        </h2>
        <p id={descriptionId} className="mb-4 text-sm text-fg-muted">
          {t(isMismatch ? "hostKeyDialog.mismatchBodyBeforeHost" : "hostKeyDialog.unknownBodyBeforeHost")}
          <strong className="font-mono break-all text-fg" data-testid="host-key-address">
            {event.host}:{event.port}
          </strong>
          {t(isMismatch ? "hostKeyDialog.mismatchBodyAfterHost" : "hostKeyDialog.unknownBodyAfterHost")}
        </p>
        <dl className="border border-border bg-surface font-mono text-xs">
          {isMismatch ? (
            <div className={rowClass}>
              <dt className={labelClass}>{t("hostKeyDialog.known")}</dt>
              <dd className="break-all text-fg select-text">{event.expectedFingerprint}</dd>
            </div>
          ) : null}
          <div className={`${rowClass} ${isMismatch ? "bg-danger-subtle" : ""}`}>
            <dt className={`${labelClass} ${isMismatch ? "text-danger-fg" : ""}`}>
              {t("hostKeyDialog.offeredNow")}
            </dt>
            <dd className={`break-all select-text ${isMismatch ? "text-danger-fg" : "text-fg"}`}>{event.fingerprint}</dd>
          </div>
          {event.keyType ? (
            <div className={rowClass} data-testid="host-key-type">
              <dt className={labelClass}>{t("hostKeyDialog.keyType")}</dt>
              <dd className="text-fg select-text">{event.keyType}</dd>
            </div>
          ) : null}
        </dl>
      </div>
      <div className="flex shrink-0 flex-wrap justify-end gap-2 border-t border-border bg-surface px-5 py-3 narrow:px-4">
        <Button
          ref={rejectButtonRef}
          variant={isMismatch ? "primary" : "secondary"}
          onClick={() => decide("reject")}
        >
          {rejectLabel}
        </Button>
        <Button
          variant={isMismatch ? "ghost" : "primary"}
          className={isMismatch ? "text-danger-fg" : ""}
          onClick={() => decide("trust")}
        >
          {trustLabel}
        </Button>
      </div>
    </DialogFrame>
  );
}
