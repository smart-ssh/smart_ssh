import { act, renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { I18nextProvider } from "react-i18next";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { sftpList, sftpStartDirectory } from "../../api/api";
import { testI18n } from "../../testI18n";
import type { RemoteEntryDto } from "../../api/types";
import { useRemoteDirectory } from "./useRemoteDirectory";

vi.mock("../../api/api", async () =>
  (await import("../../test/apiMock")).createApiMock({
    sftpList: vi.fn(),
    sftpStartDirectory: vi.fn(),
  }),
);
vi.mock("../../buses/toastBus", () => ({ showToast: vi.fn() }));

const entry = (name: string): RemoteEntryDto =>
  ({
    name,
    path: `/home/u/${name}`,
    isDir: false,
    size: 1,
    permissionsOctal: 0o644,
  }) as unknown as RemoteEntryDto;

const wrapper = ({ children }: { children: ReactNode }) => (
  <I18nextProvider i18n={testI18n}>{children}</I18nextProvider>
);

describe("useRemoteDirectory", () => {
  beforeEach(() => {
    vi.mocked(sftpList).mockReset();
    vi.mocked(sftpStartDirectory).mockReset();
    vi.mocked(sftpStartDirectory).mockResolvedValue({
      path: "/home/u",
      missingDirectory: null,
    });
  });

  it("loads the start directory on mount", async () => {
    vi.mocked(sftpList).mockResolvedValue([entry("a.txt")]);
    const { result } = renderHook(() => useRemoteDirectory("s1", null), {
      wrapper,
    });
    await waitFor(() =>
      expect(result.current.entries.map((e) => e.name)).toEqual(["a.txt"]),
    );
    expect(result.current.path).toBe("/home/u");
    expect(result.current.pathInput).toBe("/home/u");
    expect(result.current.startPath).toBe("/home/u");
    expect(result.current.loading).toBe(false);
    expect(result.current.error).toBeNull();
    expect(sftpList).toHaveBeenCalledWith("s1", "/home/u", null);
  });

  it("reports a load error and keeps the previous path", async () => {
    vi.mocked(sftpList).mockResolvedValueOnce([entry("a.txt")]);
    const { result } = renderHook(() => useRemoteDirectory("s1", null), {
      wrapper,
    });
    await waitFor(() => expect(result.current.path).toBe("/home/u"));
    vi.mocked(sftpList).mockRejectedValueOnce("boom");
    act(() => result.current.load("/nope"));
    await waitFor(() => expect(result.current.error).toBe("boom"));
    expect(result.current.path).toBe("/home/u");
    expect(result.current.loading).toBe(false);
  });

  it("refreshing the current path keeps the path", async () => {
    vi.mocked(sftpList).mockResolvedValueOnce([entry("a.txt")]);
    const { result } = renderHook(() => useRemoteDirectory("s1", null), {
      wrapper,
    });
    await waitFor(() => expect(result.current.path).toBe("/home/u"));
    vi.mocked(sftpList).mockResolvedValueOnce([entry("a.txt"), entry("b.txt")]);
    act(() => result.current.load(result.current.path));
    await waitFor(() => expect(result.current.entries).toHaveLength(2));
    expect(result.current.path).toBe("/home/u");
  });

  it("passes the elevated channel user to the listing", async () => {
    vi.mocked(sftpList).mockResolvedValue([]);
    const { result } = renderHook(() => useRemoteDirectory("s1", "root"), {
      wrapper,
    });
    await waitFor(() =>
      expect(sftpList).toHaveBeenCalledWith("s1", "/home/u", "root"),
    );
    act(() => result.current.load("/etc", null));
    await waitFor(() =>
      expect(sftpList).toHaveBeenCalledWith("s1", "/etc", null),
    );
  });
});
