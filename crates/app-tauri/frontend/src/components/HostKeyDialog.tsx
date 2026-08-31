import type { HostKeyInfo, HostKeyUserDecision } from "../types";

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
 */
export function HostKeyDialog({ event, onDecision }: HostKeyDialogProps) {
  const isMismatch = event.kind === "mismatch";

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4">
      <div
        className={`w-full max-w-md rounded-lg border-2 p-6 shadow-xl ${
          isMismatch ? "border-red-500 bg-red-950" : "border-slate-600 bg-slate-800"
        }`}
      >
        {isMismatch ? (
          <>
            <h2 className="mb-2 text-lg font-bold text-red-300">
              ⚠ Host-Key hat sich geändert!
            </h2>
            <p className="mb-4 text-sm text-red-200">
              Der Server <strong>{event.host}:{event.port}</strong> präsentiert einen anderen
              Schlüssel als beim letzten Mal gespeichert. Das kann auf einen
              Man-in-the-Middle-Angriff hindeuten. Nur fortfahren, wenn du sicher bist, dass sich
              der Server-Schlüssel legitim geändert hat (z. B. Neuinstallation des Servers).
            </p>
            <div className="mb-4 space-y-1 rounded bg-black/30 p-2 font-mono text-xs text-red-200">
              <div>Erwartet: {event.expectedFingerprint}</div>
              <div>Jetzt: {event.fingerprint}</div>
            </div>
          </>
        ) : (
          <>
            <h2 className="mb-2 text-lg font-semibold text-slate-100">Unbekannter Host</h2>
            <p className="mb-4 text-sm text-slate-300">
              Du verbindest dich zum ersten Mal mit <strong>{event.host}:{event.port}</strong>.
              Bitte den Fingerprint prüfen, bevor du vertraust.
            </p>
            <div className="mb-4 rounded bg-black/20 p-2 font-mono text-xs text-slate-300">
              {event.fingerprint}
            </div>
          </>
        )}

        <div className="flex gap-2">
          <button
            type="button"
            onClick={() => onDecision({ decision: "reject" })}
            className="flex-1 rounded bg-slate-700 px-3 py-2 text-sm text-slate-100 hover:bg-slate-600"
          >
            Ablehnen
          </button>
          <button
            type="button"
            onClick={() => onDecision({ decision: "trust" })}
            className={`flex-1 rounded px-3 py-2 text-sm font-medium text-white ${
              isMismatch ? "bg-red-700 hover:bg-red-600" : "bg-indigo-600 hover:bg-indigo-500"
            }`}
          >
            {isMismatch ? "Trotzdem vertrauen" : "Vertrauen"}
          </button>
        </div>
      </div>
    </div>
  );
}
