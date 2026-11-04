import { act, renderHook } from "@testing-library/react";
import type { ReactNode } from "react";
import { I18nextProvider } from "react-i18next";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { deleteServer } from "../../api/api";
import { testI18n } from "../../testI18n";
import type { DeleteServerResult } from "../../api/types";
import { useServerDelete } from "./useServerDelete";

vi.mock("../../api/api", async () =>
  (await import("../../test/apiMock")).createApiMock({
    deleteServer: vi.fn(),
  }),
);

const wrapper = ({ children }: { children: ReactNode }) => (
  <I18nextProvider i18n={testI18n}>{children}</I18nextProvider>
);

function setup() {
  const setError = vi.fn();
  const onDeleted = vi.fn();
  const hook = renderHook(() => useServerDelete({ serverId: "s1", setError, onDeleted }), { wrapper });
  return { setError, onDeleted, ...hook };
}

describe("useServerDelete", () => {
  beforeEach(() => vi.clearAllMocks());

  it("loads the preview first and deletes only after confirmation", async () => {
    const preview = { secretsLeftBehind: [] } as unknown as DeleteServerResult;
    vi.mocked(deleteServer).mockResolvedValue(preview);
    const { result, onDeleted } = setup();
    await act(() => result.current.handleDeleteClick());
    expect(deleteServer).toHaveBeenCalledWith("s1", false);
    expect(result.current.deletePreview).toBe(preview);
    expect(onDeleted).not.toHaveBeenCalled();

    await act(() => result.current.handleConfirmDelete());
    expect(deleteServer).toHaveBeenLastCalledWith("s1", true);
    expect(onDeleted).toHaveBeenCalledTimes(1);
    expect(result.current.deleting).toBe(false);
  });

  it("keeps the form open and lists leftover secrets", async () => {
    vi.mocked(deleteServer).mockResolvedValue({ secretsLeftBehind: ["k1"] } as unknown as DeleteServerResult);
    const { result, onDeleted } = setup();
    await act(() => result.current.handleConfirmDelete());
    expect(result.current.secretsLeftBehind).toEqual(["k1"]);
    expect(onDeleted).not.toHaveBeenCalled();
  });

  it("reports a delete error and does not call onDeleted", async () => {
    vi.mocked(deleteServer).mockRejectedValue({ code: "X", message: "nope" });
    const { result, setError, onDeleted } = setup();
    await act(() => result.current.handleConfirmDelete());
    expect(setError.mock.lastCall?.[0]).toEqual(expect.any(String));
    expect(onDeleted).not.toHaveBeenCalled();
    expect(result.current.deleting).toBe(false);
  });
});
