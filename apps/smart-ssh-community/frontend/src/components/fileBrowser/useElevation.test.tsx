import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { sftpElevationDisable, sftpElevationEnable } from "../../api/api";
import { onElevationAutoDisabled, onElevationRevokedByProfile } from "../../api/events";
import { showToast } from "../../buses/toastBus";
import type { ElevationAutoDisabledEvent as ElevationAutoDisabledPayload } from "../../api/types";
import { I18nextProvider } from "react-i18next";
import { testI18n } from "../../testI18n";
import { useElevation } from "./useElevation";

vi.mock("../../api/api", async () =>
  (await import("../../test/apiMock")).createApiMock({
    sftpElevationDisable: vi.fn(),
    sftpElevationEnable: vi.fn(),
  }),
);

vi.mock("../../api/events", () => ({
  onElevationAutoDisabled: vi.fn(),
  onElevationRevokedByProfile: vi.fn(),
}));
vi.mock("../../buses/toastBus", () => ({ showToast: vi.fn() }));

let revoked: (event: { sessionId: string }) => void = () => {};
let autoDisabled: (event: ElevationAutoDisabledPayload) => void = () => {};

const reload = vi.fn();
const onElevationChange = vi.fn();

const active = (targetUser = "root") => ({
  active: true,
  targetUser,
  sftpServerPath: null,
  elevationAllowed: true,
  failure: null,
  warning: null,
  runningOperations: [],
  serverLogging: false,
  sudoAuth: null,
});

describe("useElevation", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(sftpElevationDisable).mockResolvedValue({
      disabled: true,
      runningOperations: [],
    });
    vi.mocked(onElevationAutoDisabled).mockImplementation((handler) => {
      autoDisabled = handler;
      return Promise.resolve(() => {});
    });
    vi.mocked(onElevationRevokedByProfile).mockImplementation((handler) => {
      revoked = handler as typeof revoked;
      return Promise.resolve(() => {});
    });
  });

  const setup = () =>
    renderHook(() => useElevation("s1", { onElevationChange, reload }), {
      wrapper: ({ children }) => (
        <I18nextProvider i18n={testI18n}>{children}</I18nextProvider>
      ),
    });

  it("starts in normal mode and closes a leftover channel on mount", async () => {
    const { result } = setup();
    expect(result.current.elevated).toBe(false);
    expect(result.current.channelUser).toBeNull();
    await waitFor(() =>
      expect(sftpElevationDisable).toHaveBeenCalledWith("s1"),
    );
  });

  it("enable success switches the mode, reports it and reloads", async () => {
    vi.mocked(sftpElevationEnable).mockResolvedValue(active("admin"));
    const { result } = setup();
    await act(() => result.current.handleToggleElevation("admin"));
    expect(result.current.elevated).toBe(true);
    expect(result.current.channelUser).toBe("admin");
    expect(onElevationChange).toHaveBeenCalledWith("admin");
    expect(reload).toHaveBeenCalledWith("admin");
    expect(result.current.enablingElevation).toBe(false);
  });

  it("enable failure shows the failure state and stays normal", async () => {
    vi.mocked(sftpElevationEnable).mockResolvedValue({
      active: false,
      targetUser: "root",
      sftpServerPath: null,
      elevationAllowed: true,
      failure: { kind: "sudoNotAllowed", sudoersLine: "x", detail: null },
    } as never);
    const { result } = setup();
    await act(() => result.current.handleToggleElevation());
    expect(result.current.elevationFailure?.failure?.kind).toBe(
      "sudoNotAllowed",
    );
    expect(result.current.elevated).toBe(false);
    expect(onElevationChange).not.toHaveBeenCalled();
  });

  it("an enable call that throws becomes a checkFailed failure", async () => {
    vi.mocked(sftpElevationEnable).mockRejectedValue("net down");
    const { result } = setup();
    await act(() => result.current.handleToggleElevation());
    expect(result.current.elevationFailure?.failure?.kind).toBe("checkFailed");
    expect(result.current.enablingElevation).toBe(false);
  });

  it("disable with running operations asks first and changes nothing", async () => {
    vi.mocked(sftpElevationEnable).mockResolvedValue(active());
    const { result } = setup();
    await act(() => result.current.handleToggleElevation());
    vi.mocked(sftpElevationDisable).mockResolvedValue({
      disabled: false,
      runningOperations: [{ id: "t1" }] as never,
    });
    await act(() => result.current.handleToggleElevation());
    expect(result.current.stopPrompt?.action).toEqual({ kind: "disable" });
    expect(result.current.elevated).toBe(true);
    expect(onElevationChange).not.toHaveBeenCalledWith(null);

    vi.mocked(sftpElevationDisable).mockResolvedValue({
      disabled: true,
      runningOperations: [],
    });
    await act(() => result.current.confirmStopPrompt());
    expect(sftpElevationDisable).toHaveBeenLastCalledWith("s1", true);
    expect(result.current.stopPrompt).toBeNull();
    expect(result.current.elevated).toBe(false);
    expect(reload).toHaveBeenLastCalledWith(null);
  });

  it("an enable that returns after the mode was switched off is discarded", async () => {
    let resolveEnable!: (v: ReturnType<typeof active>) => void;
    vi.mocked(sftpElevationEnable).mockReturnValue(
      new Promise((r) => {
        resolveEnable = r;
      }) as never,
    );
    const { result } = setup();
    let pending!: Promise<void>;
    act(() => {
      pending = result.current.handleToggleElevation();
    });
    act(() => result.current.updateElevation(null));
    await act(async () => {
      resolveEnable(active());
      await pending;
    });
    expect(result.current.elevated).toBe(false);
    expect(reload).not.toHaveBeenCalled();
  });

  // Issue #1193: the backend switched the channel off after inactivity.
  it("leaves elevated mode and shows the persistent toast with the minutes", async () => {
    vi.mocked(sftpElevationEnable).mockResolvedValue(active());
    const { result } = setup();
    await act(() => result.current.handleToggleElevation());
    expect(result.current.elevated).toBe(true);
    vi.mocked(sftpElevationDisable).mockClear();
    reload.mockClear();

    act(() => autoDisabled({ sessionId: "s1", minutes: 15 }));

    await waitFor(() => expect(result.current.elevated).toBe(false));
    expect(onElevationChange).toHaveBeenLastCalledWith(null);
    expect(reload).toHaveBeenCalledWith(null);
    // The channel is already gone: no extra disable call.
    expect(sftpElevationDisable).not.toHaveBeenCalled();
    expect(showToast).toHaveBeenCalledWith({
      kind: "info",
      message: expect.stringContaining("15"),
    });
  });

  // Issue #1194: the profile switch was turned off while elevated.
  it("leaves elevated mode when the profile revokes the permission", async () => {
    vi.mocked(sftpElevationEnable).mockResolvedValue(active());
    const { result } = setup();
    await act(() => result.current.handleToggleElevation());
    expect(result.current.elevated).toBe(true);
    act(() => revoked({ sessionId: "other" }));
    expect(result.current.elevated).toBe(true);
    act(() => revoked({ sessionId: "s1" }));
    await waitFor(() => expect(result.current.elevated).toBe(false));
    expect(onElevationChange).toHaveBeenLastCalledWith(null);
    expect(showToast).toHaveBeenCalled();
  });

  it("ignores the event of another session or when not elevated", async () => {
    vi.mocked(sftpElevationEnable).mockResolvedValue(active());
    const { result } = setup();
    act(() => autoDisabled({ sessionId: "s1", minutes: 15 }));
    expect(showToast).not.toHaveBeenCalled();
    await act(() => result.current.handleToggleElevation());
    act(() => autoDisabled({ sessionId: "other", minutes: 15 }));
    expect(result.current.elevated).toBe(true);
    expect(showToast).not.toHaveBeenCalled();
  });
});
