import { describe, expect, it, afterEach } from "vitest";
import {
  listSettingsSections,
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
});
