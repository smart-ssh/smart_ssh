// Spec 0050, Teil 2 ("Testbarkeit"): "Format-Hinweis: falscher Präfix →
// Warnung, Speichern trotzdem möglich; generischer Provider → keine
// Warnung."
import { fireEvent, render, screen } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { describe, expect, it, vi } from "vitest";
import { testI18n } from "../testI18n";
import { AiProviderSettings } from "./AiProviderSettings";

vi.mock("../api", () => ({
  listAiProviders: vi.fn(() => Promise.resolve([])),
  addAiProvider: vi.fn(() => Promise.resolve("new-id")),
  deleteAiProvider: vi.fn(),
  discoverModels: vi.fn(),
  fetchAttestationInfo: vi.fn(),
  setActiveAiProvider: vi.fn(),
  commandErrorMessage: (err: unknown) => String(err),
}));

vi.mock("../riskSettings", () => ({
  loadRiskClassifierSettings: vi.fn(() => Promise.resolve({ enabled: false, providerId: null })),
  saveRiskClassifierSettings: vi.fn(),
}));

function renderForm() {
  return render(
    <I18nextProvider i18n={testI18n}>
      <AiProviderSettings onProvidersChanged={vi.fn()} />
    </I18nextProvider>,
  );
}

describe("AiProviderSettings API-key format hint (Spec 0050, Teil 2)", () => {
  it("shows a non-blocking hint for an Anthropic key with the wrong prefix, and the submit button stays enabled", () => {
    renderForm();

    // Default provider type is "openai" (emptyForm()); switch to Anthropic.
    fireEvent.change(screen.getByLabelText("Typ"), { target: { value: "anthropic" } });
    fireEvent.change(screen.getByLabelText("API-Key"), {
      target: { value: "totally-wrong-format" },
    });

    expect(screen.getByText(/Sieht nicht wie ein Anthropic-Key aus/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Hinzufügen" })).not.toBeDisabled();
  });

  it("shows no hint once the key matches the expected prefix", () => {
    renderForm();

    fireEvent.change(screen.getByLabelText("Typ"), { target: { value: "anthropic" } });
    fireEvent.change(screen.getByLabelText("API-Key"), {
      target: { value: "sk-ant-abc123" },
    });

    expect(screen.queryByText(/Sieht nicht wie ein/)).not.toBeInTheDocument();
  });

  it("shows no hint for a generic OpenAI-compatible provider regardless of key content", () => {
    renderForm();

    fireEvent.change(screen.getByLabelText("Typ"), {
      target: { value: "generic_openai_compatible" },
    });
    fireEvent.change(screen.getByLabelText("API-Key"), {
      target: { value: "whatever-format-this-self-hosted-endpoint-uses" },
    });

    expect(screen.queryByText(/Sieht nicht wie ein/)).not.toBeInTheDocument();
  });

  it("shows no hint while the key field is still empty", () => {
    renderForm();

    fireEvent.change(screen.getByLabelText("Typ"), { target: { value: "anthropic" } });

    expect(screen.queryByText(/Sieht nicht wie ein/)).not.toBeInTheDocument();
  });
});
