// Spec 0092, A1.2/U3: die Einstellung „Bei rotem Risiko immer nachfragen"
// muss auf derselben Fail-safe-Regel wie das Backend beruhen
// (`app_shell::risk_second_opinion::red_risk_always_confirm_from_stored`,
// s. dortige Tests `test_missing_key_means_on`/`test_non_boolean_values_
// mean_on`): fehlt der Schlüssel oder ist der gespeicherte Wert kein
// boolescher Wert, gilt „an" — nur ein tatsächliches `false` schaltet ab.
// *Gegenbeweis* (im Bericht dokumentiert): mit `typeof value === "boolean"
// ? value : true` durch `value ?? true` ersetzt (das würde einen
// nicht-booleschen, aber "truthy" Wert wie den String `"false"` fälschlich
// als eingeschaltet UND einen nicht-booleschen falsy Wert nicht abfangen)
// schlägt der Test unten zu `"false"` fehl.
import { describe, expect, it, vi } from "vitest";

const storeGet = vi.fn();
const storeSet = vi.fn();
const storeSave = vi.fn();

const invokeMock = vi.fn();
vi.mock("./api", async () =>
  (await import("./test/apiMock")).createApiMock({
    setRiskSecondOpinionSettings: (enabled: boolean, providerId: string | null) =>
      invokeMock("set_risk_second_opinion_settings", { enabled, providerId }),
    setRedRiskAlwaysConfirm: (value: boolean) =>
      invokeMock("set_red_risk_always_confirm", { value }),
  }),
);

vi.mock("./i18n", () => ({
  settingsStore: () => Promise.resolve({ get: storeGet, set: storeSet, save: storeSave }),
}));

// Import erst NACH dem Mock, damit `settingsStore` bereits ersetzt ist.
const {
  loadRedRiskAlwaysConfirm,
  saveRedRiskAlwaysConfirm,
  saveRiskClassifierSettings,
} = await import("./riskSettings");

describe("loadRedRiskAlwaysConfirm (Spec 0092, A1.2)", () => {
  it("returns true when nothing is stored (missing key)", async () => {
    storeGet.mockResolvedValue(undefined);
    expect(await loadRedRiskAlwaysConfirm()).toBe(true);
  });

  it("returns true for an explicit boolean true", async () => {
    storeGet.mockResolvedValue(true);
    expect(await loadRedRiskAlwaysConfirm()).toBe(true);
  });

  it("returns false for an explicit boolean false — the only way to turn it off", async () => {
    storeGet.mockResolvedValue(false);
    expect(await loadRedRiskAlwaysConfirm()).toBe(false);
  });

  // T16/U3: derselbe adversariale Fall wie im Backend — ein String "false"
  // ist kein boolescher Wert und darf die Eskalation nicht still abschalten.
  it.each([["false"], ["true"], [0], [1], [null], [[]], [{}]])(
    "treats the non-boolean value %j as 'on' (fail-safe)",
    async (value) => {
      storeGet.mockResolvedValue(value);
      expect(await loadRedRiskAlwaysConfirm()).toBe(true);
    },
  );
});

describe("saving goes through backend commands (issue #856)", () => {
  it("saveRedRiskAlwaysConfirm invokes the command and never writes the store", async () => {
    invokeMock.mockClear();
    await saveRedRiskAlwaysConfirm(false);
    expect(invokeMock).toHaveBeenCalledWith("set_red_risk_always_confirm", { value: false });
    expect(storeSet).not.toHaveBeenCalled();
    expect(storeSave).not.toHaveBeenCalled();
  });

  it("saveRiskClassifierSettings invokes the command and never writes the store", async () => {
    invokeMock.mockClear();
    await saveRiskClassifierSettings({ enabled: true, providerId: "p-1" });
    expect(invokeMock).toHaveBeenCalledWith("set_risk_second_opinion_settings", {
      enabled: true,
      providerId: "p-1",
    });
    expect(storeSet).not.toHaveBeenCalled();
    expect(storeSave).not.toHaveBeenCalled();
  });

  it("propagates a backend rejection to the caller", async () => {
    invokeMock.mockRejectedValueOnce({ message: "x", code: "RISK_SETTINGS_PROVIDER_UNKNOWN" });
    await expect(
      saveRiskClassifierSettings({ enabled: true, providerId: "gone" }),
    ).rejects.toMatchObject({ code: "RISK_SETTINGS_PROVIDER_UNKNOWN" });
  });
});
