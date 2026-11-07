import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const storeGet = vi.fn();
const setUiPreference = vi.fn();

vi.mock("../../i18n", () => ({
  setUiPreference: (key: string, value: unknown) => setUiPreference(key, value),
  settingsStore: () => Promise.resolve({ get: storeGet }),
}));

const { ElevationNotice } = await import("./ElevationNotice");
const { ElevationBanner } = await import("./ElevationBanner");

const KEY = "elevationNoticeDismissedServers";

function notice(props: Partial<React.ComponentProps<typeof ElevationNotice>> = {}) {
  return (
    <ElevationNotice serverId="s1" elevatedUser="root" serverLogging={false} passwordless={false} {...props} />
  );
}

beforeEach(() => {
  storeGet.mockReset().mockResolvedValue(undefined);
  setUiPreference.mockReset().mockResolvedValue(undefined);
});

describe("ElevationNotice (issue #1195)", () => {
  it("shows on first activation, again after 'Got it' on a later activation, not after 'Don't show again'", async () => {
    const first = render(notice());
    fireEvent.click(await screen.findByRole("button", { name: "fileElevation.notice.gotIt" }));
    expect(screen.queryByRole("status")).toBeNull();
    expect(setUiPreference).not.toHaveBeenCalled();
    first.unmount();

    // Later activation in the same app run (fresh mount): shown again.
    const second = render(notice());
    fireEvent.click(await screen.findByRole("button", { name: "fileElevation.notice.dontShowAgain" }));
    expect(screen.queryByRole("status")).toBeNull();
    await waitFor(() => expect(setUiPreference).toHaveBeenCalledWith(KEY, ["s1"]));
    second.unmount();

    // The store now holds s1: no notice for s1, but one for another server.
    storeGet.mockResolvedValue(["s1"]);
    const third = render(notice());
    await act(async () => {});
    expect(screen.queryByRole("status")).toBeNull();
    third.unmount();
    render(notice({ serverId: "s2" }));
    expect(await screen.findByRole("status")).toBeTruthy();
  });

  it("shows the logging and passwordless sentences only when the result says so", async () => {
    const { unmount } = render(notice());
    const panel = await screen.findByRole("status");
    expect(within(panel).getByText("fileElevation.notice.loggedApp")).toBeTruthy();
    expect(within(panel).queryByText("fileElevation.notice.loggedAppAndServer")).toBeNull();
    expect(within(panel).queryByText("fileElevation.notice.passwordless")).toBeNull();
    unmount();
    render(notice({ serverLogging: true, passwordless: true }));
    const full = await screen.findByRole("status");
    expect(within(full).getByText("fileElevation.notice.loggedAppAndServer")).toBeTruthy();
    expect(within(full).getByText("fileElevation.notice.passwordless")).toBeTruthy();
  });

  it("is a keyboard-operable disclosure announced as a status region", async () => {
    render(notice());
    const toggle = await screen.findByRole("button", { name: /fileElevation\.notice\.title/ });
    expect(toggle.getAttribute("aria-expanded")).toBe("true");
    expect(screen.getByRole("status")).toBeTruthy();
    // A real <button>: reachable by Tab and operated by Enter/Space, which
    // the browser turns into a click.
    expect(toggle.tagName).toBe("BUTTON");
    expect((toggle as HTMLButtonElement).tabIndex).toBeGreaterThanOrEqual(0);
    fireEvent.click(toggle);
    expect(toggle.getAttribute("aria-expanded")).toBe("false");
    expect(screen.queryByRole("status")).toBeNull();
    fireEvent.click(toggle);
    expect(screen.getByRole("status")).toBeTruthy();
    for (const name of ["fileElevation.notice.gotIt", "fileElevation.notice.dontShowAgain"]) {
      expect((screen.getByRole("button", { name }) as HTMLButtonElement).tabIndex).toBeGreaterThanOrEqual(0);
    }
  });

  it("shows nothing without a server id", async () => {
    render(notice({ serverId: undefined }));
    await act(async () => {});
    expect(screen.queryByRole("status")).toBeNull();
  });

  it("does not delay the banner and renders inside it", async () => {
    render(<ElevationBanner elevatedUser="root" warningPath={null} serverId="s1" onToggleOff={() => {}} />);
    expect(screen.getByRole("alert")).toBeTruthy();
    expect(await screen.findByRole("status")).toBeTruthy();
  });
});
