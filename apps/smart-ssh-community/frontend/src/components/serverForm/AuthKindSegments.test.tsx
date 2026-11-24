import { fireEvent, render, screen } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { describe, expect, it, vi } from "vitest";
import { testI18n } from "../../testI18n";
import { AuthKindSegments } from "./AuthKindSegments";

const OPTIONS = [
  { kind: "password", label: "Passwort" },
  { kind: "privateKey", label: "Private Key" },
] as const;

function renderSegments(onChange = vi.fn()) {
  render(
    <I18nextProvider i18n={testI18n}>
      <AuthKindSegments value="password" options={[...OPTIONS]} onChange={onChange} />
    </I18nextProvider>,
  );
  return onChange;
}

describe("AuthKindSegments (issue #1380)", () => {
  it("is a radio group with one radio per option and the current one checked", () => {
    renderSegments();
    expect(screen.getByRole("radiogroup")).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: "Passwort" })).toBeChecked();
    expect(screen.getByRole("radio", { name: "Private Key" })).not.toBeChecked();
  });

  it("reports a switch once and does not fire for the already selected option", () => {
    const onChange = renderSegments();
    fireEvent.click(screen.getByRole("radio", { name: "Passwort" }));
    expect(onChange).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("radio", { name: "Private Key" }));
    expect(onChange).toHaveBeenCalledExactlyOnceWith("privateKey");
  });
});
