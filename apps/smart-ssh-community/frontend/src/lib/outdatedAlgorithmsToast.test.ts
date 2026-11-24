import { describe, expect, it, vi } from "vitest";
import { outdatedAlgorithmsMessage, showOutdatedAlgorithmsToast } from "./outdatedAlgorithmsToast";
import { showToast } from "../buses/toastBus";

vi.mock("../buses/toastBus", () => ({ showToast: vi.fn() }));

const t = (key: string, o?: Record<string, unknown>) => `${key}|${JSON.stringify(o)}`;

describe("outdated algorithms toast", () => {
  it("shows an info toast when there are warnings", () => {
    showOutdatedAlgorithmsToast([{ hop: "a", algorithms: ["ssh-rsa"] }]);
    expect(showToast).toHaveBeenCalledWith(expect.objectContaining({ kind: "info" }));
  });
  it("shows nothing without warnings", () => {
    vi.mocked(showToast).mockClear();
    showOutdatedAlgorithmsToast([]);
    expect(showToast).not.toHaveBeenCalled();
  });
  it("names the hop for chains", () => {
    const m = outdatedAlgorithmsMessage(t as never, [
      { hop: "a", algorithms: ["x"] },
      { hop: "b", algorithms: ["y", "z"] },
    ]);
    expect(m).toContain("outdatedAlgorithmsHop");
    expect(m).toContain('"hop":"b","algorithms":"y, z"');
  });
});
