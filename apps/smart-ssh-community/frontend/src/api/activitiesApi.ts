// Issue #1325, Spec 0116: IPC wrappers for the activity center.
import { invokeCommand } from "./api";
import type { Activity, ActivityOutcome, ActivitySpec, ActivityUpdate } from "../generated/ipc";

export type { Activity, ActivityOutcome, ActivitySpec, ActivityState, ActivityUpdate } from "../generated/ipc";

export const listActivities = () => invokeCommand<Activity[]>("activities_list");

export const cancelActivity = (id: string) => invokeCommand<null>("activity_cancel", { id });

export const dismissActivity = (id: string) => invokeCommand<null>("activity_dismiss", { id });

export const startActivityCommand = (spec: ActivitySpec) =>
  invokeCommand<string>("activity_start", { spec });

export const updateActivityCommand = (id: string, update: ActivityUpdate) =>
  invokeCommand<null>("activity_update", { id, update });

export const finishActivityCommand = (id: string, outcome: ActivityOutcome) =>
  invokeCommand<null>("activity_finish", { id, outcome });
