import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { testI18n } from "../../testI18n";
import type { Activity } from "../../api/activitiesApi";
import { QuitConfirmDialog } from "./QuitConfirmDialog";

const api = vi.hoisted(() => ({
  confirm: vi.fn(),
  handler: null as null | ((p: { activities: Activity[] }) => void),
}));

vi.mock("../../api/activitiesApi", () => ({ confirmQuit: () => api.confirm() }));
vi.mock("../../api/events", () => ({
  onQuitRequested: (h: (p: { activities: Activity[] }) => void) => {
    api.handler = h;
    return Promise.resolve(() => {
      api.handler = null;
    });
  },
}));

function activity(over: Partial<Activity> = {}): Activity {
  return {
    id: "a1",
    kind: "speech-model",
    key: "base",
    titleKey: "activities.speechModelTitle",
    titleParams: { name: "Base" },
    state: "running",
    done: 0,
    total: 100,
    statusKey: null,
    errorCode: null,
    errorMessage: null,
    cancellable: true,
    resumable: true,
    startedAt: 1,
    dismissed: false,
    ...over,
  };
}

async function requestQuit(activities: Activity[]) {
  await waitFor(() => expect(api.handler).not.toBeNull());
  act(() => api.handler?.({ activities }));
  return screen.findByRole("alertdialog");
}

function renderDialog() {
  return render(
    <I18nextProvider i18n={testI18n}>
      <QuitConfirmDialog />
    </I18nextProvider>,
  );
}

describe("QuitConfirmDialog", () => {
  beforeEach(() => {
    api.confirm.mockReset().mockResolvedValue(null);
    api.handler = null;
  });

  it("shows nothing until a quit is requested", async () => {
    renderDialog();
    await waitFor(() => expect(api.handler).not.toBeNull());
    expect(screen.queryByRole("alertdialog")).toBeNull();
  });

  it("lists the running activities by name", async () => {
    renderDialog();
    const dialog = await requestQuit([activity(), activity({ id: "a2", titleParams: { name: "Small" } })]);
    expect(within(dialog).getByText("Sprachmodell Base")).toBeTruthy();
    expect(within(dialog).getByText("Sprachmodell Small")).toBeTruthy();
  });

  it("Keep running closes the dialog without quitting", async () => {
    renderDialog();
    const dialog = await requestQuit([activity()]);
    fireEvent.click(within(dialog).getByRole("button", { name: "Weiterlaufen lassen" }));
    await waitFor(() => expect(screen.queryByRole("alertdialog")).toBeNull());
    expect(api.confirm).not.toHaveBeenCalled();
  });

  it("Quit anyway confirms the quit", async () => {
    renderDialog();
    const dialog = await requestQuit([activity()]);
    fireEvent.click(within(dialog).getByRole("button", { name: "Trotzdem beenden" }));
    await waitFor(() => expect(api.confirm).toHaveBeenCalledTimes(1));
  });
});
