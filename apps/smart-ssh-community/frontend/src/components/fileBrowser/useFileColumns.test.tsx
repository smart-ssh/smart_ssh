import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  loadFileManagerColumnWidths,
  loadFileManagerSort,
  loadHiddenFileColumns,
  saveFileManagerSort,
  saveHiddenFileColumns,
} from "../../settings/layoutSettings";
import { COLUMN_ORDER } from "./shared";
import { useFileColumns } from "./useFileColumns";

vi.mock("../../settings/layoutSettings", () => ({
  loadFileManagerColumnWidths: vi.fn(),
  saveFileManagerColumnWidths: vi.fn(() => Promise.resolve()),
  loadFileManagerSort: vi.fn(),
  saveFileManagerSort: vi.fn(() => Promise.resolve()),
  loadHiddenFileColumns: vi.fn(),
  saveHiddenFileColumns: vi.fn(() => Promise.resolve()),
}));

describe("useFileColumns", () => {
  beforeEach(() => {
    vi.mocked(loadFileManagerColumnWidths).mockResolvedValue({});
    vi.mocked(loadFileManagerSort).mockResolvedValue(null);
    vi.mocked(loadHiddenFileColumns).mockResolvedValue([]);
    vi.mocked(saveFileManagerSort).mockClear();
    vi.mocked(saveHiddenFileColumns).mockClear();
  });

  it("propagates a sort change to other mounted panels and reverses from shared state", async () => {
    const a = renderHook(() => useFileColumns());
    const b = renderHook(() => useFileColumns());
    act(() => a.result.current.handleSortClick("size"));
    expect(b.result.current.fileSort).toEqual({ column: "size", direction: "asc" });
    act(() => b.result.current.handleSortClick("size"));
    expect(a.result.current.fileSort).toEqual({ column: "size", direction: "desc" });
    a.unmount();
    act(() => b.result.current.handleSortClick("name"));
    expect(b.result.current.fileSort).toEqual({ column: "name", direction: "asc" });
  });

  it("uses the design's default widths without a saved layout and keeps saved widths", async () => {
    const fresh = renderHook(() => useFileColumns());
    expect(fresh.result.current.columnWidths).toEqual({ size: 110, permissions: 130, modified: 170, owner: 150 });
    expect(COLUMN_ORDER).toEqual(["size", "permissions", "modified", "owner"]);
    vi.mocked(loadFileManagerColumnWidths).mockResolvedValue({ size: 200 });
    const saved = renderHook(() => useFileColumns());
    await waitFor(() => expect(saved.result.current.columnWidths.size).toBe(200));
    expect(saved.result.current.columnWidths.owner).toBe(150);
  });

  it("applies the stored sort on mount", async () => {
    vi.mocked(loadFileManagerSort).mockResolvedValue({
      column: "size",
      direction: "desc",
    });
    const { result } = renderHook(() => useFileColumns());
    await waitFor(() =>
      expect(result.current.fileSort).toEqual({
        column: "size",
        direction: "desc",
      }),
    );
  });

  it("toggles the sort direction and persists it", async () => {
    const { result } = renderHook(() => useFileColumns());
    const first = result.current.fileSort;
    act(() => result.current.handleSortClick("size"));
    expect(result.current.fileSort).toEqual({
      column: "size",
      direction: "asc",
    });
    expect(saveFileManagerSort).toHaveBeenLastCalledWith({
      column: "size",
      direction: "asc",
    });
    act(() => result.current.handleSortClick("size"));
    expect(result.current.fileSort).toEqual({
      column: "size",
      direction: "desc",
    });
    expect(saveFileManagerSort).toHaveBeenLastCalledWith({
      column: "size",
      direction: "desc",
    });
    expect(first).not.toEqual(result.current.fileSort);
  });

  it("toggles a hidden column and persists the list", async () => {
    const { result } = renderHook(() => useFileColumns());
    await waitFor(() => expect(loadHiddenFileColumns).toHaveBeenCalled());
    act(() => result.current.toggleColumn("owner"));
    expect(result.current.hiddenColumns).toEqual(["owner"]);
    expect(saveHiddenFileColumns).toHaveBeenLastCalledWith(["owner"]);
    act(() => result.current.toggleColumn("owner"));
    expect(result.current.hiddenColumns).toEqual([]);
    expect(saveHiddenFileColumns).toHaveBeenLastCalledWith([]);
  });
});
