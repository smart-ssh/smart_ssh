import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import * as api from "../api/api";

vi.mock("../api/api", async () =>
  (await import("../test/apiMock")).createApiMock({
    getChatSessionRetentionDays: vi.fn(() => Promise.resolve(null)),
    setChatSessionRetentionDays: vi.fn(() => Promise.resolve()),
    getRecordActiveMinutes: vi.fn(() => Promise.resolve(true)),
    setRecordActiveMinutes: vi.fn(() => Promise.resolve()),
  }),
);
vi.mock("./AutoReconnectSettings", () => ({ AutoReconnectSettings: () => null }));
vi.mock("./ElevatedIdleSettings", () => ({ ElevatedIdleSettings: () => null }));

import { ChatRetentionSettings } from "./ChatRetentionSettings";

describe("ChatRetentionSettings: record active minutes (issue #1447)", () => {
  beforeEach(() => {
    vi.mocked(api.setRecordActiveMinutes).mockClear();
  });

  it("shows the stored value and writes the new one when toggled", async () => {
    render(<ChatRetentionSettings />);
    const box = await screen.findByRole("checkbox", { name: /recordActiveMinutes.label/ });
    await waitFor(() => expect(box).toBeChecked());
    fireEvent.click(box);
    await waitFor(() => expect(vi.mocked(api.setRecordActiveMinutes)).toHaveBeenCalledWith(false));
    await waitFor(() => expect(box).not.toBeChecked());
  });
});
