import { act, render } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { afterEach, describe, expect, it } from "vitest";
import { testI18n } from "../testI18n";
import { FeatureLockedDialog } from "./FeatureLockedDialog";
import { publishFeatureLocked } from "./featureLockedBus";
import { registerTier, resetTiersForTests, tierLabel } from "./tiers";
import type { Entitlements } from "./entitlements";

afterEach(() => resetTiersForTests());

const t = (k: string) => `T(${k})`;

describe("tiers", () => {
  it("uses the registered label of a tier", () => {
    registerTier({ id: "edition_x", labelKey: "tier.x" });
    expect(tierLabel("edition_x", t)).toBe("T(tier.x)");
  });

  it("falls back to a generic label for an unknown tier", () => {
    expect(tierLabel("never_registered", t)).toBe("never_registered");
    expect(tierLabel("free", t)).toBe("free");
  });

  it("accepts entitlements with any tier id (type is a plain string)", () => {
    const e: Entitlements = {
      tier: "edition_x",
      features: [],
      seats: null,
      expiresAt: null,
      nonCommercial: false,
      licensee: null,
    };
    expect(e.tier).toBe("edition_x");
  });

  it("the locked dialog shows the registered tier label", () => {
    testI18n.addResource("en", "common", "tier.x", "Edition X");
    registerTier({ id: "edition_x", labelKey: "tier.x" });
    render(
      <I18nextProvider i18n={testI18n}>
        <FeatureLockedDialog />
      </I18nextProvider>,
    );
    act(() => publishFeatureLocked({ feature: "f", tier: "edition_x" }));
    expect(document.body.textContent).toContain("Edition X");
  });
});
