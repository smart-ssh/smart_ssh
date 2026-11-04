import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { sftpElevationDisable, sftpElevationEnable } from "../../api";
import { useElevation } from "./useElevation";

vi.mock("../../api", async () =>
  (await import("../../test/apiMock")).createApiMock({
    sftpElevationDisable: vi.fn(),
    sftpElevationEnable: vi.fn(),
  }),
);

const reload = vi.fn();
const onElevationChange = vi.fn();

const active = (targetUser = "root") => ({
  active: true,
  targetUser,
  sftpServerPath: null,
  failure: null,
});

describe("useElevation", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(sftpElevationDisable).mockResolvedValue({ disabled: true, runningOperations: [] });
  });

  const setup = () => renderHook(() => useElevation("s1", { onElevationChange, reload }));

  it("starts in normal mode and closes a leftover channel on mount", async () => {
    const { result } = setup();
    expect(result.current.elevated).toBe(false);
    expect(result.current.channelUser).toBeNull();
    await waitFor(() => expect(sftpElevationDisable).toHaveBeenCalledWith("s1"));
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
      failure: { kind: "sudoNotAllowed", sudoersLine: "x", detail: null },
    } as never);
    const { result } = setup();
    await act(() => result.current.handleToggleElevation());
    expect(result.current.elevationFailure?.failure?.kind).toBe("sudoNotAllowed");
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

    vi.mocked(sftpElevationDisable).mockResolvedValue({ disabled: true, runningOperations: [] });
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
});
