// Helpers for the connecting tab (issue #548).
import type { ConnectProgressEvent, ConnectStepSnapshot } from "../../src/types";
import type { AppDriver } from "./app";
import { expect } from "./app";

export function snapshotStep(
  kind: ConnectStepSnapshot["kind"],
  status: ConnectStepSnapshot["status"],
  detail: ConnectStepSnapshot["detail"] = null,
  errorCode: string | null = null,
): ConnectStepSnapshot {
  return { kind, hopIndex: 0, hop: "deploy@h.example.test:22", status, errorCode, detail };
}

/** The attempt id of the latest `connect` call, once it was made. */
export async function lastAttemptId(app: AppDriver): Promise<string> {
  await expect.poll(async () => (await app.calls("connect")).length).toBeGreaterThan(0);
  const calls = await app.calls("connect");
  return calls[calls.length - 1].args.attemptId as string;
}

export async function emitProgress(
  app: AppDriver,
  steps: ConnectStepSnapshot[],
  serverId = "s-1",
): Promise<void> {
  const event: ConnectProgressEvent = {
    attemptId: await lastAttemptId(app),
    serverId,
    attempt: 1,
    steps,
  };
  await app.emit("connect-progress", event);
}
