import {
  type KeyboardEvent,
  type RefObject,
  useEffect,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
} from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { commandErrorMessage, listPromptHistory } from "../../api";
import { isPointerInside } from "../../dropZone";
import {
  type ComposerAction,
  type ComposerAttachment,
  type ComposerContext,
  listComposerActions,
  listComposerDropHandlers,
  subscribeComposer,
} from "../../extensions/registry";
import { initialHistoryNavState, navigateHistory, type HistoryNavState } from "../../promptHistoryNav";

/**
 * Chat input state: draft, attachments, composer extension slot, native drop
 * routing and prompt-history navigation. Sending stays in `ChatPanel`; it
 * reads the draft and attachments from here and clears them after sending.
 */
export function useChatComposer(
  sessionId: string,
  serverId: string,
  inputRef: RefObject<HTMLTextAreaElement | null>,
  chatAreaRef: RefObject<HTMLElement | null>,
) {
  const [draft, setDraft] = useState("");
  // Issue #471: composer extension point, pending attachments, error at the input.
  const composerActions = useSyncExternalStore(subscribeComposer, listComposerActions);
  const dropHandlers = useSyncExternalStore(subscribeComposer, listComposerDropHandlers);
  const [attachments, setAttachments] = useState<ComposerAttachment[]>([]);
  const [composerError, setComposerError] = useState<string | null>(null);
  const composerCtx = useMemo<ComposerContext>(
    () => ({
      sessionId,
      addAttachment: (a) =>
        setAttachments((prev) => (prev.some((p) => p.id === a.id) ? prev : [...prev, a])),
      removeAttachment: (id) => setAttachments((prev) => prev.filter((p) => p.id !== id)),
    }),
    [sessionId],
  );
  const invokeComposerAction = (action: ComposerAction) => {
    setComposerError(null);
    void Promise.resolve(action.onInvoke(composerCtx)).catch((err) =>
      setComposerError(commandErrorMessage(err)),
    );
  };
  const composerCtxRef = useRef(composerCtx);
  composerCtxRef.current = composerCtx;
  const dropHandlersRef = useRef(dropHandlers);
  dropHandlersRef.current = dropHandlers;
  const hasDropHandler = dropHandlers.length > 0;
  const [dropOver, setDropOver] = useState(false);
  // Native Drops auf den Chat gehen an die registrierten Handler; ohne
  // Handler bleibt der Listener aus und das Ablegen tut im Chat nichts.
  // Pfade verlassen das Backend nicht (ADR 0113): der Handler holt sie dort.
  useEffect(() => {
    if (!hasDropHandler) return;
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    getCurrentWebview()
      .onDragDropEvent((event) => {
        const payload = event.payload;
        if (payload.type === "leave") {
          setDropOver(false);
          return;
        }
        // Only the visible chat area, and only under the pointer.
        const inside = isPointerInside(chatAreaRef.current, payload.position);
        if (payload.type === "over" || payload.type === "enter") {
          setDropOver(inside);
          return;
        }
        setDropOver(false);
        if (!inside) return;
        for (const handler of dropHandlersRef.current) {
          void Promise.resolve(handler(composerCtxRef.current)).catch((err) =>
            setComposerError(commandErrorMessage(err)),
          );
        }
      })
      .then((fn) => {
        if (cancelled) fn();
        else unlisten = fn;
      });
    return () => {
      cancelled = true;
      unlisten?.();
      setDropOver(false);
    };
  }, [hasDropHandler, chatAreaRef]);

  // Spec 0015, Abschnitt 5: einmalig pro Server geladen, kein
  // wiederholtes Nachladen bei jeder Pfeiltaste. `historyNav` verfolgt den
  // laufenden Navigations-Modus (s. `promptHistoryNav.ts`) — beim
  // Serverwechsel zurückgesetzt, damit kein Navigations-Zustand auf die
  // History eines anderen Servers zeigt.
  const [promptHistory, setPromptHistory] = useState<string[]>([]);
  const [historyNav, setHistoryNav] = useState<HistoryNavState>(initialHistoryNavState);

  useEffect(() => {
    listPromptHistory(serverId)
      .then(setPromptHistory)
      .catch(() => setPromptHistory([]));
    setHistoryNav(initialHistoryNavState);
  }, [serverId]);

  /** Reloads the history after the backend stored a sent prompt. */
  const reloadPromptHistory = () => {
    void listPromptHistory(serverId)
      .then(setPromptHistory)
      .catch(() => {});
  };
  const resetHistoryNav = () => setHistoryNav(initialHistoryNavState);

  // Spec 0015, Abschnitt 5: Pfeil-oben/-unten lösen Historien-Navigation nur
  // an den jeweiligen Feldrändern aus (Cursor-Position-Gate hier, reine
  // Index-Logik in `navigateHistory`) — sonst läuft die normale
  // Cursor-Bewegung des Browsers unverändert durch (kein `preventDefault`).
  const pendingCaretToEndRef = useRef(false);

  const handleInputKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    // Spec 0055, Teil 1: Standard-Chat-Verhalten — Enter sendet,
    // Shift+Enter fügt einen Zeilenumbruch ein.
    //
    // `isComposing`/`keyCode === 229`: during an IME composition Enter only
    // confirms the candidate selection and must not send.
    if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing && e.keyCode !== 229) {
      e.preventDefault();
      // Submit through the form (same path as the send button);
      // `requestSubmit` is missing in older WebKit, so fall back to a
      // synthetic `submit` event, which is what React's `onSubmit` hears.
      const form = e.currentTarget.form;
      if (form) {
        if (typeof form.requestSubmit === "function") {
          form.requestSubmit();
        } else {
          form.dispatchEvent(new Event("submit", { cancelable: true, bubbles: true }));
        }
      }
      return;
    }
    if (e.key !== "ArrowUp" && e.key !== "ArrowDown") return;
    const input = e.currentTarget;
    const atStart = input.selectionStart === 0 && input.selectionEnd === 0;
    const atEnd =
      input.selectionStart === input.value.length && input.selectionEnd === input.value.length;
    if (e.key === "ArrowUp" && !atStart) return;
    if (e.key === "ArrowDown" && !atEnd) return;

    const result = navigateHistory(
      e.key === "ArrowUp" ? "up" : "down",
      promptHistory,
      historyNav,
      draft,
    );
    if (!result) return;

    e.preventDefault();
    setHistoryNav(result.nextState);
    setDraft(result.value);
    pendingCaretToEndRef.current = true;
  };

  // Setzt den Cursor nach dem Einsetzen eines Historieneintrags ans Ende.
  // Läuft erst *nach* dem Commit des neuen `draft`-Werts, damit der DOM-Wert
  // feststeht; der Ref-Flag verhindert, dass normales Tippen den Cursor ans
  // Ende zwingt.
  useEffect(() => {
    if (pendingCaretToEndRef.current && inputRef.current) {
      const pos = inputRef.current.value.length;
      inputRef.current.setSelectionRange(pos, pos);
      pendingCaretToEndRef.current = false;
    }
  }, [draft, inputRef]);

  // Spec 0055, Teil 1: auto-grow up to the `max-h-40` cap, then scroll. The
  // height is reset to "auto" first so a shortened draft shrinks again.
  useEffect(() => {
    const textarea = inputRef.current;
    if (!textarea) return;
    textarea.style.height = "auto";
    textarea.style.height = `${textarea.scrollHeight}px`;
  }, [draft, inputRef]);

  const handleDraftChange = (value: string) => {
    setDraft(value);
    // Spec 0015, Abschnitt 5, bewusste MVP-Vereinfachung: jede normale
    // Texteingabe beendet den Navigations-Modus. Feuert nie für die
    // programmatischen `setDraft`-Aufrufe aus `handleInputKeyDown`.
    if (historyNav.historyIndex !== null) {
      setHistoryNav(initialHistoryNavState);
    }
  };

  return {
    draft,
    setDraft,
    attachments,
    setAttachments,
    composerError,
    setComposerError,
    composerCtx,
    composerActions,
    invokeComposerAction,
    hasDropHandler,
    dropOver,
    handleInputKeyDown,
    handleDraftChange,
    reloadPromptHistory,
    resetHistoryNav,
  };
}
