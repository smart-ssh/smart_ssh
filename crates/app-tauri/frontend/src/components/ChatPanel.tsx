import { type FormEvent, useEffect, useRef, useState } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import {
  acceptAndCreateRule,
  commandErrorMessage,
  exportDocument,
  listAiProviders,
  respondToAction,
  sendChatMessage,
  suggestRulePatterns,
} from "../api";
import {
  onChatActionProposed,
  onChatActionResult,
  onChatDocumentGenerated,
  onChatError,
  onChatTextDelta,
} from "../events";
import type {
  ActionResultPayload,
  ActionUserDecision,
  AiAction,
  Decision,
  DocumentFormat,
  PatternSuggestionDto,
  PatternType,
  Scope,
} from "../types";

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
  | { type: "error"; id: string; message: string }
  | { type: "document"; id: string; title: string; contentMarkdown: string };

function describeAction(action: AiAction): { label: string; command?: string } {
  if ("SuggestCommand" in action) {
    return { label: "Kommando vorschlagen", command: action.SuggestCommand.command };
  }
  if ("GenerateDocument" in action) {
    return { label: `Dokument generieren: ${action.GenerateDocument.title}` };
  }
  const target = action.ProposeNoteUpdate.target;
  const targetLabel = "Server" in target ? `Server ${target.Server}` : `Gruppe ${target.Group}`;
  return { label: `Notiz aktualisieren (${targetLabel})` };
}

/// `Decision::Confirm`/`Deny`-Reasons aus der Filter-Engine sind
/// `"; "`-getrennte Einzelgründe (`crate::filter::engine::merge_reasons`).
/// Als ein durchgehender Satz mit Semikola wirkt das bei mehreren Gründen
/// unübersichtlich — hier stattdessen als kompakte, durch " · " getrennte
/// Liste dargestellt.
function formatReason(reason: string): string {
  return reason
    .split("; ")
    .filter((part, index, all) => all.indexOf(part) === index)
    .join(" · ");
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
  /** Spec 0011, Abschnitt 4: Default-Scope für den Regel-Schnellvorschlag
   * ist der aktuell verbundene Server. */
  serverId: string;
}

export function ChatPanel({ sessionId, serverId }: ChatPanelProps) {
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
      onChatDocumentGenerated((event) => {
        if (event.sessionId !== sessionId) return;
        setItems((prev) => [
          ...prev,
          {
            type: "document",
            id: freshId(),
            title: event.title,
            contentMarkdown: event.contentMarkdown,
          },
        ]);
      }),
    ];

    return () => {
      unlisten.forEach((p) => p.then((unlistenFn) => unlistenFn()));
    };
  }, [sessionId]);

  useEffect(() => {
    scrollRef.current?.scrollTo({ top: scrollRef.current.scrollHeight });
  }, [items]);

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

  /** Spec 0011, Abschnitt 3: `accept_and_create_rule` legt die Regel an
   * **und** löst die Confirm-Entscheidung selbst auf (Backend) — anders als
   * `respond()` oben ruft dies also `respondToAction` nicht zusätzlich auf. */
  const acceptWithRule = (
    actionId: string,
    patternType: PatternType,
    patternValue: string,
    scope: Scope,
  ) => {
    setItems((prev) =>
      prev.map((item) =>
        item.type === "action" && item.actionId === actionId
          ? { ...item, responded: true }
          : item,
      ),
    );
    acceptAndCreateRule(sessionId, actionId, patternType, patternValue, scope).catch((err) =>
      setItems((prev) => [
        ...prev,
        { type: "error", id: freshId(), message: commandErrorMessage(err) },
      ]),
    );
  };

  /** Spec 0012, Abschnitt 3: der native Speichern-Dialog selbst lebt im
   * Backend (`export_document`) — bricht der Nutzer ihn ab, kehrt der
   * Command einfach ohne Fehler zurück (s. dortiger Doc-Kommentar), hier
   * ist dann schlicht nichts zu tun. */
  const handleExport = async (contentMarkdown: string, title: string, format: DocumentFormat) => {
    try {
      await exportDocument(contentMarkdown, title, format);
    } catch (err) {
      setItems((prev) => [
        ...prev,
        { type: "error", id: freshId(), message: commandErrorMessage(err) },
      ]);
      throw err;
    }
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
          <ChatItemView
            key={item.id}
            item={item}
            onRespond={respond}
            onAcceptWithRule={acceptWithRule}
            onExport={handleExport}
            serverId={serverId}
          />
        ))}
        {sending && (
          <div className="flex items-center gap-2 rounded-lg bg-slate-800/80 px-3 py-2 text-xs text-indigo-300">
            <span className="inline-block h-2 w-2 animate-ping rounded-full bg-indigo-400" />
            <span>KI generiert Antwort / Dokument…</span>
          </div>
        )}
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

function ChatItemView({
  item,
  onRespond,
  onAcceptWithRule,
  onExport,
  serverId,
}: {
  item: ChatItem;
  onRespond: (actionId: string, decision: ActionUserDecision) => void;
  onAcceptWithRule: (
    actionId: string,
    patternType: PatternType,
    patternValue: string,
    scope: Scope,
  ) => void;
  onExport: (contentMarkdown: string, title: string, format: DocumentFormat) => Promise<void>;
  serverId: string;
}) {
  if (item.type === "document") {
    return (
      <DocumentCard title={item.title} contentMarkdown={item.contentMarkdown} onExport={onExport} />
    );
  }
  if (item.type === "user") {
    return (
      <div className="ml-auto max-w-[80%] rounded-lg bg-indigo-600 px-3 py-2 text-sm text-white">
        {item.text}
      </div>
    );
  }
  if (item.type === "assistant") {
    return <AssistantMessageView text={item.text} onExport={onExport} />;
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
        <p className="mt-1 text-xs text-slate-400">{formatReason(item.decision.Confirm.reason)}</p>
      )}
      {typeof item.decision === "object" && "Deny" in item.decision && (
        <p className="mt-1 text-xs text-red-300">{formatReason(item.decision.Deny.reason)}</p>
      )}

      {needsConfirmation && (
        <ConfirmActionForm
          actionId={item.actionId}
          initialCommand={command}
          onRespond={onRespond}
          onAcceptWithRule={onAcceptWithRule}
          serverId={serverId}
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
  onAcceptWithRule,
  serverId,
}: {
  actionId: string;
  initialCommand?: string;
  onRespond: (actionId: string, decision: ActionUserDecision) => void;
  onAcceptWithRule: (
    actionId: string,
    patternType: PatternType,
    patternValue: string,
    scope: Scope,
  ) => void;
  serverId: string;
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
      <div className="flex flex-wrap items-center gap-2">
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
        {/* Spec 0011: nur für Kommando-Vorschläge sinnvoll, nicht für
         * Notiz-Aktualisierungen (kein `initialCommand` dort). */}
        {initialCommand !== undefined && (
          <QuickRuleButton
            actionId={actionId}
            command={edited}
            serverId={serverId}
            onAccept={onAcceptWithRule}
          />
        )}
      </div>
    </div>
  );
}

/** Spec 0011, Abschnitt 4: zusätzlicher Button neben Ausführen/Ablehnen,
 * öffnet ein kompaktes Dropdown mit Muster-Vorschlägen + Scope-Auswahl
 * (Default: aktueller Server). Klick auf einen Vorschlag ruft
 * `accept_and_create_rule` auf und schließt das Dropdown — der
 * Bestätigungsdialog selbst verschwindet dann über `onAccept`s
 * optimistisches `responded: true` (s. `ChatPanel.acceptWithRule`), genau
 * wie bei Ausführen/Ablehnen. */
function QuickRuleButton({
  actionId,
  command,
  serverId,
  onAccept,
}: {
  actionId: string;
  command: string;
  serverId: string;
  onAccept: (
    actionId: string,
    patternType: PatternType,
    patternValue: string,
    scope: Scope,
  ) => void;
}) {
  const [open, setOpen] = useState(false);
  const [suggestions, setSuggestions] = useState<PatternSuggestionDto[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [scopeKind, setScopeKind] = useState<"server" | "global" | "tag">("server");
  const [tag, setTag] = useState("");

  const toggleOpen = () => {
    const next = !open;
    setOpen(next);
    if (next && suggestions.length === 0 && !loading) {
      setLoading(true);
      setError(null);
      suggestRulePatterns(command)
        .then(setSuggestions)
        .catch((err) => setError(commandErrorMessage(err)))
        .finally(() => setLoading(false));
    }
  };

  const buildScope = (): Scope | null => {
    if (scopeKind === "global") return "Global";
    if (scopeKind === "server") return { Server: serverId };
    const trimmed = tag.trim();
    return trimmed ? { Tag: trimmed } : null;
  };

  const handlePick = (suggestion: PatternSuggestionDto) => {
    const scope = buildScope();
    if (!scope) return;
    onAccept(actionId, suggestion.patternType, suggestion.patternValue, scope);
    setOpen(false);
  };

  return (
    <div className="relative inline-block">
      <button
        type="button"
        onClick={toggleOpen}
        className="rounded bg-slate-700 px-3 py-1 text-xs text-slate-100 hover:bg-slate-600"
      >
        Akzeptieren und Regel erstellen ▾
      </button>
      {open && (
        <div className="absolute z-10 mt-1 w-72 space-y-2 rounded border border-slate-600 bg-slate-800 p-2 shadow-lg">
          <div className="flex gap-3 text-xs text-slate-300">
            <label className="flex items-center gap-1">
              <input
                type="radio"
                checked={scopeKind === "server"}
                onChange={() => setScopeKind("server")}
              />
              Dieser Server
            </label>
            <label className="flex items-center gap-1">
              <input
                type="radio"
                checked={scopeKind === "global"}
                onChange={() => setScopeKind("global")}
              />
              Global
            </label>
            <label className="flex items-center gap-1">
              <input
                type="radio"
                checked={scopeKind === "tag"}
                onChange={() => setScopeKind("tag")}
              />
              Tag
            </label>
          </div>
          {scopeKind === "tag" && (
            <input
              type="text"
              value={tag}
              onChange={(e) => setTag(e.target.value)}
              placeholder="Tag-Name"
              className="w-full rounded border border-slate-600 bg-slate-900 px-2 py-1 text-xs text-slate-100"
            />
          )}
          {loading && <p className="text-xs text-slate-400">Lädt Vorschläge…</p>}
          {error && <p className="text-xs text-red-400">{error}</p>}
          {!loading && !error && suggestions.length === 0 && (
            <p className="text-xs text-slate-400">Keine Vorschläge.</p>
          )}
          <ul className="space-y-1">
            {suggestions.map((suggestion) => (
              <li key={suggestion.patternValue}>
                <button
                  type="button"
                  onClick={() => handlePick(suggestion)}
                  className="w-full rounded px-2 py-1 text-left text-xs text-slate-200 hover:bg-slate-700"
                >
                  {suggestion.label}
                </button>
              </li>
            ))}
          </ul>
        </div>
      )}
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

/** Spec 0012, Abschnitt 3: eigene Karte für `chat-document-generated`,
 * gerendertes Markdown statt Rohtext, mit den beiden Export-Buttons. Kein
 * Bestätigungsdialog/keine Approve-Deny-Buttons wie bei `type: "action"` —
 * es ist bereits nichts Persistentes passiert, das rückgängig zu machen
 * wäre. */
function DocumentCard({
  title,
  contentMarkdown,
  onExport,
}: {
  title: string;
  contentMarkdown: string;
  onExport: (contentMarkdown: string, title: string, format: DocumentFormat) => Promise<void>;
}) {
  const [exporting, setExporting] = useState<DocumentFormat | null>(null);
  const [savedFormat, setSavedFormat] = useState<DocumentFormat | null>(null);

  const handleExportClick = async (format: DocumentFormat) => {
    setExporting(format);
    try {
      await onExport(contentMarkdown, title, format);
      setSavedFormat(format);
    } finally {
      setExporting(null);
    }
  };

  return (
    <div className="rounded-lg border border-indigo-700/60 bg-slate-800/80 p-3.5 text-sm shadow-md">
      <div className="mb-2 flex items-center justify-between">
        <div className="flex items-center gap-2">
          <span className="text-base">📄</span>
          <span className="font-semibold text-slate-100">{title}</span>
        </div>
        <span className="rounded bg-indigo-950/80 px-2 py-0.5 text-xs text-indigo-300 border border-indigo-800/50">
          Dokument generiert
        </span>
      </div>
      <div className="prose prose-sm prose-invert max-w-none rounded bg-slate-950 p-3 prose-pre:bg-slate-900 prose-p:my-1 prose-ul:my-1 prose-ol:my-1 prose-headings:my-1.5">
        <ReactMarkdown remarkPlugins={[remarkGfm]}>{contentMarkdown}</ReactMarkdown>
      </div>
      <div className="mt-3 flex flex-wrap items-center gap-2">
        <button
          type="button"
          disabled={exporting !== null}
          onClick={() => handleExportClick("markdown")}
          className="rounded bg-indigo-700 px-3 py-1.5 text-xs font-medium text-white hover:bg-indigo-600 disabled:opacity-50"
        >
          {exporting === "markdown" ? "Speichert…" : "Als Markdown speichern"}
        </button>
        <button
          type="button"
          disabled={exporting !== null}
          onClick={() => handleExportClick("word")}
          className="rounded bg-indigo-700 px-3 py-1.5 text-xs font-medium text-white hover:bg-indigo-600 disabled:opacity-50"
        >
          {exporting === "word" ? "Speichert…" : "Als Word speichern (.docx)"}
        </button>
        {savedFormat && (
          <span className="text-xs text-emerald-400">
            ✓ Als {savedFormat === "word" ? "Word-Dokument" : "Markdown"} exportiert
          </span>
        )}
      </div>
    </div>
  );
}

function AssistantMessageView({
  text,
  onExport,
}: {
  text: string;
  onExport: (contentMarkdown: string, title: string, format: DocumentFormat) => Promise<void>;
}) {
  const [exporting, setExporting] = useState<DocumentFormat | null>(null);
  const [savedFormat, setSavedFormat] = useState<DocumentFormat | null>(null);

  const handleExportClick = async (format: DocumentFormat) => {
    setExporting(format);
    try {
      const firstLine =
        text
          .trim()
          .split("\n")[0]
          .replace(/^[#\s*-]+/, "")
          .slice(0, 40) || "Antwort";
      await onExport(text, firstLine, format);
      setSavedFormat(format);
    } finally {
      setExporting(null);
    }
  };

  return (
    <div className="max-w-[85%] space-y-2 rounded-lg bg-slate-800 p-3 text-sm text-slate-100 shadow-sm">
      <div className="prose prose-sm prose-invert max-w-none prose-pre:bg-slate-950 prose-p:my-1 prose-ul:my-1 prose-ol:my-1 prose-headings:my-1.5">
        <ReactMarkdown remarkPlugins={[remarkGfm]}>{text}</ReactMarkdown>
      </div>
      <div className="flex flex-wrap items-center gap-2 border-t border-slate-700/60 pt-2 text-xs">
        <span className="text-slate-400">Export:</span>
        <button
          type="button"
          disabled={exporting !== null}
          onClick={() => handleExportClick("markdown")}
          className="rounded bg-slate-700/80 px-2 py-1 text-xs text-slate-200 hover:bg-slate-600 hover:text-white disabled:opacity-50"
        >
          {exporting === "markdown" ? "Speichert…" : "📄 Als Markdown"}
        </button>
        <button
          type="button"
          disabled={exporting !== null}
          onClick={() => handleExportClick("word")}
          className="rounded bg-slate-700/80 px-2 py-1 text-xs text-slate-200 hover:bg-slate-600 hover:text-white disabled:opacity-50"
        >
          {exporting === "word" ? "Speichert…" : "📄 Als Word (.docx)"}
        </button>
        {savedFormat && (
          <span className="text-xs text-emerald-400">
            ✓ Als {savedFormat === "word" ? "Word" : "Markdown"} exportiert
          </span>
        )}
      </div>
    </div>
  );
}
