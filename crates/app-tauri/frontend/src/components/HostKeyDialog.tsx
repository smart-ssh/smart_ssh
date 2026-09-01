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
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4">
      <div
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
                Sicherheitswarnung · Host-Schlüssel geändert
              </div>
              <h2 className="font-heading mt-1 mb-2 text-2xl leading-tight font-bold text-red-100">
                Der Schlüssel dieses Servers ist nicht mehr derselbe.
              </h2>
              <p className="mb-4 text-sm text-red-200/90">
                Der Server <strong>{event.host}:{event.port}</strong> präsentiert einen anderen
                Schlüssel als beim letzten Mal gespeichert. Das kann auf einen
                Man-in-the-Middle-Angriff hindeuten. Nur fortfahren, wenn du sicher bist, dass sich
                der Server-Schlüssel legitim geändert hat (z. B. Neuinstallation des Servers).
              </p>
              <div className="mb-4 grid grid-cols-2 gap-px border border-red-700/40 bg-red-700/25">
                <div className="flex flex-col gap-1 bg-red-950 p-3">
                  <span className="font-heading text-[11px] font-semibold tracking-wide text-red-400/80 uppercase">
                    Bekannt
                  </span>
                  <span className="font-mono text-xs break-all text-emerald-300">
                    {event.expectedFingerprint}
                  </span>
                </div>
                <div className="flex flex-col gap-1 bg-red-950 p-3">
                  <span className="font-heading text-[11px] font-semibold tracking-wide text-red-300 uppercase">
                    Jetzt angeboten
                  </span>
                  <span className="font-mono text-xs break-all text-red-300">
                    {event.fingerprint}
                  </span>
                </div>
              </div>

              <div className="flex gap-2 border-t border-red-700/30 pt-4">
                <button
                  type="button"
                  onClick={() => onDecision({ decision: "reject" })}
                  className="font-heading flex-1 bg-red-600 px-3 py-2 text-sm font-bold tracking-wide text-red-50 hover:bg-red-500"
                >
                  Verbindung abbrechen
                </button>
                <button
                  type="button"
                  onClick={() => onDecision({ decision: "trust" })}
                  className="font-heading flex-1 border border-white/15 px-3 py-2 text-sm font-semibold tracking-wide text-red-200 hover:bg-white/6"
                >
                  Trotzdem vertrauen
                </button>
              </div>
            </div>
          </>
        ) : (
          <div className="p-6">
            <h2 className="font-heading mb-2 text-lg font-semibold text-slate-100">
              Unbekannter Host
            </h2>
            <p className="mb-4 text-sm text-slate-300">
              Du verbindest dich zum ersten Mal mit <strong>{event.host}:{event.port}</strong>.
              Bitte den Fingerprint prüfen, bevor du vertraust.
            </p>
            <div className="mb-4 border border-slate-700 bg-slate-950 p-2 font-mono text-xs text-slate-300">
              {event.fingerprint}
            </div>

            <div className="flex gap-2">
              <button
                type="button"
                onClick={() => onDecision({ decision: "reject" })}
                className="font-heading flex-1 border border-slate-600 px-3 py-2 text-sm font-semibold tracking-wide text-slate-100 hover:bg-slate-700"
              >
                Ablehnen
              </button>
              <button
                type="button"
                onClick={() => onDecision({ decision: "trust" })}
                className="font-heading flex-1 bg-indigo-600 px-3 py-2 text-sm font-semibold tracking-wide text-slate-950 hover:bg-indigo-500"
              >
                Vertrauen
              </button>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
