// Issue #1325, Spec 0116: public API for activities whose work runs in the
// frontend. Same operations as the Rust `ActivityHandle`; exported from
// `public-api.ts`.
import type { UnlistenFn } from "@tauri-apps/api/event";
import {
  finishActivityCommand,
  startActivityCommand,
  updateActivityCommand,
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
  const id = await startActivityCommand(full);
  let cancelCallback: (() => void) | null = null;
  let done = false;
  let unlisten: UnlistenFn | null = null;
  let released = false;

  const release = () => {
    released = true;
    unlisten?.();
    unlisten = null;
  };
  onActivityChanged((a) => {
    if (a.id !== id) return;
    if (a.state === "cancelled" && !done) {
      done = true;
      cancelCallback?.();
    }
    if (a.state !== "running") release();
  })
    .then((un) => {
      if (released) un();
      else unlisten = un;
    })
    .catch((err) => console.warn("activity-changed listener failed:", err));

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
  };
}
