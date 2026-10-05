import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import {
  commandErrorCode,
  commandErrorMessage,
  getStartupState,
  quitApplication,
  startOverFromUnlockScreen,
  unlockWithMasterPassword,
  answerStartupPrompt,
} from "../api";
import { onStartupPrompt, onStartupUnlocked } from "../events";
import { applyStoredLanguage } from "../i18n";
import { translateErrorCode } from "../errorCodes";
import type { StartupPromptAnswer, StartupPromptRequest, StartupStateDto } from "../types";
import { StartupPromptDialog } from "./StartupPromptDialog";

interface StartupGateProps {
  /** Der Zustand, den `main.tsx` schon geholt hat, damit die Maske ohne
   * einen leeren Augenblick erscheint. `null`: Das Holen ist gescheitert —
   * dann fragt diese Komponente selbst noch einmal. */
  initialState: StartupStateDto | null;
  children: ReactNode;
}

/**
 * Spec 0101, A16: **Das Tor vor der Oberfläche.**
 *
 * Bis der Zustand steht, zeigt diese Komponente eine Startmaske und
 * **nicht** die App. Das ist keine Kosmetik: Vor der Entsperrung weist das
 * Tor im Backend jedes andere Kommando mit `APP_LOCKED` ab (gemessen M4),
 * und `App` ruft beim Mounten Daten-Kommandos auf. Würde die App trotzdem
 * gerendert, sähe der Nutzer eine Oberfläche voller Fehlermeldungen.
 *
 * **Welche Maske**, entscheidet das Backend mit einem Feld
 * (`StartupStateDto.screen`, s. dort) — nicht diese Komponente aus
 * mehreren Flaggen. Dasselbe gilt für „Neu anfangen": Der Knopf erscheint,
 * wenn `offersStartOver` gilt, und das Kommando prüft dieselbe Bedingung
 * noch einmal selbst (Klarstellung 9/11).
 *
 * **Die Reihenfolge beim Start ist wichtig:** Zuerst der Zuhörer für
 * `startup:prompt`, dann das erste Kommando. `emit` im Backend liefert
 * `Ok`, auch wenn niemand zuhört — ginge die erste Frage verloren, liefe
 * sie dort in die Zeitgrenze von fünf Minuten, und der Start bräche mit
 * „keine Antwort erhalten" ab.
 */
export function StartupGate({ initialState, children }: StartupGateProps) {
  const { t } = useTranslation();
  const [state, setState] = useState<StartupStateDto | null>(initialState);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [password, setPassword] = useState("");
  /** Die offene Frage. Getrennt vom Hinweis unten, weil ein Hinweis auf
   * keine Antwort wartet (Klarstellung 10e) und der Startablauf hinter ihm
   * weiterläuft — die nächste Frage kann also kommen, während er noch
   * steht. */
  const [question, setQuestion] = useState<StartupPromptRequest | null>(null);
  const [notice, setNotice] = useState<StartupPromptRequest | null>(null);
  /** Zählt die eingegangenen Fragen und dient als `key` des Dialogs:
   * Klarstellung 10e, „ein geöffnetes Passwortfeld ist leer" — mit einem
   * neuen `key` entsteht der Zustand des Dialogs je Frage neu. */
  const [questionSerial, setQuestionSerial] = useState(0);
  /** Teil 0 Frage 3: Der Startablauf wird genau **einmal** je Programmlauf
   * aus dem Fenster fortgesetzt. Ohne den Merker schickte ein zweiter
   * Durchlauf des Effekts (React im Entwicklungsmodus) einen zweiten
   * Aufruf, der im Backend auf der Entsperr-Sperre wartet, bis die erste
   * Frage beantwortet ist. */
  const continuedInTheWindow = useRef(false);
  /** Zählt die ausdrücklichen „Erneut versuchen" auf dieser Maske. Nötig
   * als Abhängigkeit des Effekts: Der Bildschirm bleibt derselbe, der
   * Effekt liefe also nicht erneut, wenn nur der Merker zurückgesetzt
   * würde. */
  const [continueAttempt, setContinueAttempt] = useState(0);
  /** Steht der Zuhörer für `startup:prompt`? Erst dann darf ein Kommando
   * laufen, dessen Antwort eine Frage im Fenster ist. */
  const [listening, setListening] = useState(false);

  /** Den Zustand neu holen. **Löscht die Meldung nicht** — sonst wischte
   * das Nachfragen nach einem Fehlversuch (s. [`run`]) genau die Meldung
   * weg, um die es geht: „Passwort falsch oder Datei beschädigt" (A17). Wer
   * die Meldung loswerden will, setzt sie selbst zurück. */
  const refresh = useCallback(async () => {
    try {
      setState(await getStartupState());
    } catch (err) {
      setError(messageFor(t, err));
    }
  }, [t]);

  const retry = useCallback(() => {
    setError(null);
    void refresh();
  }, [refresh]);

  useEffect(() => {
    let cancelled = false;
    // Die Versprechen, nicht die aufgelösten Funktionen: Läuft die
    // Aufräumfunktion, **bevor** `listen` zurückgekommen ist, muss sie
    // trotzdem abmelden können — sonst bleibt ein Zuhörer stehen, und beim
    // nächsten Mounten (React im Entwicklungsmodus) gibt es zwei, die
    // dieselbe Frage doppelt verarbeiten (Review-Fund Runde 1).
    const prompt = onStartupPrompt((request) => {
      if (cancelled) return;
      if (request.kind === "notice") {
        setNotice(request);
        return;
      }
      setQuestion(request);
      setQuestionSerial((serial) => serial + 1);
    });
    const unlocked = onStartupUnlocked(() => {
      if (cancelled) return;
      // A16: Erst jetzt sind `store` und `os` registriert — die
      // gespeicherte Sprachwahl aus Spec 0024 wird nachgeholt (ADR 0095
      // §8). Ein Fehler dabei ist kein Grund, die App nicht zu zeigen.
      void applyStoredLanguage().catch((err: unknown) => console.error(err));
      void refresh();
    });

    void (async () => {
      // **Erst der Zuhörer, dann alles andere.** `emit` im Backend liefert
      // `Ok`, auch wenn niemand zuhört: Ginge die erste Frage verloren,
      // liefe sie dort in die Zeitgrenze von fünf Minuten, und der Start
      // bräche mit „keine Antwort erhalten" ab. Der Merker unten ist die
      // Zusage, dass kein Kommando vorher läuft — vorher hing es allein an
      // der Reihenfolge zweier IPC-Nachrichten (Review-Fund Runde 1).
      await Promise.all([prompt, unlocked]);
      if (cancelled) return;
      setListening(true);
      if (!initialState) await refresh();
    })();

    return () => {
      cancelled = true;
      void prompt.then((unlisten) => unlisten()).catch(() => {});
      void unlocked.then((unlisten) => unlisten()).catch(() => {});
    };
  }, [initialState, refresh]);

  // Teil 0 Frage 3: Im Schlüsselbund-Modus **ohne** Zustand ist der Start in
  // D1 gelandet und soll im Fenster weitergehen — dort gibt es das
  // Eingabefeld, das die nativen Dialoge nicht haben. Der Aufruf setzt den
  // Ablauf fort; die Wahl selbst fällt im Dialog D1, der daraufhin kommt.
  //
  // **Erst, wenn der Zuhörer steht** (`listening`): Der Dialog D1 kommt als
  // Ereignis zurück, und ein verlorenes Ereignis wäre hier ein Hänger von
  // fünf Minuten.
  useEffect(() => {
    if (!listening) return;
    if (state?.screen !== "setUpMasterPassword") return;
    if (continuedInTheWindow.current) return;
    continuedInTheWindow.current = true;
    void run(() => unlockWithMasterPassword(""));
    // Abhängigkeiten sind allein der Bildschirm und der Zuhörer: `run`
    // entsteht bei jedem Render neu, und der Merker oben stellt ohnehin
    // sicher, dass es bei genau einem Aufruf bleibt.
  }, [listening, state?.screen, continueAttempt]);

  async function run(action: () => Promise<StartupStateDto>) {
    setBusy(true);
    setError(null);
    try {
      setState(await action());
    } catch (err) {
      // Nach einem gescheiterten Versuch gilt ein neuer Zustand: Klarstellung
      // 11 zählt im Backend mit, und ab dem dritten Fehlversuch gibt es
      // „Neu anfangen". Ohne dieses Nachfragen erschiene der Ausweg erst
      // beim nächsten Start. **Erst nachfragen, dann die Meldung setzen** —
      // andersherum stünde die Meldung schon, während `refresh` noch läuft,
      // und ein Fehler dort überschriebe sie.
      await refresh();
      setError(messageFor(t, err));
    } finally {
      setBusy(false);
      setPassword("");
    }
  }

  const sendAnswer = (
    answer: StartupPromptAnswer,
    pw?: string,
    repeated?: string,
    warningConfirmed?: boolean,
  ) => {
    setQuestion(null);
    void answerStartupPrompt(answer, pw, repeated, warningConfirmed).catch((err: unknown) => {
      setError(messageFor(t, err));
    });
  };

  if (state?.screen === "unlocked") {
    return (
      <>
        {children}
        {/* Ein Hinweis kann die Entsperrung überdauern: Er wartet auf keine
         * Antwort, der Start läuft weiter, und der Nutzer soll den neuen
         * Dateinamen aus A5 trotzdem lesen können (Klarstellung 10e). */}
        {notice && (
          <StartupPromptDialog
            request={notice}
            onAnswer={() => setNotice(null)}
            onDismiss={() => setNotice(null)}
          />
        )}
      </>
    );
  }

  return (
    <div className="flex h-screen flex-col items-center justify-center bg-slate-900 p-6 text-slate-100">
      <div className="w-full max-w-md rounded-lg border border-slate-700 bg-slate-800 p-6 shadow-xl">
        <h1 className="font-heading mb-1 text-xl font-semibold tracking-wide">Smart SSH</h1>

        {state === null ? (
          <>
            <p className="mb-4 mt-3 text-sm text-slate-300">{t("startup.stateUnknown")}</p>
            <Actions>
              <Secondary onClick={retry}>{t("startup.answers.retry")}</Secondary>
              <Secondary onClick={() => void quitApplication()}>
                {t("startup.answers.quit")}
              </Secondary>
            </Actions>
          </>
        ) : state.screen === "unlock" ? (
          <form
            onSubmit={(e) => {
              e.preventDefault();
              void run(() => unlockWithMasterPassword(password));
            }}
          >
            <p className="mb-4 mt-3 text-sm text-slate-300">{t("startup.unlockIntro")}</p>
            <label className="block text-sm text-slate-200">
              {t("startup.passwordLabel")}
              <input
                type="password"
                autoFocus
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                className="mt-1 w-full rounded border border-slate-600 bg-slate-900 px-3 py-2 text-sm text-slate-100"
              />
            </label>
            {state.failedUnlockAttempts > 0 && (
              <p className="mt-3 text-sm text-slate-400">
                {t("startup.failedAttempts", { attempts: state.failedUnlockAttempts })}
              </p>
            )}
            <Actions>
              <button
                type="submit"
                disabled={busy || password.length === 0}
                className="rounded bg-indigo-600 px-4 py-2 text-sm font-medium text-white hover:bg-indigo-500 disabled:cursor-not-allowed disabled:opacity-50"
              >
                {t("startup.unlockButton")}
              </button>
              <Secondary onClick={() => void quitApplication()}>
                {t("startup.answers.quit")}
              </Secondary>
            </Actions>
            {/* Klarstellung 11: erst nach dem dritten Fehlversuch — und nie
             * bei einer Datei, die sich bloß nicht lesen lässt. Beides
             * entscheidet das Backend. */}
            {state.offersStartOver && (
              <div className="mt-6 border-t border-slate-700 pt-4">
                <p className="mb-3 text-sm text-slate-400">{t("startup.wayOutHint")}</p>
                <StartOver busy={busy} onClick={() => void run(startOverFromUnlockScreen)} t={t} />
              </div>
            )}
          </form>
        ) : state.screen === "unusableWrapping" ? (
          <>
            <p className="mb-4 mt-3 text-sm text-slate-300">{t("startup.unusableWrapping")}</p>
            <Actions>
              <StartOver busy={busy} onClick={() => void run(startOverFromUnlockScreen)} t={t} />
              <Secondary onClick={() => void quitApplication()}>
                {t("startup.answers.quit")}
              </Secondary>
            </Actions>
          </>
        ) : state.screen === "unreachableWrapping" ? (
          <>
            {/* A3 *nicht erreichbar* (D1), Klarstellung 10a: Über den Inhalt
             * der Datei ist nichts gesagt — hier gibt es **kein** „Neu
             * anfangen", auch nach beliebig vielen Versuchen nicht. */}
            <p className="mb-4 mt-3 text-sm text-slate-300">{t("startup.unreachableWrapping")}</p>
            <Actions>
              <button
                type="button"
                onClick={retry}
                className="rounded bg-indigo-600 px-4 py-2 text-sm font-medium text-white hover:bg-indigo-500"
              >
                {t("startup.answers.retry")}
              </button>
              <Secondary onClick={() => void quitApplication()}>
                {t("startup.answers.quit")}
              </Secondary>
            </Actions>
          </>
        ) : (
          <>
            <p className="mb-4 mt-3 text-sm text-slate-300">{t("startup.continuingInTheWindow")}</p>
            {/* **Diese Maske braucht eigene Knöpfe** (Review-Fund Runde 1,
              * A16/Spec 0059): Wählt der Nutzer im Dialog D1 „Beenden",
              * kommt der Startablauf als Fehler zurück („Der Start wurde
              * abgebrochen") — und der Bildschirm bleibt dieser. Ohne
              * Knöpfe hätte „Beenden" dann nicht beendet, und es gäbe
              * keinen Weg mehr aus der App außer dem Fenstersystem. */}
            <Actions>
              <button
                type="button"
                disabled={busy}
                onClick={() => {
                  // Noch einmal in den Startablauf: Der Merker wird
                  // zurückgesetzt, damit der Dialog erneut erscheint.
                  continuedInTheWindow.current = false;
                  setContinueAttempt((attempt) => attempt + 1);
                  setError(null);
                }}
                className="rounded bg-indigo-600 px-4 py-2 text-sm font-medium text-white hover:bg-indigo-500 disabled:cursor-not-allowed disabled:opacity-50"
              >
                {t("startup.answers.retry")}
              </button>
              <Secondary onClick={() => void quitApplication()}>
                {t("startup.answers.quit")}
              </Secondary>
            </Actions>
          </>
        )}

        {error && (
          <p
            role="alert"
            className="mt-4 rounded border border-red-800 bg-red-950 px-3 py-2 text-sm text-red-200"
          >
            {error}
          </p>
        )}
      </div>

      {question && (
        <StartupPromptDialog
          key={questionSerial}
          request={question}
          onAnswer={sendAnswer}
          onDismiss={() => setQuestion(null)}
        />
      )}
      {notice && (
        <StartupPromptDialog
          request={notice}
          onAnswer={() => setNotice(null)}
          onDismiss={() => setNotice(null)}
        />
      )}
    </div>
  );
}

function Actions({ children }: { children: ReactNode }) {
  return <div className="mt-5 flex flex-wrap gap-2">{children}</div>;
}

function Secondary({ onClick, children }: { onClick: () => void; children: ReactNode }) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="rounded bg-slate-700 px-4 py-2 text-sm font-medium text-white hover:bg-slate-600"
    >
      {children}
    </button>
  );
}

/** A5: „Neu anfangen" ist rot und führt in die zweite Bestätigung — die
 * kommt als eigene Frage aus dem Backend (`confirmStartOver`), nicht aus
 * dieser Komponente. */
function StartOver({
  busy,
  onClick,
  t,
}: {
  busy: boolean;
  onClick: () => void;
  t: (key: string) => string;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={busy}
      className="rounded bg-red-700 px-4 py-2 text-sm font-medium text-white hover:bg-red-600 disabled:cursor-not-allowed disabled:opacity-50"
    >
      {t("startup.answers.startOver")}
    </button>
  );
}

/** Der Fehlertext: übersetzter Code, sonst der Text des Backends (Spec
 * 0024, Abschnitt 5) — nie eine leere Anzeige. */
function messageFor(
  t: (key: string, options?: Record<string, unknown>) => string,
  err: unknown,
): string {
  return translateErrorCode(t, commandErrorCode(err), commandErrorMessage(err));
}
