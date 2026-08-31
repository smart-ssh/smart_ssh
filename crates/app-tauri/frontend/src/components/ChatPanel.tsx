import { type FormEvent, useEffect, useRef, useState } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import {
  commandErrorMessage,
  listAiProviders,
  respondToAction,
  sendChatMessage,
} from "../api";
import { onChatActionProposed, onChatActionResult, onChatError, onChatTextDelta } from "../events";
import type { ActionResultPayload, ActionUserDecision, AiAction, Decision } from "../types";

type ChatItem =
  | { type: "user"; id: string; text: string }
  | { type: "assistant"; id: string; text: string }
  | {
      type: "action";
      id: string;
      actionId: string;
      action: AiAction;
      decision: Decision;
      /** Optimistisches lokales Flag — Deny über `respond_to_action` löst
       * (anders als Approve/EditThenApprove) kein `chat-action-result` aus,
       * die Buttons müssen also unabhängig davon verschwinden. */
      responded: boolean;
      result?: ActionResultPayload;
    }
  | { type: "error"; id: string; message: string };

function describeAction(action: AiAction): { label: string; command?: string } {
  if ("SuggestCommand" in action) {
    return { label: "Kommando vorschlagen", command: action.SuggestCommand.command };
  }
  const target = action.ProposeNoteUpdate.target;
  const targetLabel = "Server" in target ? `Server ${target.Server}` : `Gruppe ${target.Group}`;
  return { label: `Notiz aktualisieren (${targetLabel})` };
}

function decisionBadge(decision: Decision): { text: string; className: string } {
  if (decision === "AutoExec") {
    return { text: "automatisch ausgeführt", className: "bg-emerald-900 text-emerald-300" };
  }
  if ("Confirm" in decision) {
    return { text: "Bestätigung nötig", className: "bg-amber-900 text-amber-300" };
  }
  return { text: "blockiert", className: "bg-red-900 text-red-300" };
}

let nextId = 0;
const freshId = () => `item-${nextId++}`;

interface ChatPanelProps {
  sessionId: string;
}

export function ChatPanel({ sessionId }: ChatPanelProps) {
  const [items, setItems] = useState<ChatItem[]>([]);
  const [draft, setDraft] = useState("");
  const [sending, setSending] = useState(false);
  const [hasActiveProvider, setHasActiveProvider] = useState<boolean | null>(null);
  const scrollRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    listAiProviders()
      .then((providers) => setHasActiveProvider(providers.some((p) => p.isActive)))
      .catch(() => setHasActiveProvider(null));
  }, []);

  useEffect(() => {
    const unlisten = [
      onChatTextDelta((event) => {
        if (event.sessionId !== sessionId) return;
        setItems((prev) => {
          const last = prev[prev.length - 1];
          if (last?.type === "assistant") {
            return [...prev.slice(0, -1), { ...last, text: last.text + event.delta }];
          }
          return [...prev, { type: "assistant", id: freshId(), text: event.delta }];
        });
      }),
      onChatActionProposed((event) => {
        if (event.sessionId !== sessionId) return;
        setItems((prev) => [
          ...prev,
          {
            type: "action",
            id: freshId(),
            actionId: event.actionId,
            action: event.action,
            decision: event.decision,
            responded: false,
          },
        ]);
      }),
      onChatActionResult((event) => {
        if (event.sessionId !== sessionId) return;
        setItems((prev) =>
          prev.map((item) =>
            item.type === "action" && item.actionId === event.actionId
              ? { ...item, result: event.result, responded: true }
              : item,
          ),
        );
      }),
      onChatError((event) => {
        if (event.sessionId !== sessionId) return;
        setItems((prev) => [...prev, { type: "error", id: freshId(), message: event.message }]);
      }),
    ];

    return () => {
      unlisten.forEach((p) => p.then((unlistenFn) => unlistenFn()));
    };
  }, [sessionId]);

  useEffect(() => {
    scrollRef.current?.scrollTo({ top: scrollRef.current.scrollHeight });
  }, [items]);

  // Spec 0007, Abschnitt 7: solange die send_chat_message-Anfrage noch
  // aussteht und der letzte Eintrag keine bereits laufende
  // Assistenten-Antwort ist (z. B. direkt nach dem Senden, oder während
  // einer Confirm-Bestätigung/nach deren Ausführung), warten wir sichtbar
  // auf die nächste KI-Reaktion.
  const lastItem = items[items.length - 1];
  const showTypingIndicator = sending && lastItem?.type !== "assistant";

  const respond = (actionId: string, decision: ActionUserDecision) => {
    setItems((prev) =>
      prev.map((item) =>
        item.type === "action" && item.actionId === actionId
          ? { ...item, responded: true }
          : item,
      ),
    );
    respondToAction(sessionId, actionId, decision).catch((err) =>
      setItems((prev) => [
        ...prev,
        { type: "error", id: freshId(), message: commandErrorMessage(err) },
      ]),
    );
  };

  const handleSubmit = async (event: FormEvent) => {
    event.preventDefault();
    const text = draft.trim();
    if (!text || sending) return;
    setItems((prev) => [...prev, { type: "user", id: freshId(), text }]);
    setDraft("");
    setSending(true);
    try {
      await sendChatMessage(sessionId, text);
    } catch (err) {
      setItems((prev) => [
        ...prev,
        { type: "error", id: freshId(), message: commandErrorMessage(err) },
      ]);
    } finally {
      setSending(false);
    }
  };

  return (
    <div className="flex h-full flex-col">
      <div ref={scrollRef} className="flex-1 space-y-3 overflow-y-auto p-4">
        {items.map((item) => (
          <ChatItemView key={item.id} item={item} onRespond={respond} />
        ))}
        {showTypingIndicator && <TypingIndicator />}
      </div>

      {hasActiveProvider === false ? (
        <div className="border-t border-slate-700 p-4 text-sm text-amber-300">
          Kein aktiver AI-Provider konfiguriert. Bitte zuerst in den Einstellungen einrichten.
        </div>
      ) : (
        <form onSubmit={handleSubmit} className="flex gap-2 border-t border-slate-700 p-3">
          <input
            type="text"
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            placeholder="Nachricht an die KI…"
            disabled={sending}
            className="flex-1 rounded border border-slate-600 bg-slate-900 px-3 py-2 text-sm text-slate-100"
          />
          <button
            type="submit"
            disabled={sending || draft.trim().length === 0}
            className="rounded bg-indigo-600 px-4 py-2 text-sm font-medium text-white hover:bg-indigo-500 disabled:opacity-50"
          >
            Senden
          </button>
        </form>
      )}
    </div>
  );
}

function TypingIndicator() {
  return (
    <div className="flex max-w-[80%] items-center gap-1 rounded-lg bg-slate-800 px-3 py-2.5">
      {[0, 1, 2].map((i) => (
        <span
          key={i}
          className="h-1.5 w-1.5 animate-bounce rounded-full bg-slate-400"
          style={{ animationDelay: `${i * 0.15}s` }}
        />
      ))}
    </div>
  );
}

function ChatItemView({
  item,
  onRespond,
}: {
  item: ChatItem;
  onRespond: (actionId: string, decision: ActionUserDecision) => void;
}) {
  if (item.type === "user") {
    return (
      <div className="ml-auto max-w-[80%] rounded-lg bg-indigo-600 px-3 py-2 text-sm text-white">
        {item.text}
      </div>
    );
  }
  if (item.type === "assistant") {
    return (
      <div className="prose prose-sm prose-invert max-w-[80%] rounded-lg bg-slate-800 px-3 py-2 prose-pre:bg-slate-950 prose-p:my-1 prose-ul:my-1 prose-ol:my-1 prose-headings:my-1.5">
        <ReactMarkdown remarkPlugins={[remarkGfm]}>{item.text}</ReactMarkdown>
      </div>
    );
  }
  if (item.type === "error") {
    return (
      <div className="rounded-lg bg-red-950 px-3 py-2 text-sm text-red-300">⚠ {item.message}</div>
    );
  }

  const { label, command } = describeAction(item.action);
  const badge = decisionBadge(item.decision);
  const needsConfirmation = !item.responded && typeof item.decision === "object" && "Confirm" in item.decision;

  return (
    <div className="rounded-lg border border-slate-700 bg-slate-800/60 p-3 text-sm">
      <div className="mb-1 flex items-center gap-2">
        <span className="font-medium text-slate-100">{label}</span>
        <span className={`rounded px-2 py-0.5 text-xs ${badge.className}`}>{badge.text}</span>
      </div>
      {command && <code className="block rounded bg-slate-950 px-2 py-1 text-xs text-slate-300">{command}</code>}
      {typeof item.decision === "object" && "Confirm" in item.decision && (
        <p className="mt-1 text-xs text-slate-400">{item.decision.Confirm.reason}</p>
      )}
      {typeof item.decision === "object" && "Deny" in item.decision && (
        <p className="mt-1 text-xs text-red-300">{item.decision.Deny.reason}</p>
      )}

      {needsConfirmation && (
        <ConfirmActionForm
          actionId={item.actionId}
          initialCommand={command}
          onRespond={onRespond}
        />
      )}

      {item.result && <ActionResultView result={item.result} />}
    </div>
  );
}

function ConfirmActionForm({
  actionId,
  initialCommand,
  onRespond,
}: {
  actionId: string;
  initialCommand?: string;
  onRespond: (actionId: string, decision: ActionUserDecision) => void;
}) {
  const [edited, setEdited] = useState(initialCommand ?? "");

  const handleApprove = () => {
    if (initialCommand !== undefined && edited !== initialCommand) {
      onRespond(actionId, { decision: "editThenApprove", command: edited });
    } else {
      onRespond(actionId, { decision: "approve" });
    }
  };

  return (
    <div className="mt-2 space-y-2">
      {initialCommand !== undefined && (
        <textarea
          value={edited}
          onChange={(e) => setEdited(e.target.value)}
          rows={2}
          className="w-full rounded border border-slate-600 bg-slate-950 px-2 py-1 font-mono text-xs text-slate-100"
        />
      )}
      <div className="flex gap-2">
        <button
          type="button"
          onClick={() => onRespond(actionId, { decision: "deny" })}
          className="rounded bg-red-900 px-3 py-1 text-xs text-red-200 hover:bg-red-800"
        >
          Ablehnen
        </button>
        <button
          type="button"
          onClick={handleApprove}
          className="rounded bg-emerald-700 px-3 py-1 text-xs text-white hover:bg-emerald-600"
        >
          Ausführen
        </button>
      </div>
    </div>
  );
}

function ActionResultView({ result }: { result: ActionResultPayload }) {
  if (result.kind === "noteUpdate") {
    return <p className="mt-2 text-xs text-emerald-300">{result.summary}</p>;
  }
  const truncate = (s: string, max = 2000) => (s.length > max ? `${s.slice(0, max)}\n… (gekürzt)` : s);
  return (
    <div className="mt-2 space-y-1 rounded bg-slate-950 p-2 font-mono text-xs">
      {result.stdout && (
        <pre className="whitespace-pre-wrap text-slate-300">{truncate(result.stdout)}</pre>
      )}
      {result.stderr && (
        <pre className="whitespace-pre-wrap text-red-300">{truncate(result.stderr)}</pre>
      )}
      <p className="text-slate-500">exit code: {result.exitCode ?? "—"}</p>
    </div>
  );
}
