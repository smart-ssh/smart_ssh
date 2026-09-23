// Spec 0072, Teil 3 (BL-0202, C1/C2): der Platzhaltertext ohne Auswahl
// ("Links eine Gruppe oder einen Server auswählen, ...") war fest deutsch
// eingebaut und erschien so auch in der englischen Oberfläche. *Gegenbeweis:*
// vor der Umstellung auf `t("management.emptyState", ...)` lieferte die
// englische `testI18n`-Instanz denselben deutschen Text zurück, weil er gar
// nicht über i18next lief — dieser Test schlug fehl (verifiziert, dann
// wiederhergestellt).
import { render, screen } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { describe, expect, it, vi } from "vitest";
import { testI18n } from "../testI18n";
import { ManagementView } from "./ManagementView";

vi.mock("../api", () => ({
  listGroups: vi.fn(() => Promise.resolve([])),
  listServers: vi.fn(() => Promise.resolve([])),
  commandErrorMessage: (err: unknown) => String(err),
}));

function renderWithLanguage(language: "de" | "en") {
  void testI18n.changeLanguage(language);
  return render(
    <I18nextProvider i18n={testI18n}>
      <ManagementView />
    </I18nextProvider>,
  );
}

describe("ManagementView empty state (Spec 0072, C1/C2)", () => {
  it("zeigt den deutschen Text auf Deutsch", () => {
    renderWithLanguage("de");

    expect(
      screen.getByText(/Links eine Gruppe oder einen Server auswählen/),
    ).toBeInTheDocument();
  });

  it("zeigt den übersetzten Text auf Englisch, nicht den deutschen", () => {
    renderWithLanguage("en");

    expect(screen.getByText(/Select a group or server on the left/)).toBeInTheDocument();
    expect(screen.queryByText(/Links eine Gruppe/)).not.toBeInTheDocument();
  });
});
