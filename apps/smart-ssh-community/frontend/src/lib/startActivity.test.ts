import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Activity } from "../api/activitiesApi";
import { startActivity } from "./startActivity";

const api = vi.hoisted(() => ({
  start: vi.fn(),
  update: vi.fn(),
  finish: vi.fn(),
  cancel: vi.fn(),
  handler: null as null | ((a: Activity) => void),
}));

vi.mock("../api/activitiesApi", () => ({
  startActivityCommand: (s: unknown) => api.start(s),
  updateActivityCommand: (...a: unknown[]) => api.update(...a),
  cancelActivity: (id: string) => api.cancel(id),
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
    api.start.mockReset().mockResolvedValue("id1");
    api.update.mockReset().mockResolvedValue(null);
    api.finish.mockReset().mockResolvedValue(null);
    api.cancel.mockReset().mockResolvedValue(null);
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
});
