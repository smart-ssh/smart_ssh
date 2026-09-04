import { act, render, screen } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { describe, expect, it } from "vitest";
import { testI18n } from "../testI18n";
import { FeatureLockedDialog } from "./FeatureLockedDialog";
import { publishFeatureLocked } from "./featureLockedBus";

describe("FeatureLockedDialog", () => {
  it("renders nothing until a feature_locked error is published", () => {
    render(
      <I18nextProvider i18n={testI18n}>
        <FeatureLockedDialog />
      </I18nextProvider>,
    );

    expect(screen.queryByRole("button")).not.toBeInTheDocument();
  });

  it("shows the locked feature/tier via document.body (portal), not the render tree", () => {
    const { container } = render(
      <I18nextProvider i18n={testI18n}>
        <FeatureLockedDialog />
      </I18nextProvider>,
    );

    act(() => {
      publishFeatureLocked({ feature: "document_export", tier: "free" });
    });

    // Regression: a previous version rendered inline instead of into a
    // portal, so an ancestor's `display:none` (e.g. an inactive session
    // tab, s. `App.tsx`) could hide this dialog entirely — s.
    // `FeatureLockedDialog.tsx`'s Doc-Kommentar.
    expect(container).toBeEmptyDOMElement();
    expect(screen.getByRole("button")).toBeInTheDocument();
    expect(document.body.textContent).toContain("document_export");
    expect(document.body.textContent).toContain("free");
  });
});
