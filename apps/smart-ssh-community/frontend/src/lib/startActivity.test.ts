import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Activity } from "../api/activitiesApi";
import { adoptActivity, startActivity } from "./startActivity";

const api = vi.hoisted(() => ({
  start: vi.fn(),
  list: vi.fn(),
  update: vi.fn(),
  finish: vi.fn(),
  cancel: vi.fn(),
  handler: null as null | ((a: Activity) => void),
  gate: null as null | Promise<void>,
}));

vi.mock("../api/activitiesApi", () => ({
  listActivities: () => api.list(),
  startActivityCommand: (s: unknown) => api.start(s),
  updateActivityCommand: (...a: unknown[]) => api.update(...a),
  cancelActivity: (id: string) => api.cancel(id),
  finishActivityCommand: (...a: unknown[]) => api.finish(...a),
}));
vi.mock("../api/events", () => ({
  onActivityChanged: (h: (a: Activity) => void) => {
    api.handler = h;
    return (api.gate ?? Promise.resolve()).then(() => () => {
      api.handler = null;
    });
  },
}));

const spec = { kind: "k", key: "m", titleKey: "activities.title", cancellable: true };

describe("startActivity", () => {
  beforeEach(() => {
    api.list.mockReset().mockResolvedValue([]);
    api.start.mockReset().mockResolvedValue("id1");
    api.update.mockReset().mockResolvedValue(null);
    api.finish.mockReset().mockResolvedValue(null);
    api.cancel.mockReset().mockResolvedValue(null);
    api.handler = null;
    api.gate = null;
  });

  it("does not call start or list before the listener registration resolved", async () => {
    let open!: () => void;
    api.gate = new Promise<void>((r) => (open = r));
    const p1 = startActivity(spec);
    const p2 = adoptActivity("k", "m");
    await Promise.resolve();
    await Promise.resolve();
    expect(api.start).not.toHaveBeenCalled();
    expect(api.list).not.toHaveBeenCalled();
    open();
    await p1;
    await p2;
    expect(api.start).toHaveBeenCalledTimes(1);
    expect(api.list).toHaveBeenCalledTimes(1);
  });

  it("forwards a resume spec to the backend (issue #1327)", async () => {
    await startActivity({ ...spec, resume: { kind: "k", key: "m" } });
    expect(api.start).toHaveBeenCalledWith(expect.objectContaining({ resume: { kind: "k", key: "m" } }));
  });

  it("starts with defaults and forwards progress, status and success", async () => {
    const h = await startActivity({ ...spec, cancellable: undefined });
    expect(api.start).toHaveBeenCalledWith({
      kind: "k",
      key: "m",
      titleKey: "activities.title",
      titleParams: {},
      cancellable: false,
      total: null,
      resume: null,
    });
    await h.progress(5, 10);
    expect(api.update).toHaveBeenCalledWith("id1", { done: 5, total: 10, statusKey: null });
    await h.status("activities.inProgress");
    expect(api.update).toHaveBeenCalledWith("id1", {
      done: null,
      total: null,
      statusKey: "activities.inProgress",
    });
    await h.status(null);
    expect(api.update).toHaveBeenLastCalledWith("id1", { done: null, total: null, statusKey: "" });
    await h.succeed();
    expect(api.finish).toHaveBeenCalledWith("id1", { outcome: "succeeded" });
  });

  it("reports failure with code and message", async () => {
    const h = await startActivity(spec);
    await h.fail("E_X", "boom");
    expect(api.finish).toHaveBeenCalledWith("id1", { outcome: "failed", code: "E_X", message: "boom" });
  });

  it("runs the cancel callback when the activity is cancelled and ignores later calls", async () => {
    const h = await startActivity(spec);
    const cb = vi.fn();
    h.onCancel(cb);
    await vi.waitFor(() => expect(api.handler).not.toBeNull());
    api.handler?.({ id: "other", state: "cancelled" } as Activity);
    expect(cb).not.toHaveBeenCalled();
    api.handler?.({ id: "id1", state: "cancelled" } as Activity);
    expect(cb).toHaveBeenCalledTimes(1);
    await h.succeed();
    await h.progress(1, 2);
    expect(api.finish).not.toHaveBeenCalled();
    expect(api.update).not.toHaveBeenCalled();
  });

  it("cancel() ends the activity as cancelled without onCancel, then ignores later calls", async () => {
    const h = await startActivity(spec);
    const cb = vi.fn();
    h.onCancel(cb);
    await vi.waitFor(() => expect(api.handler).not.toBeNull());
    api.cancel.mockImplementation(async () => {
      api.handler?.({ id: "id1", state: "cancelled" } as Activity);
    });
    await h.cancel();
    expect(api.cancel).toHaveBeenCalledWith("id1");
    expect(api.finish).not.toHaveBeenCalled();
    expect(cb).not.toHaveBeenCalled();
    await h.progress(1);
    await h.status("x");
    await h.succeed();
    await h.fail("E", "m");
    await h.cancel();
    expect(api.update).not.toHaveBeenCalled();
    expect(api.finish).not.toHaveBeenCalled();
    expect(api.cancel).toHaveBeenCalledTimes(1);
  });

  it("cancel() after succeed() or fail() does nothing", async () => {
    const h = await startActivity(spec);
    await h.succeed();
    await h.cancel();
    const h2 = await startActivity(spec);
    await h2.fail("E", "m");
    await h2.cancel();
    expect(api.cancel).not.toHaveBeenCalled();
  });

  it("cancel() surfaces the refusal for a non-cancellable activity and the handle stays usable", async () => {
    api.cancel.mockRejectedValue(new Error("activity_not_cancellable"));
    const h = await startActivity({ ...spec, cancellable: false });
    await expect(h.cancel()).rejects.toThrow("activity_not_cancellable");
    await h.succeed();
    expect(api.finish).toHaveBeenCalledWith("id1", { outcome: "succeeded" });
  });

  it("keeps the handle done when the center cancelled and the rejection arrives before the terminal event", async () => {
    api.cancel.mockRejectedValue(new Error("activity_not_cancellable"));
    api.list.mockResolvedValue([{ id: "id1", state: "cancelled" }]);
    const h = await startActivity(spec);
    const onCancel = vi.fn();
    h.onCancel(onCancel);
    await expect(h.cancel()).rejects.toThrow("activity_not_cancellable");
    api.handler?.({ id: "id1", state: "cancelled" } as Activity);
    await h.progress(1);
    await h.status("x");
    await h.succeed();
    await h.fail("c", "m");
    await h.cancel();
    expect(onCancel).not.toHaveBeenCalled();
    expect(api.update).not.toHaveBeenCalled();
    expect(api.finish).not.toHaveBeenCalled();
    expect(api.cancel).toHaveBeenCalledTimes(1);
  });

  it("keeps the handle done when the terminal event arrives before the rejection", async () => {
    const h = await startActivity(spec);
    const onCancel = vi.fn();
    h.onCancel(onCancel);
    api.cancel.mockImplementation(async () => {
      api.handler?.({ id: "id1", state: "cancelled" } as Activity);
      throw new Error("activity_not_cancellable");
    });
    await expect(h.cancel()).rejects.toThrow("activity_not_cancellable");
    await h.progress(1);
    await h.succeed();
    await h.cancel();
    expect(onCancel).not.toHaveBeenCalled();
    expect(api.update).not.toHaveBeenCalled();
    expect(api.finish).not.toHaveBeenCalled();
    expect(api.cancel).toHaveBeenCalledTimes(1);
  });

  it("resets done after a refusal for a still-running activity (re-query says running)", async () => {
    api.cancel.mockRejectedValue(new Error("activity_not_cancellable"));
    api.list.mockResolvedValue([{ id: "id1", state: "running" }]);
    const h = await startActivity(spec);
    await expect(h.cancel()).rejects.toThrow("activity_not_cancellable");
    await h.succeed();
    expect(api.finish).toHaveBeenCalledWith("id1", { outcome: "succeeded" });
  });
});

const act = (o: Partial<Activity>): Activity => ({
  id: "a",
  kind: "k",
  key: "m",
  titleKey: "t",
  titleParams: {},
  state: "running",
  done: 0,
  total: null,
  statusKey: null,
  errorCode: null,
  errorMessage: null,
  cancellable: true,
  resumable: false,
  startedAt: 100,
  dismissed: false,
  ...o,
});

describe("adoptActivity (issue #1339)", () => {
  beforeEach(() => {
    api.list.mockReset().mockResolvedValue([]);
    api.start.mockReset().mockResolvedValue("new");
    api.update.mockReset().mockResolvedValue(null);
    api.finish.mockReset().mockResolvedValue(null);
    api.handler = null;
  });

  it("returns null for none, finished, and different key/kind", async () => {
    expect(await adoptActivity("k", "m")).toBeNull();
    api.list.mockResolvedValue([
      act({ state: "succeeded" }),
      act({ id: "b", state: "cancelled" }),
      act({ id: "c", key: "other" }),
      act({ id: "d", kind: "other" }),
    ]);
    expect(await adoptActivity("k", "m")).toBeNull();
  });

  it("defaults the key to the empty string", async () => {
    api.list.mockResolvedValue([act({ id: "e", key: "" })]);
    expect((await adoptActivity("k"))?.id).toBe("e");
  });

  it("acts on the existing id and creates no second activity", async () => {
    api.list.mockResolvedValue([act({ id: "x1" })]);
    const h = await adoptActivity("k", "m");
    expect(h?.id).toBe("x1");
    await h!.progress(3, 4);
    expect(api.update).toHaveBeenCalledWith("x1", { done: 3, total: 4, statusKey: null });
    await h!.status("s");
    expect(api.update).toHaveBeenCalledWith("x1", { done: null, total: null, statusKey: "s" });
    await h!.succeed();
    expect(api.finish).toHaveBeenCalledWith("x1", { outcome: "succeeded" });
    expect(api.start).not.toHaveBeenCalled();
  });

  it("fail finishes the existing activity", async () => {
    api.list.mockResolvedValue([act({ id: "x1" })]);
    const h = await adoptActivity("k", "m");
    await h!.fail("c", "msg");
    expect(api.finish).toHaveBeenCalledWith("x1", { outcome: "failed", code: "c", message: "msg" });
    expect(api.start).not.toHaveBeenCalled();
  });

  it("runs the cancel callback exactly once", async () => {
    api.list.mockResolvedValue([act({ id: "x1" })]);
    const h = await adoptActivity("k", "m");
    const cb = vi.fn();
    h!.onCancel(cb);
    await vi.waitFor(() => expect(api.handler).not.toBeNull());
    const handler = api.handler!;
    handler(act({ id: "other", state: "cancelled" }));
    expect(cb).not.toHaveBeenCalled();
    handler(act({ id: "x1", state: "cancelled" }));
    handler(act({ id: "x1", state: "cancelled" }));
    expect(cb).toHaveBeenCalledTimes(1);
  });

  it("adopts the oldest running match", async () => {
    api.list.mockResolvedValue([
      act({ id: "late", startedAt: 300 }),
      act({ id: "old", startedAt: 100 }),
      act({ id: "mid", startedAt: 200 }),
    ]);
    expect((await adoptActivity("k", "m"))?.id).toBe("old");
  });

  it("runs onCancel once for a cancel delivered between list and listener registration (issue #1343)", async () => {
    api.list.mockImplementation(async () => {
      const list = [act({ id: "x1" })];
      // The cancel arrives right after the read, before `adoptActivity` returns.
      queueMicrotask(() => api.handler?.(act({ id: "x1", state: "cancelled" })));
      return list;
    });
    const h = await adoptActivity("k", "m");
    const cb = vi.fn();
    h!.onCancel(cb);
    expect(cb).toHaveBeenCalledTimes(1);
    await h!.progress(1, 2);
    expect(api.update).not.toHaveBeenCalled();
    expect(cb).toHaveBeenCalledTimes(1);
  });
});

describe("startActivity race (issue #1343)", () => {
  beforeEach(() => {
    api.list.mockReset().mockResolvedValue([]);
    api.start.mockReset();
    api.update.mockReset().mockResolvedValue(null);
    api.finish.mockReset().mockResolvedValue(null);
    api.handler = null;
  });

  it("runs onCancel once for a cancel delivered before activity_start returns", async () => {
    api.start.mockImplementation(async () => {
      api.handler?.(act({ id: "id1", state: "cancelled" }));
      return "id1";
    });
    const h = await startActivity(spec);
    const cb = vi.fn();
    h.onCancel(cb);
    expect(cb).toHaveBeenCalledTimes(1);
    await h.progress(1);
    expect(api.update).not.toHaveBeenCalled();
  });

  it("does not call onCancel when the activity ended as succeeded or failed in the gap", async () => {
    for (const state of ["succeeded", "failed"] as const) {
      api.start.mockImplementation(async () => {
        api.handler?.(act({ id: "id1", state }));
        return "id1";
      });
      const h = await startActivity(spec);
      const cb = vi.fn();
      h.onCancel(cb);
      expect(cb).not.toHaveBeenCalled();
    }
  });

  it("stops listening when activity_start fails", async () => {
    api.start.mockRejectedValue(new Error("boom"));
    await expect(startActivity(spec)).rejects.toThrow("boom");
    await vi.waitFor(() => expect(api.handler).toBeNull());
  });
});
