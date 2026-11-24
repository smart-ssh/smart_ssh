import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import {
  getStartupState,
  quitApplication,
  startOverFromUnlockScreen,
  unlockWithMasterPassword,
  answerStartupPrompt,
} from "../api/api";
import { onStartupPrompt, onStartupUnlocked } from "../api/events";
import { applyStoredLanguage } from "../i18n";
import { applyStoredTheme } from "../lib/theme";
import { commandErrorText } from "../api/errorCodes";
import type { StartupPromptAnswer, StartupPromptRequest, StartupStateDto } from "../api/types";
import { Button, CornerTicks, Field, FIELD_CONTROL_CLASS, Icon, Logo } from "./ui";
import { StartupPromptDialog } from "./StartupPromptDialog";
import { StartupTitleBarDragStrip } from "./StartupTitleBarDragStrip";
import { TECHNICAL_INPUT_PROPS } from "../lib/technicalInputProps";
import type { TranslateFn } from "../i18nTypes";

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
  /** §2 Frage 3: Der Startablauf wird genau **einmal** je Programmlauf
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
  /** Zählt die Versuche, die Zuhörer anzumelden. „Erneut versuchen“ erhöht
   * ihn, wenn die Anmeldung gescheitert ist (`listening` falsch) — als
   * Abhängigkeit des Zuhörer-Effekts meldet dieser sich dann neu an, nachdem
   * die Aufräumfunktion die halb angemeldeten Zuhörer abgemeldet hat. */
  const [listenAttempt, setListenAttempt] = useState(0);

  /** Den Zustand neu holen. **Löscht die Meldung nicht** — sonst wischte
   * das Nachfragen nach einem Fehlversuch (s. [`run`]) genau die Meldung
   * weg, um die es geht: „Passwort falsch oder Datei beschädigt" (A17). Wer
   * die Meldung loswerden will, setzt sie selbst zurück. */
  const refresh = useCallback(async () => {
    try {
      setState(await getStartupState());
    } catch (err) {
      setError(commandErrorText(t, err));
    }
  }, [t]);

  const retry = useCallback(() => {
    setError(null);
    if (!listening) {
      // Die Zuhörer stehen nicht: Erst sie neu anmelden. Der Effekt holt
      // danach bei Bedarf auch den Zustand (Reihenfolge „Zuhörer zuerst,
      // dann Kommandos“ bleibt).
      setListenAttempt((attempt) => attempt + 1);
      return;
    }
    void refresh();
  }, [refresh, listening]);

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
      void applyStoredTheme().catch((err: unknown) => console.error(err));
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
    })().catch((err: unknown) => {
      // **Kein stiller Endzustand** (spec-reviewer Runde 2, Spec 0059/§6).
      // Scheitert das Anmelden der Zuhörer, bleibt `listening` falsch — und
      // damit kehrt der Effekt unten immer früh zurück, der Bildschirm
      // zeigt endlos „wird fortgesetzt" und der Knopf „Erneut versuchen"
      // ist wirkungslos. Ohne dieses `catch` stünde davon nur eine
      // unbehandelte Ablehnung in der Konsole, die niemand sieht.
      if (cancelled) return;
      setError(commandErrorText(t, err));
    });

    return () => {
      cancelled = true;
      void prompt.then((unlisten) => unlisten()).catch(() => {});
      void unlocked.then((unlisten) => unlisten()).catch(() => {});
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- a language change must not re-register the listeners
  }, [initialState, refresh, listenAttempt]);

  // §2 Frage 3: Im Schlüsselbund-Modus **ohne** Zustand ist der Start in
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
    // eslint-disable-next-line react-hooks/exhaustive-deps -- run is recreated each render; a ref guards the single call
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
      setError(commandErrorText(t, err));
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
      setError(commandErrorText(t, err));
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
    // Issue #163 / Spec 0014, Abschnitt 5: Oben bleibt ein Rand in
    // Titelleistenhöhe frei (`pt-9` zusätzlich zum `p-6`, also
    // `pt-15`), über dem die Drag-Leiste liegt — die Karte rückt nie
    // darunter.
    <div
      data-testid="startup-screen"
      className="flex h-screen flex-col items-center overflow-y-auto bg-surface bg-[linear-gradient(to_right,var(--color-grid-line)_1px,transparent_1px),linear-gradient(to_bottom,var(--color-grid-line)_1px,transparent_1px)] bg-[size:32px_32px] px-6 pb-6 pt-15 text-fg narrow:bg-[size:26px_26px] narrow:px-4"
    >
      <StartupTitleBarDragStrip />
      <div
        data-testid="startup-card"
        className={`relative my-auto flex w-full max-w-[460px] flex-col border border-border-strong bg-surface-raised p-8 narrow:p-5 ${
          // Only the unlock card fills the narrow screen (its way-out area is
          // pinned to the bottom); the other states keep a content-height card.
          state?.screen === "unlock" ? "narrow:my-0 narrow:flex-1" : ""
        }`}
      >
        <CornerTicks tone="accent" corners="all" />
        <Logo variant="lockup" wordmark="full" size={40} className="mb-6" />

        {state?.screen !== "unlock" && <h1 className="sr-only">Smart SSH</h1>}

        {state === null ? (
          <>
            <p className="mb-4 text-sm text-fg-muted">{t("startup.stateUnknown")}</p>
            <Actions>
              <Secondary onClick={retry}>{t("startup.answers.retry")}</Secondary>
              <Secondary onClick={() => void quitApplication()}>
                {t("startup.answers.quit")}
              </Secondary>
            </Actions>
          </>
        ) : state.screen === "unlock" ? (
          <form
            className="flex flex-1 flex-col"
            onSubmit={(e) => {
              e.preventDefault();
              void run(() => unlockWithMasterPassword(password));
            }}
          >
            <h1 className="text-display mb-3 flex items-center gap-3 text-fg-strong">
              <Icon name="lock" size={24} className="text-accent" />
              {t("startup.unlockButton")}
            </h1>
            <p className="mb-5 text-sm text-fg-muted">{t("startup.unlockIntro")}</p>
            <Field
              label={t("startup.passwordLabel")}
              help={
                state.failedUnlockAttempts > 0 ? (
                  <span className="flex items-center gap-1.5 text-warning-fg">
                    <Icon name="warn" size={14} className="shrink-0" />
                    {t("startup.failedAttempts", { attempts: state.failedUnlockAttempts })}
                  </span>
                ) : undefined
              }
            >
              {(control) => (
                <input
                  {...control}
                  {...TECHNICAL_INPUT_PROPS}
                  type="password"
                  // eslint-disable-next-line jsx-a11y/no-autofocus -- gewollter Anfangsfokus des Dialogs (Issue #112): das Passwortfeld ist die einzige Eingabe.
                  autoFocus
                  value={password}
                  onChange={(e) => setPassword(e.target.value)}
                  className={`${FIELD_CONTROL_CLASS} h-[42px] narrow:h-12`}
                />
              )}
            </Field>
            <Button
              type="submit"
              variant="primary"
              className="mt-5 w-full"
              disabled={busy || password.length === 0}
            >
              {t("startup.unlockButton")}
            </Button>
            {/* Way-out area: Quit is always there (Spec 0101 A16). Klarstellung
             * 11: Start over only after the third failed attempt — and never
             * for a file that merely cannot be read. The backend decides. */}
            <div
              data-testid="unlock-way-out"
              className="mt-6 flex flex-col gap-3 border-t border-border pt-4 narrow:mt-auto narrow:[&_button]:w-full"
            >
              {state.offersStartOver && (
                <p className="text-xs text-fg-subtle">{t("startup.wayOutHint")}</p>
              )}
              <div className="flex flex-wrap gap-2 narrow:flex-col">
                {state.offersStartOver && (
                  <StartOver busy={busy} onClick={() => void run(startOverFromUnlockScreen)} t={t} />
                )}
                <Secondary onClick={() => void quitApplication()}>
                  {t("startup.answers.quit")}
                </Secondary>
              </div>
            </div>
          </form>
        ) : state.screen === "unusableWrapping" ? (
          <>
            <p className="mb-4 text-sm text-fg-muted">{t("startup.unusableWrapping")}</p>
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
            <p className="mb-4 text-sm text-fg-muted">{t("startup.unreachableWrapping")}</p>
            <Actions>
              <Button variant="primary" onClick={retry}>
                {t("startup.answers.retry")}
              </Button>
              <Secondary onClick={() => void quitApplication()}>
                {t("startup.answers.quit")}
              </Secondary>
            </Actions>
          </>
        ) : (
          <>
            <p className="mb-4 text-sm text-fg-muted">{t("startup.continuingInTheWindow")}</p>
            {/* **Diese Maske braucht eigene Knöpfe** (Review-Fund Runde 1,
              * A16/Spec 0059): Wählt der Nutzer im Dialog D1 „Beenden",
              * kommt der Startablauf als Fehler zurück („Der Start wurde
              * abgebrochen") — und der Bildschirm bleibt dieser. Ohne
              * Knöpfe hätte „Beenden" dann nicht beendet, und es gäbe
              * keinen Weg mehr aus der App außer dem Fenstersystem. */}
            <Actions>
              <Button
                variant="primary"
                disabled={busy}
                onClick={() => {
                  // Standen die Zuhörer nie (Anmeldung gescheitert), kommt
                  // zuerst die erneute Anmeldung; danach setzt der Effekt
                  // über `listening` den Startablauf fort.
                  if (!listening) {
                    setListenAttempt((attempt) => attempt + 1);
                    setError(null);
                    return;
                  }
                  // Noch einmal in den Startablauf: Der Merker wird
                  // zurückgesetzt, damit der Dialog erneut erscheint.
                  continuedInTheWindow.current = false;
                  setContinueAttempt((attempt) => attempt + 1);
                  setError(null);
                }}
              >
                {t("startup.answers.retry")}
              </Button>
              <Secondary onClick={() => void quitApplication()}>
                {t("startup.answers.quit")}
              </Secondary>
            </Actions>
          </>
        )}

        {error && (
          <p
            role="alert"
            className="mt-4 rounded border border-danger-border bg-danger-subtle px-3 py-2 text-sm text-danger-fg-softer"
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
  return <div className="mt-5 flex flex-wrap justify-end gap-2">{children}</div>;
}

function Secondary({ onClick, children }: { onClick: () => void; children: ReactNode }) {
  return <Button onClick={onClick}>{children}</Button>;
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
  t: TranslateFn;
}) {
  return (
    <Button variant="danger" onClick={onClick} disabled={busy}>
      {t("startup.answers.startOver")}
    </Button>
  );
}

