// Spec 0099 (BL-0054), T6-T9.
import { render, screen, waitFor } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { afterEach, describe, expect, it, vi } from "vitest";
import { testI18n } from "../testI18n";
import { ThirdPartyLicensesDialog } from "./ThirdPartyLicensesDialog";

const MARKER = "SMART-SSH-THIRD-PARTY-NOTICES-V1";

function renderDialog() {
  return render(
    <I18nextProvider i18n={testI18n}>
      <ThirdPartyLicensesDialog onClose={() => {}} />
    </I18nextProvider>,
  );
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("ThirdPartyLicensesDialog (Spec 0099)", () => {
  it("T6: shows the license text once the fetch resolves with a marked file", async () => {
    const content = `${MARKER}\n\nMIT License\n\nVerwendet von: example 1.0.0`;
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({ ok: true, text: () => Promise.resolve(content) }),
    );

    renderDialog();

    expect(await screen.findByText(/MIT License/)).toBeInTheDocument();
  });

  it("T7a: shows the 'release builds only' hint instead of crashing when the fetch fails", async () => {
    vi.stubGlobal("fetch", vi.fn().mockRejectedValue(new Error("network error")));

    renderDialog();

    expect(
      await screen.findByText("Drittanbieter-Lizenzen sind nur in Release-Builds enthalten."),
    ).toBeInTheDocument();
  });

  it("T7b: shows the hint (not the loaded content) when the response has no marker — e.g. the dev server's SPA fallback page", async () => {
    const spaFallbackHtml = "<!doctype html><html><body><div id=\"root\"></div></body></html>";
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({ ok: true, text: () => Promise.resolve(spaFallbackHtml) }),
    );

    renderDialog();

    expect(
      await screen.findByText("Drittanbieter-Lizenzen sind nur in Release-Builds enthalten."),
    ).toBeInTheDocument();
    expect(screen.queryByText(/doctype/)).not.toBeInTheDocument();
  });

  it("T8: renders an adversarial license text as literal text, never as markup", async () => {
    const adversarial = `${MARKER}\n\n<img src=x onerror=alert(1)>\n<script>alert(1)</script>`;
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue({ ok: true, text: () => Promise.resolve(adversarial) }),
    );

    const { container } = renderDialog();

    await waitFor(() => expect(screen.getByText(/onerror=alert/)).toBeInTheDocument());
    expect(container.querySelector("img")).toBeNull();
    expect(container.querySelector("script")).toBeNull();
    expect(screen.getByText(/onerror=alert/).textContent).toContain("<script>alert(1)</script>");
  });

  it("T9: only ever requests a relative address, never a foreign host", async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValue({ ok: true, text: () => Promise.resolve(`${MARKER}\n\ncontent`) });
    vi.stubGlobal("fetch", fetchMock);

    renderDialog();
    await waitFor(() => expect(fetchMock).toHaveBeenCalled());

    expect(fetchMock).toHaveBeenCalledTimes(1);
    const requestedUrl = fetchMock.mock.calls[0][0] as string;
    expect(requestedUrl.startsWith("/")).toBe(true);
    expect(requestedUrl).not.toMatch(/^[a-z]+:\/\//i);
  });
});
