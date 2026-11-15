// Issue #1325, Spec 0116: public API for activities whose work runs in the
// frontend. Same operations as the Rust `ActivityHandle`; exported from
// `public-api.ts`.
import type { UnlistenFn } from "@tauri-apps/api/event";
import {
  cancelActivity,
  finishActivityCommand,
  listActivities,
  startActivityCommand,
  updateActivityCommand,
  type Activity,
  type ActivitySpec,
} from "../api/activitiesApi";
import { onActivityChanged } from "../api/events";

/** What a caller gives to `startActivity`; optional fields default to "none". */
export interface StartActivitySpec {
  kind: string;
  /** Identifies the subject within the kind (e.g. a model id). */
  key?: string;
  titleKey: string;
  titleParams?: Record<string, string>;
  /** Must be `true` for the row to offer a cancel button; `onCancel` then runs. */
  cancellable?: boolean;
  total?: number | null;
  /** Makes the activity resumable: when the app quits while it runs, it is
   * offered as "Interrupted" at the next start and handed to the handler
   * registered with `registerActivityResumeHandler(kind, …)`. */
  resume?: { kind: string; key: string };
}

export interface ActivityHandle {
  readonly id: string;
  progress(done: number, total?: number | null): Promise<void>;
  /** Status text key (e.g. "Verifying"); `null` clears it. */
  status(key: string | null): Promise<void>;
  /** Runs when the user cancels (needs `cancellable: true` in the spec). */
  onCancel(callback: () => void): void;
  succeed(): Promise<void>;
  fail(code: string, message: string): Promise<void>;
  /** Ends the activity as "cancelled" (no error text) when the owner itself
   * cancels the work. `onCancel` does not run for this call. Rejects with
   * `activity_not_cancellable` for an activity started without
   * `cancellable: true`; the handle then stays usable. */
  cancel(): Promise<void>;
}

/** Starts an activity in the activity center and returns its handle. */
export async function startActivity(spec: StartActivitySpec): Promise<ActivityHandle> {
  const full: ActivitySpec = {
    kind: spec.kind,
    key: spec.key ?? "",
    titleKey: spec.titleKey,
    titleParams: spec.titleParams ?? {},
    cancellable: spec.cancellable ?? false,
    total: spec.total ?? null,
    resume: spec.resume ?? null,
  };
  // Listen before the command: a cancel arriving before it returns is buffered.
  const watcher = watchActivityEvents();
  try {
    await watcher.ready;
    return makeHandle(await startActivityCommand(full), watcher);
  } catch (err) {
    watcher.stop();
    throw err;
  }
}

/** Re-attaches to the oldest running activity with this kind and key (e.g. after
 * a webview reload) and returns an owner handle for it, or `null` if none runs.
 * If the user cancelled it in the meantime (no owner), it is no longer running
 * and `null` is returned. */
export async function adoptActivity(kind: string, key = ""): Promise<ActivityHandle | null> {
  // Listen before the list call: a cancel arriving after the read is buffered.
  const watcher = watchActivityEvents();
  try {
    await watcher.ready;
    const match = (await listActivities())
      .filter((a) => a.state === "running" && a.kind === kind && a.key === key)
      .sort((a, b) => a.startedAt - b.startedAt)[0];
    if (!match) {
      watcher.stop();
      return null;
    }
    return makeHandle(match.id, watcher);
  } catch (err) {
    watcher.stop();
    throw err;
  }
}

/** Subscribes to `activity-changed` right away and buffers the events until
 * `bind` supplies the handler (the id is not known before the start/list call
 * returns); buffered events are then replayed in order. */
function watchActivityEvents() {
  let handler: ((a: Activity) => void) | null = null;
  const buffer: Activity[] = [];
  let stopped = false;
  let unlisten: UnlistenFn | null = null;
  const ready = onActivityChanged((a) => {
    if (handler) handler(a);
    else buffer.push(a);
  })
    .then((un) => {
      if (stopped) un();
      else unlisten = un;
    })
    .catch((err) => console.warn("activity-changed listener failed:", err));
  return {
    /** Resolves once the registration finished (or failed, which only warns). */
    ready,
    bind(h: (a: Activity) => void) {
      handler = h;
      for (const a of buffer.splice(0)) h(a);
    },
    stop() {
      stopped = true;
      unlisten?.();
      unlisten = null;
    },
  };
}

/** Whether the activity is no longer running. The backend lists an entry until
 * it is dismissed, and dismiss refuses running activities, so a missing entry
 * means it ended. A failed query counts as "not ended". */
async function hasEnded(id: string): Promise<boolean> {
  try {
    const a = (await listActivities()).find((x) => x.id === id);
    return !a || a.state !== "running";
  } catch {
    return false;
  }
}

function makeHandle(id: string, watcher: ReturnType<typeof watchActivityEvents>): ActivityHandle {
  let cancelCallback: (() => void) | null = null;
  let cancelPending = false; // cancelled before `onCancel` was registered
  let done = false;
  let terminalSeen = false;

  const release = () => watcher.stop();
  watcher.bind((a) => {
    if (a.id !== id) return;
    if (a.state === "cancelled" && !done) {
      done = true;
      if (cancelCallback) cancelCallback();
      else cancelPending = true;
    }
    if (a.state !== "running") {
      terminalSeen = true;
      release();
    }
  });

  return {
    id,
    progress: async (d, total) => {
      if (done) return;
      await updateActivityCommand(id, { done: d, total: total ?? null, statusKey: null });
    },
    status: async (key) => {
      if (done) return;
      await updateActivityCommand(id, { done: null, total: null, statusKey: key ?? "" });
    },
    onCancel: (cb) => {
      if (cancelPending) {
        cancelPending = false;
        cb();
        return;
      }
      cancelCallback = cb;
    },
    succeed: async () => {
      if (done) return;
      done = true;
      release();
      await finishActivityCommand(id, { outcome: "succeeded" });
    },
    fail: async (code, message) => {
      if (done) return;
      done = true;
      release();
      await finishActivityCommand(id, { outcome: "failed", code, message });
    },
    cancel: async () => {
      if (done) return;
      done = true; // before the command, so the listener skips `onCancel`
      try {
        await cancelActivity(id);
      } catch (err) {
        // Refused (e.g. not cancellable): still running, unless the activity
        // already ended meanwhile (e.g. cancelled from the activity center).
        // The terminal event may not have arrived yet, so ask the backend.
        if (!terminalSeen && !(await hasEnded(id)) && !terminalSeen) done = false;
        throw err;
      }
      release();
    },
  };
}
