import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Activity } from "../api/activitiesApi";
import { adoptActivity, startActivity } from "./startActivity";

const api = vi.hoisted(() => ({
  start: vi.fn(),
  list: vi.fn(),
  update: vi.fn(),
  finish: vi.fn(),
  handler: null as null | ((a: Activity) => void),
}));

vi.mock("../api/activitiesApi", () => ({
  listActivities: () => api.list(),
  startActivityCommand: (s: unknown) => api.start(s),
  updateActivityCommand: (...a: unknown[]) => api.update(...a),
  finishActivityCommand: (...a: unknown[]) => api.finish(...a),
}));
vi.mock("../api/events", () => ({
  onActivityChanged: (h: (a: Activity) => void) => {
    api.handler = h;
    return Promise.resolve(() => {
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
    api.handler = null;
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
});
