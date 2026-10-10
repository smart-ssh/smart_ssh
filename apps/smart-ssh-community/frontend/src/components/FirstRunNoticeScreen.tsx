import { Component as ReactComponent, useRef, useState, type ErrorInfo, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import {
  listFirstRunNoticeExtensions,
  type FirstRunNoticeContinueHandler,
  type FirstRunNoticeExtensionContext,
} from "../extensions/registry";
import { ModalBackdrop } from "./ModalBackdrop";

interface FirstRunNoticeScreenProps {
  /** Aufgerufen bei "Weiter". Der Aufrufer speichert die Pflicht-
   * Bestätigung und ruft danach — und nur, wenn das Speichern geklappt
   * hat — `afterStored` auf; erst das löst die `onContinue`-Handler der
   * Erweiterungen aus (Spec 0031, Abschnitt 6). */
  onAcknowledge: (afterStored: () => void) => void | Promise<void>;
}

/** Ruft jeden Handler auf. Ein Wurf oder eine abgelehnte Promise wird
 * protokolliert und hält weder die übrigen Handler noch das Schließen des
 * Hinweises auf (Spec 0031, Abschnitt 6). Bewusst nicht abgewartet: Die
 * Pflicht-Bestätigung ist zu diesem Zeitpunkt schon gespeichert, ein
 * langsamer Handler soll den Verbindungsaufbau nicht verzögern. */
function runContinueHandlers(handlers: [string, FirstRunNoticeContinueHandler][]): void {
  for (const [id, handler] of handlers) {
    const report = (err: unknown) =>
      console.error(`First-run notice extension "${id}" failed on continue:`, err);
    try {
      Promise.resolve(handler()).catch(report);
    } catch (err) {
      report(err);
    }
  }
}

interface ExtensionBoundaryProps {
  id: string;
  onError: (id: string, error: unknown, info: ErrorInfo) => void;
  children: ReactNode;
}

/** Fängt Render-, Lifecycle- und Effect-Fehler genau einer Erweiterung ab
 * (Spec 0031, Abschnitt 6, issue #159). Danach rendert sie nichts mehr für
 * diese Erweiterung — auch nicht den umgebenden Bereich —, damit
 * Hinweistext, Pflicht-Checkbox und "Weiter" bedienbar bleiben. Ohne sie
 * würde ein Wurf den ganzen Baum aushängen und damit die einzige
 * Möglichkeit, den Hinweis zu bestätigen. */
class ExtensionBoundary extends ReactComponent<ExtensionBoundaryProps, { failed: boolean }> {
  state = { failed: false };

  static getDerivedStateFromError(): { failed: boolean } {
    return { failed: true };
  }

  componentDidCatch(error: unknown, info: ErrorInfo): void {
    this.props.onError(this.props.id, error, info);
  }

  render(): ReactNode {
    return this.state.failed ? null : this.props.children;
  }
}

/**
 * Spec 0031: Zustimmungs-Screen vor der ersten Server-Verbindung
 * (Verantwortung für bestätigte Kommandos + Hinweis auf fehlende
 * zusätzliche Datenbank-Verschlüsselung, Abschnitt 3). "Weiter" bleibt
 * deaktiviert, bis die Checkbox aktiv ist (Abschnitt 4) — reines
 * Wegklicken ohne bewusste Bestätigung ist nicht möglich, deshalb auch
 * kein Abbrechen-/Schließen-Button.
 */
export function FirstRunNoticeScreen({ onAcknowledge }: FirstRunNoticeScreenProps) {
  const { t } = useTranslation();
  const [checked, setChecked] = useState(false);

  // Spec 0031, Abschnitt 6: Erweiterungen werden beim Öffnen einmal
  // gelesen. Jede bekommt nur `onContinue` — keinen Zugriff auf
  // `checked`, den Text oder den "Weiter"-Button.
  const continueHandlers = useRef(new Map<string, FirstRunNoticeContinueHandler>());
  const handlersRan = useRef(false);
  // Erweiterungen, deren Darstellung gescheitert ist: ihr Handler wird
  // verworfen und darf auch später nicht mehr registriert werden.
  const failedExtensions = useRef(new Set<string>());
  const [extensions] = useState(() =>
    listFirstRunNoticeExtensions().map(({ id, Component }) => {
      const context: FirstRunNoticeExtensionContext = {
        onContinue: (handler) => {
          if (failedExtensions.current.has(id)) return;
          continueHandlers.current.set(id, handler);
        },
      };
      return { id, Component, context };
    }),
  );

  const onExtensionError = (id: string, error: unknown) => {
    failedExtensions.current.add(id);
    continueHandlers.current.delete(id);
    console.error(`First-run notice extension "${id}" failed to render:`, error);
  };

  const afterStored = () => {
    if (handlersRan.current) return;
    handlersRan.current = true;
    runContinueHandlers(Array.from(continueHandlers.current.entries()));
  };

  // Unabhängiger Review-Pass: s. identischer Kommentar in
  // `HostKeyDialog.tsx` — dieser Screen kann ebenfalls von `ServerList`
  // ausgelöst werden, während `MainScreen` per `display:none` ausgeblendet
  // ist (aktiver Session-Tab), und wäre ohne Portal unsichtbar.
  return createPortal(
    <ModalBackdrop className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4">
      <div className="w-full max-w-md rounded-lg bg-slate-800 p-6 shadow-xl">
        <h2 className="font-heading mb-4 text-lg font-semibold tracking-wide text-slate-100">
          {t("firstRunNotice.title")}
        </h2>
        <p className="mb-4 text-sm text-slate-300">{t("firstRunNotice.responsibilityText")}</p>
        <p className="mb-4 text-sm text-slate-300">{t("firstRunNotice.encryptionText")}</p>
        <label className="mb-4 flex items-center gap-2 text-sm text-slate-200">
          <input
            type="checkbox"
            checked={checked}
            onChange={(e) => setChecked(e.target.checked)}
          />
          {t("firstRunNotice.checkboxLabel")}
        </label>
        {extensions.map(({ id, Component, context }) => (
          <ExtensionBoundary key={id} id={id} onError={onExtensionError}>
            <section
              data-testid={`first-run-notice-extension-${id}`}
              className="mb-4 border-t border-slate-700 pt-4 text-sm text-slate-300"
            >
              <Component {...context} />
            </section>
          </ExtensionBoundary>
        ))}
        <div className="flex justify-end">
          <button
            type="button"
            onClick={() => void onAcknowledge(afterStored)}
            disabled={!checked}
            className="rounded bg-indigo-600 px-4 py-2 text-sm font-medium text-slate-950 hover:bg-indigo-500 disabled:cursor-not-allowed disabled:opacity-50"
          >
            {t("firstRunNotice.continueButton")}
          </button>
        </div>
      </div>
    </ModalBackdrop>,
    document.body,
  );
}
