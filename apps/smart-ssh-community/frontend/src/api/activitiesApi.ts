// Issue #1325, Spec 0116: IPC wrappers for the activity center.
import { invokeCommand } from "./api";
import type {
  Activity,
  ActivityOutcome,
  ActivitySpec,
  ActivityUpdate,
  InterruptedActivity,
} from "../generated/ipc";

export type {
  Activity,
  ActivityOutcome,
  ActivitySpec,
  ActivityState,
  ActivityUpdate,
  InterruptedActivity,
  ResumeSpec,
} from "../generated/ipc";

/** A stored interrupted activity; `native`: a Rust handler owns its kind. */
export type InterruptedRow = InterruptedActivity & { native: boolean };

export const listActivities = () => invokeCommand<Activity[]>("activities_list");

export const cancelActivity = (id: string) => invokeCommand<null>("activity_cancel", { id });

export const dismissActivity = (id: string) => invokeCommand<null>("activity_dismiss", { id });

export const startActivityCommand = (spec: ActivitySpec) =>
  invokeCommand<string>("activity_start", { spec });

export const updateActivityCommand = (id: string, update: ActivityUpdate) =>
  invokeCommand<null>("activity_update", { id, update });

export const finishActivityCommand = (id: string, outcome: ActivityOutcome) =>
  invokeCommand<null>("activity_finish", { id, outcome });

// Issue #1327: quitting and resuming.
export const confirmQuit = () => invokeCommand<null>("quit_confirmed");

/** Stored entries whose kind has a handler (Rust, or one of `frontendKinds`);
 * the others are removed by the backend. */
export const listInterrupted = (frontendKinds: string[]) =>
  invokeCommand<InterruptedRow[]>("activities_interrupted", { frontendKinds });

export type InterruptedAction = "resume" | "discard" | "remove";

/** `resume`/`discard`: a Rust handler runs and the entry is removed. `remove`:
 * the frontend handler already ran; only the entry is removed. */
export const resolveInterrupted = (id: string, action: InterruptedAction) =>
  invokeCommand<null>("activity_interrupted_resolve", { id, action });
