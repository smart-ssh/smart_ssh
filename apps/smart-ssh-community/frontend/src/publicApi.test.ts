// Issue #886: guards the public entry point (`smart-ssh/public-api`).
// Counter-check: remove or rename any name in EXPECTED (or its re-export in
// public-api.ts) and "exports every listed name" fails.
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
import { beforeEach, describe, expect, it, vi } from "vitest";

const invokeMock = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));

import * as publicApi from "./public-api";
import * as testSupport from "./test-support";
import { subscribeFeatureLocked } from "./extensions/featureLockedBus";

type Kind = "function" | "object";
const fn: Kind = "function";
const obj: Kind = "object";

const EXPECTED: Record<string, Kind> = {
  // extensions/index.ts
  registerSettingsSection: fn, registerDocumentAction: fn, listSettingsSections: fn,
  listSettingsSectionsForCategory: fn, unregisterSettingsSection: fn, listDocumentActions: fn,
  unregisterDocumentAction: fn, subscribeDocumentActions: fn, registerFirstRunNoticeExtension: fn,
  listFirstRunNoticeExtensions: fn, unregisterFirstRunNoticeExtension: fn,
  subscribeFirstRunNoticeExtensions: fn, registerServerSettingsTab: fn,
  unregisterServerSettingsTab: fn, listServerSettingsTabs: fn, registerComposerAction: fn,
  registerComposerDropHandler: fn, unregisterComposerDropHandler: fn, subscribeSettingsSections: fn,
  subscribeComposer: fn, registerServerFormSection: fn, registerGroupFormSection: fn,
  registerManageView: fn, registerSessionHeaderItem: fn, unregisterSessionHeaderItem: fn,
  listSessionHeaderItems: fn, listServerFormSections: fn, listGroupFormSections: fn,
  listManageViews: fn, subscribeFormSlots: fn, resetRegistryForTests: fn,
  requestResumeChatSession: fn, useEntitlements: fn, FREE_TIER: "string" as Kind, registerTier: fn,
  tierLabel: fn, publishFeatureLocked: fn, subscribeFeatureLocked: fn,
  FeatureLockedDialog: fn, ModalBackdrop: fn,
  // bootstrap, ui
  bootstrap: fn, ErrorBoundary: fn, Button: obj, DialogFrame: fn, Field: fn, FIELD_CONTROL_CLASS: "string" as Kind,
  Spinner: fn, TabPanel: fn, Tabs: fn, tabId: fn, useTabsIdBase: fn, Tooltip: fn,
  // api and helpers
  invokeCommand: fn, commandErrorMessage: fn, commandErrorCode: fn, commandErrorFeatureLocked: fn,
  commandErrorConnectLog: fn, listServers: fn, getStartupState: fn,
  TECHNICAL_INPUT_PROPS: obj, useIsNarrow: fn, resolveSectionLabel: fn, pickAndReadTextFile: fn,
  SUPPORTED_LANGUAGES: obj, settingsStore: fn, initI18n: fn,
};

describe("public-api", () => {
  it("exports every listed name", () => {
    const mod = publicApi as Record<string, unknown>;
    const wrong = Object.entries(EXPECTED).filter(([name, kind]) => {
      const v = mod[name];
      return v === undefined || typeof v !== kind;
    });
    expect(wrong.map(([n]) => n)).toEqual([]);
  });

  it("test-support exports the matchMedia helpers", () => {
    expect(typeof testSupport.installMatchMediaMock).toBe("function");
    expect(typeof testSupport.setWide).toBe("function");
  });

  it("resolves by package name through the exports map", () => {
    const req = createRequire(fileURLToPath(import.meta.url));
    expect(req.resolve("smart-ssh/public-api")).toMatch(/src[\\/]public-api\.ts$/);
    expect(req.resolve("smart-ssh/test-support")).toMatch(/src[\\/]test-support\.ts$/);
    expect(() => req.resolve("smart-ssh/src/api")).toThrow();
  });
});

describe("invokeCommand", () => {
  beforeEach(() => invokeMock.mockReset());

  it("publishes feature_locked and rethrows the same error", async () => {
    const payload = { feature: "x", tier: "pro" };
    const err = { message: "locked", feature_locked: payload };
    invokeMock.mockImplementation((cmd: string) =>
      cmd === "cmd" ? Promise.reject(err) : Promise.resolve(undefined),
    );
    const seen: unknown[] = [];
    const off = subscribeFeatureLocked((p) => seen.push(p));
    const got = await publicApi.invokeCommand("cmd", { a: 1 }).catch((e: unknown) => e);
    expect(got === err).toBe(true);
    off();
    expect(seen).toEqual([payload]);
    expect(invokeMock).toHaveBeenCalledWith("cmd", { a: 1 });
  });

  it("publishes nothing for other rejections", async () => {
    const err = { message: "boom", code: "E" };
    invokeMock.mockImplementation((cmd: string) =>
      cmd === "cmd" ? Promise.reject(err) : Promise.resolve(undefined),
    );
    const seen: unknown[] = [];
    const off = subscribeFeatureLocked((p) => seen.push(p));
    const got = await publicApi.invokeCommand("cmd").catch((e: unknown) => e);
    expect(got === err).toBe(true);
    off();
    expect(seen).toEqual([]);
  });
});
