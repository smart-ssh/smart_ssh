import { describe, expect, it, afterEach, vi } from "vitest";
import {
  listDocumentActions,
  subscribeDocumentActions,
  subscribeFirstRunNoticeExtensions,
  unregisterDocumentAction,
  unregisterFirstRunNoticeExtension,
  listFirstRunNoticeExtensions,
  registerFirstRunNoticeExtension,
  listSettingsSections,
  registerDocumentAction,
  registerSettingsSection,
  resetRegistryForTests,
  subscribeSettingsSections,
  listSettingsSectionsForCategory,
  unregisterSettingsSection,
} from "./registry";

function Noop() {
  return null;
}

describe("registry", () => {
  afterEach(() => {
    resetRegistryForTests();
  });

  it("returns registered settings sections", () => {
    registerSettingsSection({ id: "a", component: Noop });
    registerSettingsSection({ id: "b", component: Noop });

    expect(listSettingsSections().map((s) => s.id)).toEqual(["a", "b"]);
  });

  it("replaces a section registered again under the same id", () => {
    registerSettingsSection({ id: "a", component: Noop });
    function Other() {
      return null;
    }
    registerSettingsSection({ id: "a", component: Other });

    const sections = listSettingsSections();
    expect(sections).toHaveLength(1);
    expect(sections[0].component).toBe(Other);
  });

  // Spec 0045, Abschnitt 7.
  it("returns registered document actions", () => {
    const onInvokeA = vi.fn();
    const onInvokeB = vi.fn();
    registerDocumentAction({ id: "a", label: "Action A", onInvoke: onInvokeA });
    registerDocumentAction({ id: "b", label: "Action B", onInvoke: onInvokeB });

    expect(listDocumentActions().map((a) => a.id)).toEqual(["a", "b"]);
  });

  it("replaces a document action registered again under the same id", () => {
    const first = vi.fn();
    const second = vi.fn();
    registerDocumentAction({ id: "a", label: "First", onInvoke: first });
    registerDocumentAction({ id: "a", label: "Second", onInvoke: second });

    const actions = listDocumentActions();
    expect(actions).toHaveLength(1);
    expect(actions[0].label).toBe("Second");
    expect(actions[0].onInvoke).toBe(second);
  });

  it("preserves disabled/disabledReason on a registered document action", () => {
    registerDocumentAction({
      id: "a",
      label: "Locked",
      onInvoke: vi.fn(),
      disabled: true,
      disabledReason: "Requires a paid plan",
    });

    const [action] = listDocumentActions();
    expect(action.disabled).toBe(true);
    expect(action.disabledReason).toBe("Requires a paid plan");
  });

  // Spec 0031, Abschnitt 6 (issue #157).
  it("lists first-run notice extensions sorted by order, ties by id", () => {
    registerFirstRunNoticeExtension({ id: "late", order: 20, Component: Noop });
    registerFirstRunNoticeExtension({ id: "b-early", order: 10, Component: Noop });
    registerFirstRunNoticeExtension({ id: "a-early", order: 10, Component: Noop });

    expect(listFirstRunNoticeExtensions().map((e) => e.id)).toEqual([
      "a-early",
      "b-early",
      "late",
    ]);
  });

  it("replaces a first-run notice extension registered again under the same id, and resets", () => {
    function Other() {
      return null;
    }
    registerFirstRunNoticeExtension({ id: "x", order: 1, Component: Noop });
    registerFirstRunNoticeExtension({ id: "x", order: 2, Component: Other });

    const extensions = listFirstRunNoticeExtensions();
    expect(extensions).toHaveLength(1);
    expect(extensions[0].Component).toBe(Other);

    resetRegistryForTests();
    expect(listFirstRunNoticeExtensions()).toEqual([]);
  });

  it("keeps document action snapshot identity until a change and notifies listeners (#884)", () => {
    registerDocumentAction({ id: "a", label: "A", onInvoke: vi.fn() });
    const first = listDocumentActions();
    expect(listDocumentActions()).toBe(first);

    const listener = vi.fn();
    const unsubscribe = subscribeDocumentActions(listener);
    registerDocumentAction({ id: "b", label: "B", onInvoke: vi.fn() });
    expect(listener).toHaveBeenCalledTimes(1);
    expect(listDocumentActions()).not.toBe(first);
    unregisterDocumentAction("b");
    expect(listener).toHaveBeenCalledTimes(2);
    unregisterDocumentAction("missing");
    expect(listener).toHaveBeenCalledTimes(2);
    unsubscribe();
    unregisterDocumentAction("a");
    expect(listener).toHaveBeenCalledTimes(2);
  });

  it("keeps first-run notice snapshot identity until a change and notifies listeners (#884)", () => {
    registerFirstRunNoticeExtension({ id: "a", order: 1, Component: Noop });
    const first = listFirstRunNoticeExtensions();
    expect(listFirstRunNoticeExtensions()).toBe(first);

    const listener = vi.fn();
    const unsubscribe = subscribeFirstRunNoticeExtensions(listener);
    registerFirstRunNoticeExtension({ id: "a", order: 2, Component: Noop });
    expect(listener).toHaveBeenCalledTimes(1);
    expect(listFirstRunNoticeExtensions()).not.toBe(first);
    unregisterFirstRunNoticeExtension("a");
    expect(listFirstRunNoticeExtensions()).toEqual([]);
    expect(listener).toHaveBeenCalledTimes(2);
    unsubscribe();
  });

  describe("settings sections with a target category (#674)", () => {
    it("filters by category and sorts by order, ties by registration", () => {
      registerSettingsSection({ id: "top", component: Noop });
      registerSettingsSection({ id: "c", category: "data-backup", order: 5, component: Noop });
      registerSettingsSection({ id: "a", category: "data-backup", order: 1, component: Noop });
      registerSettingsSection({ id: "b", category: "data-backup", order: 5, component: Noop });
      registerSettingsSection({ id: "z", category: "other", component: Noop });
      expect(listSettingsSectionsForCategory("data-backup").map((s) => s.id)).toEqual(["a", "c", "b"]);
    });

    it("unregister removes the section and notifies; unknown id is silent", () => {
      registerSettingsSection({ id: "a", category: "data-backup", component: Noop });
      const listener = vi.fn();
      subscribeSettingsSections(listener);
      unregisterSettingsSection("nope");
      expect(listener).not.toHaveBeenCalled();
      unregisterSettingsSection("a");
      expect(listener).toHaveBeenCalledTimes(1);
      expect(listSettingsSectionsForCategory("data-backup")).toEqual([]);
    });
  });

  describe("settings section subscriptions (issue #243)", () => {
    it("notifies listeners after a registration and returns a new list", () => {
      const listener = vi.fn();
      subscribeSettingsSections(listener);
      const before = listSettingsSections();

      registerSettingsSection({ id: "a", component: Noop });

      expect(listener).toHaveBeenCalledTimes(1);
      expect(listSettingsSections()).not.toBe(before);
      expect(listSettingsSections().map((s) => s.id)).toEqual(["a"]);
    });

    it("returns a stable array reference between changes", () => {
      registerSettingsSection({ id: "a", component: Noop });
      expect(listSettingsSections()).toBe(listSettingsSections());
    });

    it("notifies when an existing id is replaced", () => {
      function Other() {
        return null;
      }
      registerSettingsSection({ id: "a", component: Noop });
      const listener = vi.fn();
      subscribeSettingsSections(listener);

      registerSettingsSection({ id: "a", component: Other });

      expect(listener).toHaveBeenCalledTimes(1);
      expect(listSettingsSections()[0].component).toBe(Other);
    });

    it("does not call an unsubscribed listener", () => {
      const listener = vi.fn();
      const unsubscribe = subscribeSettingsSections(listener);
      unsubscribe();

      registerSettingsSection({ id: "a", component: Noop });

      expect(listener).not.toHaveBeenCalled();
    });
  });
});
