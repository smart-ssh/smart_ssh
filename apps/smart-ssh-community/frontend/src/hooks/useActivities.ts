// Issue #1325, Spec 0116: the activity store. One shared store per window;
// it loads `activities_list` when the first component subscribes and applies
// `activity-changed` events, so a remounted UI or a reopened dialog sees what
// is already running.
import { useSyncExternalStore } from "react";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { listActivities, type Activity } from "../api/activitiesApi";
import { onActivityChanged } from "../api/events";

type Listener = () => void;

let activities: Activity[] = [];
/** Ids that an event touched; a late `activities_list` answer never overwrites them. */
let touched = new Set<string>();
const listeners = new Set<Listener>();
let unlisten: UnlistenFn | null = null;
let generation = 0;

function emitChange() {
  for (const l of listeners) l();
}

function byStart(a: Activity, b: Activity) {
  return a.startedAt - b.startedAt;
}

function apply(activity: Activity) {
  touched.add(activity.id);
  const rest = activities.filter((a) => a.id !== activity.id);
  // Array.prototype.sort is stable: equal start times keep arrival order.
  activities = activity.dismissed ? rest : [...rest, activity].sort(byStart);
  emitChange();
}

function start() {
  const gen = ++generation;
  onActivityChanged(apply)
    .then((un) => {
      if (gen === generation) unlisten = un;
      else un();
    })
    .catch((err) => console.warn("activity-changed listener failed:", err));
  listActivities()
    .then((list) => {
      if (gen !== generation) return;
      const fresh = list.filter((a) => !touched.has(a.id));
      if (fresh.length === 0) return;
      activities = [...activities, ...fresh].sort(byStart);
      emitChange();
    })
    .catch((err) => console.warn("activities_list failed:", err));
}

function stop() {
  generation++;
  unlisten?.();
  unlisten = null;
  activities = [];
  touched = new Set();
}

function subscribe(listener: Listener) {
  if (listeners.size === 0) start();
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
    if (listeners.size === 0) stop();
  };
}

const getSnapshot = () => activities;

/** All activities in start order. */
export function useActivities(): Activity[] {
  return useSyncExternalStore(subscribe, getSnapshot, getSnapshot);
}

/** The running activity of `kind` and `key`, if any (so a reopened dialog can
 * show the running state instead of starting a second run). */
export function useRunningActivity(kind: string, key?: string): Activity | undefined {
  const all = useActivities();
  return all.find(
    (a) => a.state === "running" && a.kind === kind && (key === undefined || a.key === key),
  );
}

/** Stable helper for components that only need to know whether `kind`/`key` runs. */
export function useIsActivityRunning(kind: string, key?: string): boolean {
  return useRunningActivity(kind, key) !== undefined;
}

/** For tests: forget everything. */
export function resetActivitiesStoreForTests() {
  stop();
  listeners.clear();
}

