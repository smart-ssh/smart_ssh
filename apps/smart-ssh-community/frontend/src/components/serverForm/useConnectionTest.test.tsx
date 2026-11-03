import { act, renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { I18nextProvider } from "react-i18next";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { testConnection, trustHostKey } from "../../api";
import { testI18n } from "../../testI18n";
import type { ServerInput, TestConnectionReport } from "../../types";
import { useConnectionTest } from "./useConnectionTest";

vi.mock("../../api", () => ({
  cancelConnect: vi.fn(() => Promise.resolve()),
  commandErrorCode: (err: unknown) =>
    typeof err === "object" && err !== null && "code" in err ? (err as { code: string }).code : null,
  commandErrorMessage: (err: unknown) =>
    typeof err === "object" && err !== null && "message" in err ? String((err as { message: unknown }).message) : String(err),
  testConnection: vi.fn(),
  trustHostKey: vi.fn(),
}));
vi.mock("../../events", () => ({
  onConnectProgress: vi.fn(() => Promise.resolve(() => {})),
}));

const wrapper = ({ children }: { children: ReactNode }) => (
  <I18nextProvider i18n={testI18n}>{children}</I18nextProvider>
);
const input = { name: "x" } as unknown as ServerInput;
const unknownKey: TestConnectionReport = {
  kind: "hostKeyUnknown",
  host: "h",
  port: 22,
  fingerprint: "SHA256:abc",
  keyType: "ssh-ed25519",
  rawKey: "RAW",
} as unknown as TestConnectionReport;

function setup() {
  const setError = vi.fn();
  const hook = renderHook(() => useConnectionTest({ serverId: "s1", buildInput: () => input, setError }), {
    wrapper,
  });
  return { setError, ...hook };
}

describe("useConnectionTest", () => {
  beforeEach(() => vi.clearAllMocks());

  it("stores the result on success and ends the test", async () => {
    vi.mocked(testConnection).mockResolvedValue({ kind: "success" } as unknown as TestConnectionReport);
    const { result } = setup();
    await act(() => result.current.runTest());
    expect(result.current.testResult).toMatchObject({ kind: "success" });
    expect(result.current.testing).toBe(false);
    expect(testConnection).toHaveBeenCalledWith(input, expect.any(String), "s1");
  });

  it("reports a failure through setError and closes the dialog", async () => {
    vi.mocked(testConnection).mockRejectedValue({ code: "SOME_ERROR", message: "boom" });
    const { result, setError } = setup();
    await act(() => result.current.runTest());
    expect(setError).toHaveBeenLastCalledWith(expect.any(String));
    expect(setError.mock.lastCall?.[0]).not.toBeNull();
    expect(result.current.testDialog).toBeNull();
    expect(result.current.testing).toBe(false);
  });

  it("asks for the host key, trusts it only on request and retests", async () => {
    vi.mocked(testConnection).mockResolvedValueOnce(unknownKey);
    vi.mocked(trustHostKey).mockResolvedValue(undefined as never);
    const { result } = setup();
    await act(() => result.current.runTest());
    expect(result.current.pendingHostKey).toMatchObject({ host: "h", kind: "unknown", fingerprint: "SHA256:abc" });
    expect(trustHostKey).not.toHaveBeenCalled();

    vi.mocked(testConnection).mockResolvedValueOnce({ kind: "success" } as unknown as TestConnectionReport);
    await act(() => result.current.handleTrustAndRetest());
    expect(trustHostKey).toHaveBeenCalledWith("h", 22, "RAW");
    await waitFor(() => expect(result.current.testResult).toMatchObject({ kind: "success" }));
    expect(result.current.pendingHostKey).toBeNull();
    expect(testConnection).toHaveBeenCalledTimes(2);
  });
});
