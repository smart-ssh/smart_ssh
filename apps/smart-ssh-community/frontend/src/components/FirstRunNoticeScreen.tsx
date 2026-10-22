import { Component as ReactComponent, useRef, useState, type ErrorInfo, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import {
  listFirstRunNoticeExtensions,
  type FirstRunNoticeContinueHandler,
  type FirstRunNoticeExtensionContext,
} from "../extensions/registry";
import { Button, DialogFrame } from "./ui";

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
  const checkboxRef = useRef<HTMLInputElement>(null);

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

  // `DialogFrame` rendert per Portal nach `document.body` (`layer="app"`):
  // Dieser Screen kann von `ServerList` ausgelöst werden, während
  // `MainScreen` per `display:none` ausgeblendet ist (aktiver Session-Tab),
  // und wäre sonst unsichtbar. Escape bewirkt hier nichts: Der Hinweis ist
  // eine Pflicht-Bestätigung (Spec 0031, Abschnitt 4), kein Abbrechen.
  return (
    <DialogFrame
      layer="app"
      title={t("firstRunNotice.title")}
      onClose={() => {}}
      initialFocusRef={checkboxRef}
      actions={
        <Button
          variant="primary"
          onClick={() => void onAcknowledge(afterStored)}
          disabled={!checked}
        >
          {t("firstRunNotice.continueButton")}
        </Button>
      }
    >
      <p className="mb-3">{t("firstRunNotice.responsibilityText")}</p>
      <p className="mb-3">{t("firstRunNotice.encryptionText")}</p>
      <label className="flex items-start gap-2 text-slate-200">
        <input
          ref={checkboxRef}
          type="checkbox"
          checked={checked}
          onChange={(e) => setChecked(e.target.checked)}
          className="mt-1 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-indigo-400"
        />
        {t("firstRunNotice.checkboxLabel")}
      </label>
      {extensions.map(({ id, Component, context }) => (
        <ExtensionBoundary key={id} id={id} onError={onExtensionError}>
          <section
            data-testid={`first-run-notice-extension-${id}`}
            className="mt-3 min-w-0 max-w-full break-words border-t border-slate-700 pt-3 text-slate-300"
          >
            <Component {...context} />
          </section>
        </ExtensionBoundary>
      ))}
    </DialogFrame>
  );
}
