import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { testI18n } from "../../testI18n";
import type { Activity } from "../../api/activitiesApi";
import { resetActivitiesStoreForTests } from "../../hooks/useActivities";
import { resetInterruptedForTests } from "../../hooks/useInterruptedActivities";
import { registerActivityResumeHandler } from "../../lib/activityResume";
import { ActivityCenter, combinedProgress } from "./ActivityCenter";

const api = vi.hoisted(() => ({
  list: vi.fn(),
  cancel: vi.fn(),
  dismiss: vi.fn(),
  interrupted: vi.fn(),
  resolve: vi.fn(),
  handler: null as null | ((a: Activity) => void),
}));

vi.mock("../../api/activitiesApi", () => ({
  listActivities: () => api.list(),
  cancelActivity: (id: string) => api.cancel(id),
  dismissActivity: (id: string) => api.dismiss(id),
  listInterrupted: (kinds: string[]) => api.interrupted(kinds),
  resolveInterrupted: (id: string, action: string) => api.resolve(id, action),
}));
vi.mock("../../api/events", () => ({
  onActivityChanged: (h: (a: Activity) => void) => {
    api.handler = h;
    return Promise.resolve(() => {
      api.handler = null;
    });
  },
}));

function activity(over: Partial<Activity> = {}): Activity {
  return {
    id: "a1",
    kind: "k",
    key: "",
    titleKey: "activities.title",
    titleParams: {},
    state: "running",
    done: 0,
    total: 1000,
    statusKey: null,
    errorCode: null,
    errorMessage: null,
    cancellable: false,
    resumable: false,
    startedAt: 1,
    dismissed: false,
    ...over,
  };
}

function renderCenter() {
  return render(
    <I18nextProvider i18n={testI18n}>
      <ActivityCenter />
    </I18nextProvider>,
  );
}

async function emit(a: Activity) {
  await waitFor(() => expect(api.handler).not.toBeNull());
  act(() => api.handler?.(a));
}

describe("ActivityCenter", () => {
  beforeEach(() => {
    api.list.mockReset().mockResolvedValue([]);
    api.cancel.mockReset().mockResolvedValue(null);
    api.dismiss.mockReset().mockResolvedValue(null);
    api.interrupted.mockReset().mockResolvedValue([]);
    api.resolve.mockReset().mockResolvedValue(null);
  });
  afterEach(() => {
    resetActivitiesStoreForTests();
    resetInterruptedForTests();
  });

  it("shows no icon without activities", async () => {
    renderCenter();
    await waitFor(() => {
      expect(api.list).toHaveBeenCalled();
      expect(screen.queryByTestId("activity-center-button")).toBeNull();
    });
  });

  it("shows the running count and a row with progress and size", async () => {
    renderCenter();
    await emit(activity({ id: "a", done: 512, total: 1024, startedAt: 1 }));
    await emit(activity({ id: "b", done: 0, total: null, startedAt: 2 }));
    const button = await screen.findByTestId("activity-center-button");
    expect(button).toHaveTextContent("2");
    fireEvent.click(button);
    const dialog = await screen.findByRole("dialog");
    const bars = within(dialog).getAllByRole("progressbar");
    expect(bars).toHaveLength(2);
    expect(bars[0]).toHaveAttribute("value", "512");
    expect(bars[0]).toHaveAttribute("max", "1024");
    expect(bars[0]).toHaveAttribute("aria-valuetext", "512 B / 1.0 KB");
    // total unknown: indeterminate, no value
    expect(bars[1]).not.toHaveAttribute("value");
  });

  it("shows a cancel button only for cancellable activities and calls cancel", async () => {
    renderCenter();
    await emit(activity({ id: "a", cancellable: true, startedAt: 1 }));
    await emit(activity({ id: "b", cancellable: false, startedAt: 2 }));
    fireEvent.click(await screen.findByTestId("activity-center-button"));
    const dialog = await screen.findByRole("dialog");
    const cancels = within(dialog).getAllByRole("button", { name: /abbrechen/i });
    expect(cancels).toHaveLength(1);
    fireEvent.click(cancels[0]);
    expect(api.cancel).toHaveBeenCalledWith("a");
  });

  it("keeps a failed row with its error until dismissed", async () => {
    renderCenter();
    await emit(
      activity({ state: "failed", errorCode: "E", errorMessage: "disk full", cancellable: false }),
    );
    fireEvent.click(await screen.findByTestId("activity-center-button"));
    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).getByText(/disk full/)).toBeInTheDocument();
    fireEvent.click(within(dialog).getByRole("button", { name: /ausblenden/i }));
    expect(api.dismiss).toHaveBeenCalledWith("a1");
    await emit(activity({ state: "failed", dismissed: true }));
    await waitFor(() => expect(screen.queryByTestId("activity-center-button")).toBeNull());
  });

  it("shows the translated message for a speech model error code (issue #1326)", async () => {
    renderCenter();
    await emit(
      activity({
        state: "failed",
        errorCode: "speech_model_hash_mismatch",
        errorMessage: "speech model download failed",
        titleKey: "activities.speechModelTitle",
        titleParams: { name: "Base" },
      }),
    );
    fireEvent.click(await screen.findByTestId("activity-center-button"));
    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).getByText(/Integritätsprüfung/)).toBeInTheDocument();
    expect(within(dialog).queryByText(/download failed/)).toBeNull();
    expect(within(dialog).getByText(/Sprachmodell Base/)).toBeInTheDocument();
  });

  it("shows a check mark for success and clears it when the popover closes", async () => {
    renderCenter();
    await emit(activity({ state: "succeeded", done: 1000 }));
    const button = await screen.findByTestId("activity-center-button");
    expect(button).toHaveTextContent("✓");
    fireEvent.click(button);
    expect(await screen.findByRole("dialog")).toBeInTheDocument();
    fireEvent.keyDown(document, { key: "Escape" });
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(api.dismiss).toHaveBeenCalledWith("a1");
    expect(screen.getByTestId("activity-center-button")).toHaveFocus();
  });

  it("Escape closes the popover and returns focus to the icon", async () => {
    renderCenter();
    await emit(activity());
    const button = await screen.findByTestId("activity-center-button");
    fireEvent.click(button);
    await screen.findByRole("dialog");
    fireEvent.keyDown(document, { key: "Escape" });
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(button).toHaveFocus();
  });

  it("loads activities that are already running on mount", async () => {
    api.list.mockResolvedValue([activity({ id: "early" })]);
    renderCenter();
    expect(await screen.findByTestId("activity-center-button")).toBeInTheDocument();
  });

  it("combines progress over running activities with a known total", () => {
    expect(
      combinedProgress([
        activity({ done: 100, total: 200 }),
        activity({ id: "b", done: 300, total: 600 }),
        activity({ id: "c", total: null }),
        activity({ id: "d", state: "succeeded", done: 9, total: 9 }),
      ]),
    ).toBeCloseTo(0.5);
    expect(combinedProgress([activity({ total: null })])).toBeNull();
  });

  describe("interrupted activities (issue #1327)", () => {
    const row = (over = {}) => ({
      id: "i1",
      kind: "speech-model",
      key: "base",
      titleKey: "activities.speechModelTitle",
      titleParams: { name: "Base" },
      done: 50,
      total: 100,
      native: true,
      ...over,
    });

    it("lists a stored entry as Interrupted with Resume and Discard", async () => {
      api.interrupted.mockResolvedValue([row()]);
      renderCenter();
      fireEvent.click(await screen.findByTestId("activity-center-button"));
      const item = await screen.findByRole("listitem", { name: "Sprachmodell Base" });
      expect(within(item).getByText(/Unterbrochen/)).toBeTruthy();
      expect(within(item).getByRole("button", { name: /fortsetzen/i })).toBeTruthy();
      expect(within(item).getByRole("button", { name: /verwerfen/i })).toBeTruthy();
    });

    it("Resume calls the backend handler and the row goes away", async () => {
      api.interrupted.mockResolvedValue([row()]);
      renderCenter();
      fireEvent.click(await screen.findByTestId("activity-center-button"));
      fireEvent.click(await screen.findByRole("button", { name: /fortsetzen/i }));
      await waitFor(() => expect(screen.queryByTestId("activity-center-button")).toBeNull());
      expect(api.resolve).toHaveBeenCalledWith("i1", "resume");
    });

    it("Discard calls the backend discard and removes the row", async () => {
      api.interrupted.mockResolvedValue([row()]);
      renderCenter();
      fireEvent.click(await screen.findByTestId("activity-center-button"));
      fireEvent.click(await screen.findByRole("button", { name: /verwerfen/i }));
      await waitFor(() => expect(screen.queryByTestId("activity-center-button")).toBeNull());
      expect(api.resolve).toHaveBeenCalledWith("i1", "discard");
    });

    it("an extension kind goes through its frontend handler, then the entry is removed", async () => {
      const resume = vi.fn();
      const discard = vi.fn();
      const un = registerActivityResumeHandler("ext-kind", { resume, discard });
      api.interrupted.mockResolvedValue([row({ id: "i2", kind: "ext-kind", native: false })]);
      renderCenter();
      expect(api.interrupted).toHaveBeenCalledWith(["ext-kind"]);
      fireEvent.click(await screen.findByTestId("activity-center-button"));
      fireEvent.click(await screen.findByRole("button", { name: /fortsetzen/i }));
      await waitFor(() => expect(api.resolve).toHaveBeenCalledWith("i2", "remove"));
      expect(resume).toHaveBeenCalledTimes(1);
      expect(api.resolve).not.toHaveBeenCalledWith("i2", "resume");
      un();
    });
  });
});
