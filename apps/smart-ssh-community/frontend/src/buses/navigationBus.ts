/** Spec 0057, §4.2 (Etappe 4): "Mache ich selbst" (im Kürzungs-Vorschlags-
 * Dialog, `NoteShrinkSuggestionToast.tsx`, app-weit an der Wurzel von
 * `App.tsx` gemountet — außerhalb von `ManagementView`s eigenem
 * Auswahl-Zustand) muss die Notiz-Bearbeitung für einen BESTIMMTEN Server
 * öffnen können. Dasselbe Bus-Muster wie `extensions/featureLockedBus.ts`
 * (dortiger Doc-Kommentar): eine zentrale Anlaufstelle statt Props quer
 * durch den Baum zu reichen, den `NoteShrinkSuggestionToast` (an der
 * `App`-Wurzel) gar nicht kennt. */

import { createBus } from "./createBus";

type Listener = (serverId: string) => void;

const noteEditBus = createBus<string>();

export function publishRequestServerNoteEdit(serverId: string): void {
  noteEditBus.publish(serverId);
}

/** Gibt eine Unsubscribe-Funktion zurück (React-`useEffect`-Cleanup-Form). */
export function subscribeRequestServerNoteEdit(listener: Listener): () => void {
  return noteEditBus.subscribe(listener);
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

const resumeBus = createBus<ResumeChatSessionRequest>();

export function requestResumeChatSession(serverId: string, chatSessionId: string): void {
  resumeBus.publish({ serverId, chatSessionId });
}

export function subscribeRequestResumeChatSession(listener: ResumeListener): () => void {
  return resumeBus.subscribe(listener);
}

/** Issue #696: a chat card asks to open one filter rule for editing in the
 * filter rules tab. `App` switches the tab; `FilterRulesView` selects it. */
type OpenFilterRuleListener = (ruleId: string) => void;

const openFilterRuleBus = createBus<string>();

export function requestOpenFilterRule(ruleId: string): void {
  openFilterRuleBus.publish(ruleId);
}

export function subscribeRequestOpenFilterRule(listener: OpenFilterRuleListener): () => void {
  return openFilterRuleBus.subscribe(listener);
}
