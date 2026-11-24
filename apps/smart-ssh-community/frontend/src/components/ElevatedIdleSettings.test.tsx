import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { getElevatedIdleTimeout, setElevatedIdleTimeout } from "../api/api";
import { testI18n } from "../testI18n";
import { ElevatedIdleSettings } from "./ElevatedIdleSettings";

vi.mock("../api/api", async () =>
  (await import("../test/apiMock")).createApiMock(),
);

function renderSettings() {
  return render(
    <I18nextProvider i18n={testI18n}>
      <ElevatedIdleSettings />
    </I18nextProvider>,
  );
}

describe("ElevatedIdleSettings (issue #1193)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(getElevatedIdleTimeout).mockResolvedValue(30);
    vi.mocked(setElevatedIdleTimeout).mockResolvedValue(undefined);
  });

  it("shows the stored value and offers 5, 15, 30, 60 minutes and never", async () => {
    renderSettings();
    const select = await screen.findByRole("combobox");
    await waitFor(() => expect(select).toHaveValue("30"));
    const values = Array.from(select.querySelectorAll("option")).map((o) =>
      o.getAttribute("value"),
    );
    expect(values).toEqual(["5", "15", "30", "60", "0"]);
  });

  it("saves a change through its own command", async () => {
    renderSettings();
    const select = await screen.findByRole("combobox");
    await waitFor(() => expect(select).toHaveValue("30"));
    fireEvent.change(select, { target: { value: "0" } });
    await waitFor(() => expect(select).toHaveValue("0"));
    expect(setElevatedIdleTimeout).toHaveBeenCalledWith(0);
  });

  it("restores the previous value and shows the error when saving fails", async () => {
    vi.mocked(setElevatedIdleTimeout).mockRejectedValue("nope");
    renderSettings();
    const select = await screen.findByRole("combobox");
    await waitFor(() => expect(select).toHaveValue("30"));
    fireEvent.change(select, { target: { value: "60" } });
    expect(await screen.findByRole("alert")).toBeInTheDocument();
    await waitFor(() => expect(select).toHaveValue("30"));
  });
});
