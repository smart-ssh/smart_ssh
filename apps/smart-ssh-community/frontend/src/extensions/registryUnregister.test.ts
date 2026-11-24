import { afterEach, describe, expect, it, vi } from "vitest";
import {
  listComposerActions,
  listManageViews,
  registerComposerAction,
  registerManageView,
  resetRegistryForTests,
  subscribeComposer,
  subscribeFormSlots,
  unregisterComposerAction,
  unregisterManageView,
} from "./registry";

function Noop() {
  return null;
}
const view = { id: "v", label: "V", component: Noop };
const action = { id: "a", label: "A", icon: null, onInvoke: () => {} };

describe("manage view / composer action removal", () => {
  afterEach(() => resetRegistryForTests());

  it("register and list work without any unregister call", () => {
    registerManageView(view);
    registerComposerAction(action);
    expect(listManageViews().map((v) => v.id)).toEqual(["v"]);
    expect(listComposerActions().map((a) => a.id)).toEqual(["a"]);
  });

  it("unregisterManageView removes the view and notifies; unknown id is a no-op", () => {
    registerManageView(view);
    const l = vi.fn();
    subscribeFormSlots(l);
    unregisterManageView("missing");
    expect(l).not.toHaveBeenCalled();
    const before = listManageViews();
    unregisterManageView("v");
    expect(l).toHaveBeenCalledTimes(1);
    expect(listManageViews()).toEqual([]);
    expect(before).toHaveLength(1);
  });

  it("unregisterComposerAction removes the action and notifies; unknown id is a no-op", () => {
    registerComposerAction(action);
    const l = vi.fn();
    subscribeComposer(l);
    unregisterComposerAction("missing");
    expect(l).not.toHaveBeenCalled();
    unregisterComposerAction("a");
    expect(l).toHaveBeenCalledTimes(1);
    expect(listComposerActions()).toEqual([]);
  });

  it("register, unregister, register again yields one entry; list ref is stable", () => {
    registerManageView(view);
    unregisterManageView("v");
    registerManageView(view);
    registerComposerAction(action);
    unregisterComposerAction("a");
    registerComposerAction(action);
    expect(listManageViews()).toHaveLength(1);
    expect(listComposerActions()).toHaveLength(1);
    expect(listManageViews()).toBe(listManageViews());
    expect(listComposerActions()).toBe(listComposerActions());
  });
});
