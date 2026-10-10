import { render, screen } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { describe, expect, it, vi } from "vitest";
import { testI18n } from "../testI18n";
import { LanguageSettings } from "./LanguageSettings";

vi.mock("../i18n", () => ({ setLanguage: vi.fn() }));

describe("LanguageSettings", () => {
  it.each(["de", "en"])("shows endonyms regardless of the UI language (%s)", async (lng) => {
    await testI18n.changeLanguage(lng);
    render(
      <I18nextProvider i18n={testI18n}>
        <LanguageSettings />
      </I18nextProvider>,
    );
    expect(screen.getByRole("button", { name: "Deutsch" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "English" })).toBeTruthy();
    await testI18n.changeLanguage("de");
  });
});
