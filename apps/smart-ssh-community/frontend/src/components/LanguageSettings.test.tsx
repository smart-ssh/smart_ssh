import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
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

  it("marks only the selected theme and language segment with the accent rule", async () => {
    await testI18n.changeLanguage("de");
    render(
      <I18nextProvider i18n={testI18n}>
        <LanguageSettings />
      </I18nextProvider>,
    );
    const accent = (el: HTMLElement) => el.className.split(/\s+/).includes("border-b-accent");
    const pressedLanguage = screen.getByRole("button", { name: "Deutsch" });
    expect(pressedLanguage.getAttribute("aria-pressed")).toBe("true");
    expect(accent(pressedLanguage)).toBe(true);
    expect(accent(screen.getByRole("button", { name: "English" }))).toBe(false);
    const pressed = screen.getAllByRole("button").filter((b) => b.getAttribute("aria-pressed") === "true");
    // exactly one theme and one language segment are selected, and only those carry the rule
    expect(pressed).toHaveLength(2);
    for (const b of screen.getAllByRole("button")) {
      expect(accent(b)).toBe(b.getAttribute("aria-pressed") === "true");
    }
    const [theme] = pressed.filter((b) => b !== pressedLanguage);
    const other = screen
      .getAllByRole("button")
      .find((b) => b.getAttribute("aria-pressed") === "false" && b.className.includes("label-condensed"))!;
    fireEvent.click(other);
    await waitFor(() => expect(accent(other)).toBe(true));
    expect(accent(theme)).toBe(false);
    cleanup();
  });
});
