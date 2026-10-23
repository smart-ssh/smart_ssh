/** Spec 0057, §4.2 (Etappe 4): "Mache ich selbst" (im Kürzungs-Vorschlags-
 * Dialog, `NoteShrinkSuggestionToast.tsx`, app-weit an der Wurzel von
 * `App.tsx` gemountet — außerhalb von `ManagementView`s eigenem
 * Auswahl-Zustand) muss die Notiz-Bearbeitung für einen BESTIMMTEN Server
 * öffnen können. Dasselbe Bus-Muster wie `extensions/featureLockedBus.ts`
 * (dortiger Doc-Kommentar): eine zentrale Anlaufstelle statt Props quer
 * durch den Baum zu reichen, den `NoteShrinkSuggestionToast` (an der
 * `App`-Wurzel) gar nicht kennt. */

type Listener = (serverId: string) => void;

const listeners = new Set<Listener>();

export function publishRequestServerNoteEdit(serverId: string): void {
  for (const listener of listeners) listener(serverId);
}

/** Gibt eine Unsubscribe-Funktion zurück (React-`useEffect`-Cleanup-Form). */
export function subscribeRequestServerNoteEdit(listener: Listener): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** Issue #611: an extension asks to resume a saved chat session of a server.
 * `App` handles it with the same path as the session picker (switch to an
 * open tab of the server, otherwise the first-run gate and the connecting
 * tab). */
export interface ResumeChatSessionRequest {
  serverId: string;
  chatSessionId: string;
}

type ResumeListener = (request: ResumeChatSessionRequest) => void;

const resumeListeners = new Set<ResumeListener>();

export function requestResumeChatSession(serverId: string, chatSessionId: string): void {
  for (const listener of Array.from(resumeListeners)) listener({ serverId, chatSessionId });
}

export function subscribeRequestResumeChatSession(listener: ResumeListener): () => void {
  resumeListeners.add(listener);
  return () => resumeListeners.delete(listener);
}

/** Issue #696: a chat card asks to open one filter rule for editing in the
 * filter rules tab. `App` switches the tab; `FilterRulesView` selects it. */
type OpenFilterRuleListener = (ruleId: string) => void;

const openFilterRuleListeners = new Set<OpenFilterRuleListener>();

export function requestOpenFilterRule(ruleId: string): void {
  for (const listener of Array.from(openFilterRuleListeners)) listener(ruleId);
}

export function subscribeRequestOpenFilterRule(listener: OpenFilterRuleListener): () => void {
  openFilterRuleListeners.add(listener);
  return () => openFilterRuleListeners.delete(listener);
}
