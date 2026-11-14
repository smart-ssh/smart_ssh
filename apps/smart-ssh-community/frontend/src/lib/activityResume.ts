// Issue #1327, Spec 0116: resume handlers of frontend code and extensions.
// An activity that was started with `resume: { kind, key }` and still ran
// when the app quit is offered as "Interrupted" at the next start; "Resume"
// and "Discard" call the handler registered for its kind. Rust code
// registers its handlers on the backend instead.
import type { InterruptedActivity } from "../api/activitiesApi";

export interface ActivityResumeHandler {
  /** Starts a fresh activity that continues the interrupted work. */
  resume(entry: InterruptedActivity): void | Promise<void>;
  /** Cleans up (e.g. deletes partial files); optional. */
  discard?(entry: InterruptedActivity): void | Promise<void>;
}

const handlers = new Map<string, ActivityResumeHandler>();

/** Registers the handler for `kind`; returns a function that removes it.
 * Register at start-up, before the activity center reads the stored
 * entries: entries of a kind without a handler are dropped. The first
 * registration of a kind wins. */
export function registerActivityResumeHandler(kind: string, handler: ActivityResumeHandler): () => void {
  if (handlers.has(kind)) {
    console.warn(`activity resume handler for "${kind}" is already registered; ignored`);
    return () => undefined;
  }
  handlers.set(kind, handler);
  return () => {
    if (handlers.get(kind) === handler) handlers.delete(kind);
  };
}

export function activityResumeKinds(): string[] {
  return [...handlers.keys()];
}

export function activityResumeHandler(kind: string): ActivityResumeHandler | undefined {
  return handlers.get(kind);
}
