import { renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { respondToAction } from "../../api/api";
import { useActionDecisions } from "./useActionDecisions";

vi.mock("../../api/api", async () =>
  (await import("../../test/apiMock")).createApiMock({
    respondToAction: vi.fn(),
    acceptAndCreateRule: vi.fn(),
  }),
);

describe("useActionDecisions", () => {
  beforeEach(() => {
    vi.mocked(respondToAction).mockReset().mockReturnValue(new Promise(() => {}));
  });

  it("respond sends the decision once and ignores a second click while pending", () => {
    const dispatch = vi.fn();
    const settled = vi.fn();
    const { result } = renderHook(() => useActionDecisions("s1", dispatch, settled));
    result.current.respond("a1", { decision: "approve" });
    result.current.respond("a1", { decision: "approve" });
    expect(respondToAction).toHaveBeenCalledTimes(1);
    expect(respondToAction).toHaveBeenCalledWith("s1", "a1", { decision: "approve" });
    expect(settled).toHaveBeenCalledTimes(1);
  });
});
