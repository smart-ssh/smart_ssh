import { describe, expect, it, afterEach, vi } from "vitest";
import {
  listDocumentActions,
  listFirstRunNoticeExtensions,
  registerFirstRunNoticeExtension,
  listSettingsSections,
  registerDocumentAction,
  registerSettingsSection,
  resetRegistryForTests,
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
});
