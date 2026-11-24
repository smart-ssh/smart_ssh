// Issue #611: the persisted chat session id of each runtime session, fed by
// `chat-session-changed` (sent at connect/resume, after a reconnect and after
// "New Chat") and by `list_sessions` after a reload. Kept outside the tab
// state because the event for a new session arrives before its tab turns
// into the connected session tab.
import { useSyncExternalStore } from "react";

const ids = new Map<string, string | null>();
const listeners = new Set<() => void>();

export function setChatSessionId(sessionId: string, chatSessionId: string | null): void {
  if (ids.has(sessionId) && ids.get(sessionId) === chatSessionId) return;
  ids.set(sessionId, chatSessionId);
  for (const listener of Array.from(listeners)) listener();
}

export function getChatSessionId(sessionId: string): string | null {
  return ids.get(sessionId) ?? null;
}

export function forgetChatSessionId(sessionId: string): void {
  if (ids.delete(sessionId)) for (const listener of Array.from(listeners)) listener();
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function useChatSessionId(sessionId: string): string | null {
  return useSyncExternalStore(subscribe, () => getChatSessionId(sessionId));
}

export function resetChatSessionIdsForTests(): void {
  ids.clear();
}
