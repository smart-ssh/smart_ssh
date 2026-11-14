// Issue #1327: activities a quit interrupted, listed once per window at
// start. Entries whose kind has no handler are removed by the backend.
import { useSyncExternalStore } from "react";
import { listInterrupted, resolveInterrupted, type InterruptedRow } from "../api/activitiesApi";
import { activityResumeHandler, activityResumeKinds } from "../lib/activityResume";

type Listener = () => void;

let rows: InterruptedRow[] = [];
let loaded = false;
const listeners = new Set<Listener>();

function set(next: InterruptedRow[]) {
  rows = next;
  for (const l of listeners) l();
}

function load() {
  if (loaded) return;
  loaded = true;
  listInterrupted(activityResumeKinds())
    .then(set)
    .catch((err) => {
      loaded = false;
      console.warn("activities_interrupted failed:", err);
    });
}

function subscribe(listener: Listener) {
  listeners.add(listener);
  load();
  return () => {
    listeners.delete(listener);
  };
}

const getSnapshot = () => rows;

export function useInterruptedActivities(): InterruptedRow[] {
  return useSyncExternalStore(subscribe, getSnapshot, getSnapshot);
}

/** Starts a fresh activity for the entry through its handler; the row goes away. */
export async function resumeInterrupted(row: InterruptedRow): Promise<void> {
  if (row.native) {
    await resolveInterrupted(row.id, "resume");
  } else {
    const handler = activityResumeHandler(row.kind);
    if (!handler) throw new Error(`no resume handler for ${row.kind}`);
    await handler.resume(row);
    await resolveInterrupted(row.id, "remove");
  }
  set(rows.filter((r) => r.id !== row.id));
}

/** Runs the discard handler (if any) and removes the entry. */
export async function discardInterrupted(row: InterruptedRow): Promise<void> {
  if (row.native) {
    await resolveInterrupted(row.id, "discard");
  } else {
    await activityResumeHandler(row.kind)?.discard?.(row);
    await resolveInterrupted(row.id, "remove");
  }
  set(rows.filter((r) => r.id !== row.id));
}

/** For tests: forget everything. */
export function resetInterruptedForTests() {
  rows = [];
  loaded = false;
  listeners.clear();
}
