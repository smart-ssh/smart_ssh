// Spec 0054, Teil 4 ("Testbarkeit"): "Download -> öffnen -> lokale
// Änderung erkannt -> Upload angeboten -> Diff + Konflikt-Prüfung ->
// hochgeladen; Watcher sauber beendet; Temp aufgeräumt."
import { act, renderHook } from "@testing-library/react";
import { openPath } from "@tauri-apps/plugin-opener";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  closeEditSession,
  localFileMtime,
  readLocalTextPreview,
  sftpOpenForEditing,
  sftpReadText,
  sftpStat,
  sftpUpload,
} from "./api";
import { useLocalEditSession } from "./useLocalEditSession";

vi.mock("@tauri-apps/plugin-opener", () => ({
  openPath: vi.fn(),
}));

vi.mock("./api", () => ({
  commandErrorMessage: (err: unknown) => String(err),
  sftpOpenForEditing: vi.fn(),
  localFileMtime: vi.fn(),
  closeEditSession: vi.fn(() => Promise.resolve()),
  readLocalTextPreview: vi.fn(),
  sftpStat: vi.fn(),
  sftpReadText: vi.fn(),
  sftpUpload: vi.fn(),
}));

vi.mock("./fileTypeSettings", () => ({
  loadFileTypeApps: vi.fn(() => Promise.resolve({})),
  appForFileName: () => null,
}));

const entry = {
  name: "nginx.conf",
  path: "/etc/nginx.conf",
  isDir: false,
  size: 100,
  permissions: "rw-r--r--",
  modified: "2026-01-01T00:00:00Z",
  permissionsOctal: 0o644,
  uid: null,
  gid: null,
  owner: null,
  group: null,
};

beforeEach(() => {
  vi.clearAllMocks();
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
});

describe("useLocalEditSession", () => {
  it("downloads, opens the file, and starts in 'editing' status", async () => {
    vi.mocked(sftpOpenForEditing).mockResolvedValue({
      localPath: "/tmp/edit/nginx.conf",
      remoteModified: "2026-01-01T00:00:00Z",
    });
    vi.mocked(localFileMtime).mockResolvedValue("2026-01-01T00:00:01Z");

    const { result } = renderHook(() => useLocalEditSession("session-1"));
    await act(async () => {
      await result.current.startEditing(entry);
    });

    expect(sftpOpenForEditing).toHaveBeenCalledWith("session-1", "/etc/nginx.conf");
    expect(result.current.session).toEqual(
      expect.objectContaining({ status: "editing", localPath: "/tmp/edit/nginx.conf" }),
    );
  });

  it("detects a local change via polling and switches to 'changed'", async () => {
    vi.mocked(sftpOpenForEditing).mockResolvedValue({
      localPath: "/tmp/edit/nginx.conf",
      remoteModified: "2026-01-01T00:00:00Z",
    });
    vi.mocked(localFileMtime).mockResolvedValueOnce("2026-01-01T00:00:01Z"); // initial

    const { result } = renderHook(() => useLocalEditSession("session-1"));
    await act(async () => {
      await result.current.startEditing(entry);
    });
    expect(result.current.session?.status).toBe("editing");

    vi.mocked(localFileMtime).mockResolvedValue("2026-01-01T00:05:00Z"); // geändert
    await act(async () => {
      await vi.advanceTimersByTimeAsync(2000);
    });

    expect(result.current.session?.status).toBe("changed");
  });

  it("does not flag a change when the mtime is unchanged", async () => {
    vi.mocked(sftpOpenForEditing).mockResolvedValue({
      localPath: "/tmp/edit/nginx.conf",
      remoteModified: "2026-01-01T00:00:00Z",
    });
    vi.mocked(localFileMtime).mockResolvedValue("2026-01-01T00:00:01Z");

    const { result } = renderHook(() => useLocalEditSession("session-1"));
    await act(async () => {
      await result.current.startEditing(entry);
    });

    await act(async () => {
      await vi.advanceTimersByTimeAsync(6000);
    });

    expect(result.current.session?.status).toBe("editing");
  });

  it("buildUploadOffer flags a conflict when the remote file changed since download", async () => {
    vi.mocked(sftpOpenForEditing).mockResolvedValue({
      localPath: "/tmp/edit/nginx.conf",
      remoteModified: "2026-01-01T00:00:00Z",
    });
    vi.mocked(localFileMtime).mockResolvedValue("2026-01-01T00:00:01Z");
    vi.mocked(readLocalTextPreview).mockResolvedValue({ text: "neu", size: 3 });
    vi.mocked(sftpStat).mockResolvedValue({ ...entry, modified: "2026-01-02T00:00:00Z" });
    vi.mocked(sftpReadText).mockResolvedValue("alt");

    const { result } = renderHook(() => useLocalEditSession("session-1"));
    await act(async () => {
      await result.current.startEditing(entry);
    });

    const offer = await act(async () => result.current.buildUploadOffer());

    expect(offer).toEqual({
      localText: "neu",
      localSize: 3,
      remoteText: "alt",
      remoteChangedSinceDownload: true,
    });
  });

  it("buildUploadOffer conservatively flags a conflict when both timestamps are null", async () => {
    // Spec-Reviewer-Fund (Spec 0054, Review des Gesamtpakets): weder der
    // Server liefert einen `modified`-Zeitstempel, noch konnte der
    // aktuelle `sftpStat`-Aufruf einen liefern (z. B. weil er fehlschlug)
    // — in diesem Fall lässt sich gar nichts prüfen, die Spec verlangt
    // trotzdem konservativ eine Warnung statt eines stillen "unverändert".
    vi.mocked(sftpOpenForEditing).mockResolvedValue({
      localPath: "/tmp/edit/nginx.conf",
      remoteModified: null,
    });
    vi.mocked(localFileMtime).mockResolvedValue("2026-01-01T00:00:01Z");
    vi.mocked(readLocalTextPreview).mockResolvedValue({ text: "neu", size: 3 });
    vi.mocked(sftpStat).mockRejectedValue(new Error("stat failed"));
    vi.mocked(sftpReadText).mockResolvedValue("alt");

    const { result } = renderHook(() => useLocalEditSession("session-1"));
    await act(async () => {
      await result.current.startEditing({ ...entry, modified: null });
    });

    const offer = await act(async () => result.current.buildUploadOffer());

    expect(offer?.remoteChangedSinceDownload).toBe(true);
  });

  it("buildUploadOffer reports no conflict when the remote mtime is unchanged", async () => {
    vi.mocked(sftpOpenForEditing).mockResolvedValue({
      localPath: "/tmp/edit/nginx.conf",
      remoteModified: "2026-01-01T00:00:00Z",
    });
    vi.mocked(localFileMtime).mockResolvedValue("2026-01-01T00:00:01Z");
    vi.mocked(readLocalTextPreview).mockResolvedValue({ text: "neu", size: 3 });
    vi.mocked(sftpStat).mockResolvedValue({ ...entry, modified: "2026-01-01T00:00:00Z" });
    vi.mocked(sftpReadText).mockResolvedValue("alt");

    const { result } = renderHook(() => useLocalEditSession("session-1"));
    await act(async () => {
      await result.current.startEditing(entry);
    });

    const offer = await act(async () => result.current.buildUploadOffer());

    expect(offer?.remoteChangedSinceDownload).toBe(false);
  });

  it("confirmUpload uploads the local file and returns to 'editing'", async () => {
    vi.mocked(sftpOpenForEditing).mockResolvedValue({
      localPath: "/tmp/edit/nginx.conf",
      remoteModified: "2026-01-01T00:00:00Z",
    });
    vi.mocked(localFileMtime).mockResolvedValue("2026-01-01T00:00:01Z");
    vi.mocked(sftpUpload).mockResolvedValue(undefined);
    vi.mocked(sftpStat).mockResolvedValue({ ...entry, modified: "2026-01-01T00:10:00Z" });

    const { result } = renderHook(() => useLocalEditSession("session-1"));
    await act(async () => {
      await result.current.startEditing(entry);
    });

    await act(async () => {
      await result.current.confirmUpload();
    });

    expect(sftpUpload).toHaveBeenCalledWith(
      "session-1",
      "/tmp/edit/nginx.conf",
      "/etc/nginx.conf",
    );
    expect(result.current.session?.status).toBe("editing");
  });

  it("cleans up the downloaded temp file if opening it locally fails", async () => {
    // Spec-Reviewer-Fund (Spec 0054, Review des Gesamtpakets): ein
    // Fehlschlag NACH dem erfolgreichen Download (hier: `openPath` findet
    // kein passendes Programm) darf die bereits heruntergeladene Kopie
    // nicht als Leiche zurücklassen — ohne gesetzten Session-State gäbe es
    // sonst keinen Weg mehr, sie aufzuräumen.
    vi.mocked(sftpOpenForEditing).mockResolvedValue({
      localPath: "/tmp/edit/nginx.conf",
      remoteModified: "2026-01-01T00:00:00Z",
    });
    vi.mocked(openPath).mockRejectedValueOnce(new Error("no application found"));

    const { result } = renderHook(() => useLocalEditSession("session-1"));

    // Der Fehler wird INNERHALB von `act()` gefangen (statt über
    // `expect(act(...)).rejects...`) — nur so ist garantiert, dass der
    // catch-Zweig in `startEditing` (der `closeEditSession` aufruft)
    // tatsächlich vollständig durchgelaufen ist, bevor die folgenden
    // Assertions laufen.
    let thrown: unknown;
    await act(async () => {
      try {
        await result.current.startEditing(entry);
      } catch (err) {
        thrown = err;
      }
    });

    expect(thrown).toBeInstanceOf(Error);
    expect((thrown as Error).message).toBe("no application found");
    expect(closeEditSession).toHaveBeenCalledWith("session-1", "/tmp/edit/nginx.conf");
    expect(result.current.session).toBeNull();
  });

  it("endSession stops polling and cleans up the temp file", async () => {
    vi.mocked(sftpOpenForEditing).mockResolvedValue({
      localPath: "/tmp/edit/nginx.conf",
      remoteModified: "2026-01-01T00:00:00Z",
    });
    vi.mocked(localFileMtime).mockResolvedValue("2026-01-01T00:00:01Z");

    const { result } = renderHook(() => useLocalEditSession("session-1"));
    await act(async () => {
      await result.current.startEditing(entry);
    });

    act(() => {
      result.current.endSession();
    });

    expect(closeEditSession).toHaveBeenCalledWith("session-1", "/tmp/edit/nginx.conf");
    expect(result.current.session).toBeNull();

    // Nach dem Ende darf kein weiteres Polling mehr passieren (Watcher
    // sauber beendet, Spec 0054 Teil 4 Punkt 6) — ein Timer-Tick danach
    // darf `localFileMtime` nicht erneut aufrufen.
    vi.mocked(localFileMtime).mockClear();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(10_000);
    });
    expect(localFileMtime).not.toHaveBeenCalled();
  });
});
