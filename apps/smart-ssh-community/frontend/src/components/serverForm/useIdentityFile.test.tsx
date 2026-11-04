import { act, renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { I18nextProvider } from "react-i18next";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { convertIdentityFileToKeychain, inspectKeyFile } from "../../api";
import { testI18n } from "../../testI18n";
import type { KeyFileFactsDto, ServerDto } from "../../types";
import { useIdentityFile } from "./useIdentityFile";

vi.mock("../../api", async () =>
  (await import("../../test/apiMock")).createApiMock({
    convertIdentityFileToKeychain: vi.fn(),
    inspectKeyFile: vi.fn(),
  }),
);

const wrapper = ({ children }: { children: ReactNode }) => (
  <I18nextProvider i18n={testI18n}>{children}</I18nextProvider>
);
const facts = { exists: true, validKey: true, problem: null } as unknown as KeyFileFactsDto;
const loadedServer = { id: "s1", authKind: "identity_file", identityFilePath: "/k" } as unknown as ServerDto;

function setup(props: { draftPath: string; loaded: ServerDto | null }) {
  const reloadServer = vi.fn();
  const onSaved = vi.fn();
  const hook = renderHook(
    () =>
      useIdentityFile({
        serverId: "s1",
        draftIsIdentityFile: true,
        draftPath: props.draftPath,
        loaded: props.loaded,
        reloadServer,
        onSaved,
        onKeepPathFinished: vi.fn(),
      }),
    { wrapper },
  );
  return { reloadServer, onSaved, ...hook };
}

describe("useIdentityFile", () => {
  beforeEach(() => vi.clearAllMocks());

  it("checks the draft path after the debounce", async () => {
    vi.mocked(inspectKeyFile).mockResolvedValue(facts);
    const { result } = setup({ draftPath: "/k", loaded: null });
    await waitFor(() => expect(result.current.identityFacts).toBe(facts));
    expect(result.current.identityFactsLoading).toBe(false);
    expect(result.current.identityFactsError).toBe(false);
  });

  it("flags a failed check instead of staying silent", async () => {
    vi.mocked(inspectKeyFile).mockRejectedValue(new Error("ipc"));
    const { result } = setup({ draftPath: "/k", loaded: null });
    await waitFor(() => expect(result.current.identityFactsError).toBe(true));
    expect(result.current.identityFacts).toBeNull();
  });

  it("converts to the keychain, reloads and notifies on success", async () => {
    vi.mocked(inspectKeyFile).mockResolvedValue(facts);
    vi.mocked(convertIdentityFileToKeychain).mockResolvedValue(undefined as never);
    const { result, reloadServer, onSaved } = setup({ draftPath: "", loaded: loadedServer });
    await waitFor(() => expect(result.current.convertFacts).toBe(facts));
    act(() => result.current.setConvertConfirmOpen(true));
    await act(() => result.current.handleConvertToKeychain());
    expect(convertIdentityFileToKeychain).toHaveBeenCalledWith("s1");
    expect(reloadServer).toHaveBeenCalledTimes(1);
    expect(onSaved).toHaveBeenCalledTimes(1);
    expect(result.current.convertConfirmOpen).toBe(false);
    expect(result.current.converting).toBe(false);
  });

  it("keeps the confirmation open and shows the error when conversion fails", async () => {
    vi.mocked(inspectKeyFile).mockResolvedValue(facts);
    vi.mocked(convertIdentityFileToKeychain).mockRejectedValue({ code: "X", message: "bad" });
    const { result, reloadServer, onSaved } = setup({ draftPath: "", loaded: loadedServer });
    act(() => result.current.setConvertConfirmOpen(true));
    await act(() => result.current.handleConvertToKeychain());
    expect(result.current.convertError).toEqual(expect.any(String));
    expect(result.current.convertConfirmOpen).toBe(true);
    expect(reloadServer).not.toHaveBeenCalled();
    expect(onSaved).not.toHaveBeenCalled();
  });
});
