// Spec 0105: Anzeige einer serverseitigen Web-Recherche im Chat.
import { render, screen } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { describe, expect, it } from "vitest";
import { testI18n } from "../testI18n";
import type { WebActivityDto } from "../types";
import { WebActivityCard } from "./WebActivityCard";

function activity(overrides: Partial<WebActivityDto> = {}): WebActivityDto {
  return {
    kind: "search",
    input: "nginx 1.29 release notes",
    results: [
      { title: "nginx changes", url: "https://nginx.org/en/CHANGES" },
      { title: "Other", url: "https://example.com/other" },
    ],
    cited: [{ title: "nginx changes", url: "https://nginx.org/en/CHANGES" }],
    contentTruncated: false,
    errorCode: null,
    ...overrides,
  };
}

function renderCard(a: WebActivityDto) {
  return render(
    <I18nextProvider i18n={testI18n}>
      <WebActivityCard activity={a} />
    </I18nextProvider>,
  );
}

describe("WebActivityCard (Spec 0105)", () => {
  it("shows the search query and the cited sources, not as links", () => {
    renderCard(activity());
    expect(screen.getByText(/Websuche: nginx 1.29 release notes/)).toBeInTheDocument();
    expect(screen.getByText(/https:\/\/nginx.org\/en\/CHANGES/)).toBeInTheDocument();
    expect(screen.queryByText(/example.com\/other/)).not.toBeInTheDocument();
    expect(screen.queryByRole("link")).not.toBeInTheDocument();
  });

  it("shows the fetched URL", () => {
    renderCard(activity({ kind: "fetch", input: "https://example.com/doc", cited: [] }));
    expect(screen.getByText(/Webseite gelesen: https:\/\/example.com\/doc/)).toBeInTheDocument();
    expect(screen.getByText("Seite gelesen, nicht zitiert.")).toBeInTheDocument();
  });

  it("shows a readable note for a provider tool error", () => {
    renderCard(activity({ errorCode: "max_uses_exceeded", results: [], cited: [] }));
    expect(screen.getByText(/Höchstzahl an Websuchen/)).toBeInTheDocument();
  });

  it("falls back to a generic note with the code for an unknown error", () => {
    renderCard(activity({ errorCode: "brand_new_code", results: [], cited: [] }));
    expect(screen.getByText(/brand_new_code/)).toBeInTheDocument();
  });
});
