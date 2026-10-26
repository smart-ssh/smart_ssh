import { useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { formatDateTime } from "../formatDateTime";
import { deleteChatSession, renameChatSession } from "../api";
import type { ChatSessionSummaryDto } from "../types";
import { Button, DialogFrame } from "./ui";
import { TECHNICAL_INPUT_PROPS } from "../technicalInputProps";

interface ChatSessionPickerScreenProps {
  serverName: string;
  sessions: ChatSessionSummaryDto[];
  onNewConversation: () => void;
  onResume: (sessionId: string) => void;
  /** Nach Löschen/Umbenennen ruft der Aufrufer `listChatSessions` erneut
   * auf, damit diese Komponente selbst keinen eigenen Nachlade-Zustand
   * pflegen muss. */
  onSessionsChanged: () => void;
  onCancel: () => void;
}

/**
 * Spec 0034, Abschnitt 6: Auswahl-Screen beim Verbinden auf einen Server
 * mit vorhandener Sitzungshistorie. "Neue Unterhaltung" ist die
 * Standardaktion (prominent, erster Fokus); die Liste vergangener
 * Sitzungen darunter zeigt Titel, Zeitpunkt, Nachrichtenanzahl, neueste
 * zuerst (bereits vom Backend so sortiert, s. `list_chat_sessions`).
 */
export function ChatSessionPickerScreen({
  serverName,
  sessions,
  onNewConversation,
  onResume,
  onSessionsChanged,
  onCancel,
}: ChatSessionPickerScreenProps) {
  const { t, i18n } = useTranslation();
  const newConversationRef = useRef<HTMLButtonElement>(null);
  const [error, setError] = useState<string | null>(null);
  const [busySessionId, setBusySessionId] = useState<string | null>(null);
  const [renamingId, setRenamingId] = useState<string | null>(null);
  const [renameDraft, setRenameDraft] = useState("");

  const handleDelete = async (sessionId: string) => {
    if (!window.confirm(t("chatSessionPicker.deleteConfirm"))) return;
    setBusySessionId(sessionId);
    setError(null);
    try {
      await deleteChatSession(sessionId);
      onSessionsChanged();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusySessionId(null);
    }
  };

  const startRename = (session: ChatSessionSummaryDto) => {
    setRenamingId(session.sessionId);
    setRenameDraft(session.title ?? "");
  };

  const commitRename = async (sessionId: string) => {
    const newTitle = renameDraft.trim();
    setRenamingId(null);
    if (!newTitle) return;
    setBusySessionId(sessionId);
    setError(null);
    try {
      await renameChatSession(sessionId, newTitle);
      onSessionsChanged();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusySessionId(null);
    }
  };

  return (
    <DialogFrame
      layer="app"
      title={<span className="break-words">{serverName}</span>}
      onClose={onCancel}
      initialFocusRef={newConversationRef}
      actions={<Button onClick={onCancel}>{t("chatSessionPicker.cancel")}</Button>}
    >
      <p className="mb-4 text-sm text-fg-subtle">{t("chatSessionPicker.subtitle")}</p>

      {error && (
        <p role="alert" className="mb-3 text-sm text-danger-fg">
          {error}
        </p>
      )}

      <Button ref={newConversationRef} variant="primary" onClick={onNewConversation} className="mb-4 w-full narrow:min-h-11 narrow:shrink-0">
        {t("chatSessionPicker.newConversation")}
      </Button>

      <p className="mb-2 text-xs font-semibold tracking-wide text-fg-subtle uppercase">
        {t("chatSessionPicker.pastSessions")}
      </p>
      <ul className="max-h-72 narrow:max-h-none divide-y divide-border overflow-y-auto rounded border border-border">
        {sessions.map((session) => (
          <li key={session.sessionId} className="flex items-center gap-2 px-3 py-2">
            <button
              type="button"
              onClick={() => onResume(session.sessionId)}
              disabled={busySessionId === session.sessionId}
              className="min-w-0 flex-1 rounded text-left narrow:min-h-11 focus-visible:ring-2 focus-visible:ring-accent-fg focus-visible:outline-none disabled:opacity-50"
            >
              {renamingId === session.sessionId ? (
                <input
                  {...TECHNICAL_INPUT_PROPS}
                  // eslint-disable-next-line jsx-a11y/no-autofocus -- das Umbenennen-Feld erscheint erst auf die ausdrückliche Aktion „Umbenennen“ und muss den Fokus bekommen, damit man sofort tippen kann (Issue #112).
                  autoFocus
                  type="text"
                  aria-label={t("chatSessionPicker.rename")}
                  value={renameDraft}
                  onClick={(e) => e.stopPropagation()}
                  onChange={(e) => setRenameDraft(e.target.value)}
                  onBlur={() => commitRename(session.sessionId)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") commitRename(session.sessionId);
                    if (e.key === "Escape") {
                      // Cancels the rename only; the dialog stays open.
                      e.stopPropagation();
                      setRenamingId(null);
                    }
                  }}
                  className="w-full rounded border border-border-strong bg-surface px-1.5 py-0.5 text-sm text-fg focus-visible:ring-2 focus-visible:ring-accent-fg focus-visible:outline-none"
                />
              ) : (
                <p className="truncate text-sm text-fg" title={session.title ?? undefined}>
                  {session.title ?? t("chatSessionPicker.untitled")}
                </p>
              )}
              <p className="text-xs text-fg-subtle">
                {formatDateTime(session.startedAt, i18n.language)} ·{" "}
                {t("chatSessionPicker.messageCount", { count: session.messageCount })}
              </p>
            </button>
            <Button
              variant="ghost"
              size="sm"
              className="narrow:min-h-11 narrow:min-w-11"
              onClick={(e) => {
                e.stopPropagation();
                startRename(session);
              }}
              disabled={busySessionId === session.sessionId}
              title={t("chatSessionPicker.rename")}
              aria-label={t("chatSessionPicker.rename")}
            >
              ✎
            </Button>
            <Button
              variant="ghost"
              size="sm"
              className="narrow:min-h-11 narrow:min-w-11"
              onClick={(e) => {
                e.stopPropagation();
                handleDelete(session.sessionId);
              }}
              disabled={busySessionId === session.sessionId}
              title={t("chatSessionPicker.delete")}
              aria-label={t("chatSessionPicker.delete")}
            >
              🗑
            </Button>
          </li>
        ))}
      </ul>
    </DialogFrame>
  );
}
