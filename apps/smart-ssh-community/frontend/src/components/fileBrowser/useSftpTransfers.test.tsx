import { act, renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { I18nextProvider } from "react-i18next";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { sftpExists, sftpReadText, readLocalTextPreview, sftpUpload } from "../../api/api";
import {
  onSftpTransferFinished,
  onSftpTransferProgress,
  onSftpTransferStarted,
} from "../../api/events";
import { testI18n } from "../../testI18n";
import type {
  SftpTransferFinishedEvent,
  SftpTransferProgressEvent,
  SftpTransferStartedEvent,
} from "../../api/types";
import { useSftpTransfers } from "./useSftpTransfers";

vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({ onDragDropEvent: vi.fn(() => Promise.resolve(() => {})) }),
}));
vi.mock("../../api/api", async () =>
  (await import("../../test/apiMock")).createApiMock({
    claimDroppedPaths: vi.fn(),
    pickUploadFiles: vi.fn(),
    pickUploadFolder: vi.fn(),
    readLocalTextPreview: vi.fn(),
    sftpExists: vi.fn(),
    sftpLocalPathsAreFolders: vi.fn(),
    sftpReadText: vi.fn(),
    sftpUpload: vi.fn(),
    sftpUploadFolder: vi.fn(),
    sftpUploadFolderPreview: vi.fn(),
  }),
);
vi.mock("../../api/events", () => ({
  onSftpTransferStarted: vi.fn(),
  onSftpTransferProgress: vi.fn(),
  onSftpTransferFinished: vi.fn(),
}));
vi.mock("../../buses/toastBus", () => ({ showToast: vi.fn() }));

type Handler<T> = (event: T) => void;
let started: Handler<SftpTransferStartedEvent>;
let progress: Handler<SftpTransferProgressEvent>;
let finished: Handler<SftpTransferFinishedEvent>;
const unlisten = vi.fn();

const wrapper = ({ children }: { children: ReactNode }) => (
  <I18nextProvider i18n={testI18n}>{children}</I18nextProvider>
);

const load = vi.fn();
const notifyOk = vi.fn();
const notifyFailed = vi.fn();

function setup(channelUser: string | null = null) {
  return renderHook(
    () =>
      useSftpTransfers("s1", {
        isVisible: false,
        panelRef: { current: null },
        channelUser,
        path: "/home/u",
        pathRef: { current: "/home/u" },
        load,
        entries: [],
        notifyOk,
        notifyFailed,
        modeSuffix: "",
      }),
    { wrapper },
  );
}

const start = (sessionId: string, transferId: string) =>
  act(() =>
    started({ sessionId, transferId, kind: "upload", fileName: `${transferId}.txt`, totalBytes: 100 }),
  );

describe("useSftpTransfers", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(onSftpTransferStarted).mockImplementation((h) => {
      started = h;
      return Promise.resolve(unlisten);
    });
    vi.mocked(onSftpTransferProgress).mockImplementation((h) => {
      progress = h;
      return Promise.resolve(unlisten);
    });
    vi.mocked(onSftpTransferFinished).mockImplementation((h) => {
      finished = h;
      return Promise.resolve(unlisten);
    });
  });

  it("applies a progress event to the matching transfer only", () => {
    const { result } = setup();
    start("s1", "a");
    start("s1", "b");
    act(() => progress({ sessionId: "s1", transferId: "b", bytesDone: 40, totalBytes: 100 }));
    const [a, b] = result.current.transfers;
    expect(a.bytesDone).toBeUndefined();
    expect(b.bytesDone).toBe(40);
  });

  it("ignores events of another session", () => {
    const { result } = setup();
    start("other", "x");
    start("s1", "a");
    act(() => progress({ sessionId: "other", transferId: "a", bytesDone: 9, totalBytes: 100 }));
    expect(result.current.transfers.map((t) => t.id)).toEqual(["a"]);
    expect(result.current.transfers[0].bytesDone).toBeUndefined();
  });

  it("marks a cancelled transfer as stopped and keeps its row", () => {
    const { result } = setup();
    start("s1", "a");
    act(() => finished({ sessionId: "s1", transferId: "a", error: null, stopped: "cancelled" }));
    expect(result.current.transfers[0].stopped).toBe("cancelled");
    expect(load).not.toHaveBeenCalled();
  });

  it("removes a finished transfer, reloads, and dismisses rows on request", () => {
    const { result } = setup();
    start("s1", "a");
    start("s1", "b");
    act(() => finished({ sessionId: "s1", transferId: "a", error: null }));
    expect(result.current.transfers.map((t) => t.id)).toEqual(["b"]);
    expect(load).toHaveBeenCalledWith("/home/u");
    act(() => result.current.dismissTransfer("b"));
    expect(result.current.transfers).toEqual([]);
  });

  it("keeps a failed transfer with its error", () => {
    const { result } = setup();
    start("s1", "a");
    act(() => finished({ sessionId: "s1", transferId: "a", error: "boom" }));
    expect(result.current.transfers[0].error).toBe("boom");
  });

  it("unsubscribes the three events on unmount", async () => {
    const { unmount } = setup();
    unmount();
    await waitFor(() => expect(unlisten).toHaveBeenCalledTimes(3));
  });

  it("opens a conflict for an existing target and resolves it with an upload", async () => {
    vi.mocked(sftpExists).mockResolvedValue(true);
    vi.mocked(readLocalTextPreview).mockResolvedValue({} as never);
    vi.mocked(sftpReadText).mockResolvedValue("remote");
    vi.mocked(sftpUpload).mockResolvedValue(undefined as never);
    const { result } = setup();
    const { sftpLocalPathsAreFolders } = await import("../../api/api");
    vi.mocked(sftpLocalPathsAreFolders).mockResolvedValue([false]);
    await act(() => result.current.uploadMany(["/tmp/f.txt"]));
    expect(result.current.uploadConflict?.remotePath).toBe("/home/u/f.txt");
    expect(sftpUpload).not.toHaveBeenCalled();
    await act(async () => result.current.handleConfirmUpload());
    expect(result.current.uploadConflict).toBeNull();
    expect(sftpUpload).toHaveBeenCalledWith("s1", "/tmp/f.txt", "/home/u/f.txt", null);
  });

  it("cancelling the conflict does not upload", async () => {
    vi.mocked(sftpExists).mockResolvedValue(true);
    vi.mocked(readLocalTextPreview).mockResolvedValue({} as never);
    vi.mocked(sftpReadText).mockResolvedValue("remote");
    const { sftpLocalPathsAreFolders } = await import("../../api/api");
    vi.mocked(sftpLocalPathsAreFolders).mockResolvedValue([false]);
    const { result } = setup();
    await act(() => result.current.uploadMany(["/tmp/f.txt"]));
    act(() => result.current.setUploadConflict(null));
    expect(result.current.uploadConflict).toBeNull();
    expect(sftpUpload).not.toHaveBeenCalled();
  });
});
