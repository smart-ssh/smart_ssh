import { useState } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import { deleteChatSession, renameChatSession } from "../api";
import type { ChatSessionSummaryDto } from "../types";

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
  const { t } = useTranslation();
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

  return createPortal(
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4">
      <div className="w-full max-w-lg rounded-lg bg-slate-800 p-6 shadow-xl">
        <h2 className="font-heading mb-1 text-lg font-semibold tracking-wide text-slate-100">
          {serverName}
        </h2>
        <p className="mb-4 text-sm text-slate-400">{t("chatSessionPicker.subtitle")}</p>

        {error && <p className="mb-3 text-sm text-red-400">{error}</p>}

        <button
          type="button"
          onClick={onNewConversation}
          className="mb-4 w-full rounded bg-indigo-600 px-4 py-2 text-sm font-medium text-white hover:bg-indigo-500"
        >
          {t("chatSessionPicker.newConversation")}
        </button>

        <p className="mb-2 text-xs font-semibold tracking-wide text-slate-400 uppercase">
          {t("chatSessionPicker.pastSessions")}
        </p>
        <ul className="max-h-72 divide-y divide-slate-700 overflow-y-auto rounded border border-slate-700">
          {sessions.map((session) => (
            <li key={session.sessionId} className="flex items-center gap-2 px-3 py-2">
              <button
                type="button"
                onClick={() => onResume(session.sessionId)}
                disabled={busySessionId === session.sessionId}
                className="flex-1 text-left disabled:opacity-50"
              >
                {renamingId === session.sessionId ? (
                  <input
                    autoFocus
                    type="text"
                    value={renameDraft}
                    onClick={(e) => e.stopPropagation()}
                    onChange={(e) => setRenameDraft(e.target.value)}
                    onBlur={() => commitRename(session.sessionId)}
                    onKeyDown={(e) => {
                      if (e.key === "Enter") commitRename(session.sessionId);
                      if (e.key === "Escape") setRenamingId(null);
                    }}
                    className="w-full rounded border border-slate-600 bg-slate-900 px-1.5 py-0.5 text-sm text-slate-100"
                  />
                ) : (
                  <p className="text-sm text-slate-100">
                    {session.title ?? t("chatSessionPicker.untitled")}
                  </p>
                )}
                <p className="text-xs text-slate-500">
                  {new Date(session.startedAt).toLocaleString()} ·{" "}
                  {t("chatSessionPicker.messageCount", { count: session.messageCount })}
                </p>
              </button>
              <button
                type="button"
                onClick={(e) => {
                  e.stopPropagation();
                  startRename(session);
                }}
                disabled={busySessionId === session.sessionId}
                title={t("chatSessionPicker.rename")}
                className="rounded px-2 py-1 text-xs text-slate-400 hover:bg-slate-700 hover:text-slate-200 disabled:opacity-50"
              >
                ✎
              </button>
              <button
                type="button"
                onClick={(e) => {
                  e.stopPropagation();
                  handleDelete(session.sessionId);
                }}
                disabled={busySessionId === session.sessionId}
                title={t("chatSessionPicker.delete")}
                className="rounded px-2 py-1 text-xs text-slate-400 hover:bg-red-900 hover:text-red-300 disabled:opacity-50"
              >
                🗑
              </button>
            </li>
          ))}
        </ul>

        <div className="mt-4 flex justify-end">
          <button
            type="button"
            onClick={onCancel}
            className="rounded border border-slate-600 px-3 py-1.5 text-sm text-slate-300 hover:bg-slate-700"
          >
            {t("chatSessionPicker.cancel")}
          </button>
        </div>
      </div>
    </div>,
    document.body,
  );
}
