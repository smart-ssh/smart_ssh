import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  loadFileManagerColumnWidths,
  loadFileManagerSort,
  loadHiddenFileColumns,
  saveFileManagerSort,
  saveHiddenFileColumns,
} from "../../layoutSettings";
import { useFileColumns } from "./useFileColumns";

vi.mock("../../layoutSettings", () => ({
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
