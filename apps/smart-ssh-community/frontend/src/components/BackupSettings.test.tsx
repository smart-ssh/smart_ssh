// Issue #666, Spec 0109 (K2, K5, I3, I8, T4; tests 14, 17, 18): the "Data and
// backup" section — code display and confirmation, passphrase validation,
// counts confirmation, summary and the narrow layout.
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { afterAll, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import * as api from "../api";
import { testI18n } from "../testI18n";
import { BackupSettings } from "./BackupSettings";

vi.mock("../api", () => ({
  backupKeyStatus: vi.fn(),
  createBackupKeyWithCode: vi.fn(),
  newBackupCode: vi.fn(),
  setBackupPassphrase: vi.fn(() => Promise.resolve()),
  exportEncryptedBackup: vi.fn(),
  pickBackupImportFile: vi.fn(),
  startBackupImport: vi.fn(),
  confirmBackupImport: vi.fn(),
  cancelBackupImport: vi.fn(() => Promise.resolve()),
  clipboardWriteText: vi.fn(() => Promise.resolve()),
  commandErrorMessage: (e: unknown) => (e as { message?: string }).message ?? String(e),
  commandErrorCode: (e: unknown) => (e as { code?: string | null }).code ?? null,
}));

const COUNTS = { servers: 12, groups: 2, rules: 30, notes: 8, aiProviders: 1 };
const FILE_COUNTS = { servers: 15, groups: 3, rules: 42, notes: 11, aiProviders: 2 };
const EXPORT = { path: "/tmp/b.sshbackup", servers: 12, groups: 2, rules: 30, notes: 8, aiProviders: 1, secrets: 3, hostKeys: 1 };

function renderSection(onRestored = vi.fn()) {
  render(
    <I18nextProvider i18n={testI18n}>
      <BackupSettings onRestored={onRestored} />
    </I18nextProvider>,
  );
  return onRestored;
}

beforeAll(() => testI18n.changeLanguage("en"));
afterAll(() => testI18n.changeLanguage("de"));
beforeEach(() => {
  vi.clearAllMocks();
  localStorage.clear();
  vi.mocked(api.backupKeyStatus).mockResolvedValue({ exists: false, hasCode: false, hasPassphrase: false });
  vi.mocked(api.createBackupKeyWithCode).mockResolvedValue("ABCDE-FGHJK-MNPQR");
  vi.mocked(api.exportEncryptedBackup).mockResolvedValue(EXPORT);
});

describe("BackupSettings", () => {
  it("lists what is included and what is not (T4) and has min-height buttons for touch", () => {
    renderSection();
    expect(screen.getByText(/Included: servers/)).toBeTruthy();
    expect(screen.getByText(/Not included: chat history/)).toBeTruthy();
    for (const name of ["Export backup…", "Restore from backup…", "New recovery code", "Set passphrase"]) {
      expect(screen.getByRole("button", { name }).className).toContain("narrow:min-h-11");
    }
  });

  it("first export: code in groups, Continue off until confirmed, no export before (test 18)", async () => {
    renderSection();
    fireEvent.click(screen.getByRole("button", { name: "Export backup…" }));
    fireEvent.click(await screen.findByRole("button", { name: "Create a recovery code" }));
    const next = await screen.findByRole("button", { name: "Continue" });
    expect(screen.getByTestId("backup-recovery-code").textContent).toBe("ABCDEFGHJKMNPQR");
    expect(screen.getAllByText(/^[A-Z]{5}$/)).toHaveLength(3);
    expect((next as HTMLButtonElement).disabled).toBe(true);
    fireEvent.click(next);
    expect(api.exportEncryptedBackup).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("checkbox", { name: "I have noted the code" }));
    expect((next as HTMLButtonElement).disabled).toBe(false);
    fireEvent.click(next);
    await waitFor(() => expect(api.exportEncryptedBackup).toHaveBeenCalledTimes(1));
    expect(await screen.findByText(/Backup saved to \/tmp\/b.sshbackup/)).toBeTruthy();
    expect(screen.queryByTestId("backup-recovery-code")).toBeNull();
  });

  it("leaving the code dialog unconfirmed makes the next export ask for a new code", async () => {
    renderSection();
    fireEvent.click(screen.getByRole("button", { name: "Export backup…" }));
    fireEvent.click(await screen.findByRole("button", { name: "Create a recovery code" }));
    fireEvent.click(await screen.findByRole("button", { name: "Cancel" }));
    expect(api.exportEncryptedBackup).not.toHaveBeenCalled();

    vi.mocked(api.backupKeyStatus).mockResolvedValue({ exists: true, hasCode: true, hasPassphrase: false });
    vi.mocked(api.newBackupCode).mockResolvedValue("23456-789AB-CDEFG");
    fireEvent.click(screen.getByRole("button", { name: "Export backup…" }));
    await screen.findByTestId("backup-recovery-code");
    expect(api.newBackupCode).toHaveBeenCalled();
  });

  it("with a key the export needs no input and never shows the code again", async () => {
    vi.mocked(api.backupKeyStatus).mockResolvedValue({ exists: true, hasCode: true, hasPassphrase: false });
    renderSection();
    fireEvent.click(screen.getByRole("button", { name: "Export backup…" }));
    await waitFor(() => expect(api.exportEncryptedBackup).toHaveBeenCalled());
    expect(screen.queryByTestId("backup-recovery-code")).toBeNull();
    expect(api.createBackupKeyWithCode).not.toHaveBeenCalled();
  });

  it("passphrase: rejects < 12 characters and mismatch, warns, clears after saving (K5)", async () => {
    renderSection();
    fireEvent.click(screen.getByRole("button", { name: "Set passphrase" }));
    expect(await screen.findByText(/Without the passphrase, your backups cannot be read/)).toBeTruthy();
    const first = screen.getByLabelText("Passphrase") as HTMLInputElement;
    const second = screen.getByLabelText("Repeat passphrase") as HTMLInputElement;
    const save = screen.getByRole("button", { name: "Save passphrase" }) as HTMLButtonElement;

    fireEvent.change(first, { target: { value: "elevenchars" } });
    fireEvent.change(second, { target: { value: "elevenchars" } });
    expect(screen.getByText(/Too short/)).toBeTruthy();
    expect(save.disabled).toBe(true);

    fireEvent.change(first, { target: { value: "twelve chars" } });
    fireEvent.change(second, { target: { value: "twelve chars!" } });
    expect(screen.getByText("The passphrases do not match.")).toBeTruthy();
    expect(save.disabled).toBe(true);

    fireEvent.change(second, { target: { value: "twelve chars" } });
    expect(save.disabled).toBe(false);
    fireEvent.click(save);
    await waitFor(() => expect(api.setBackupPassphrase).toHaveBeenCalledWith("twelve chars"));
  });

  describe("restore", () => {
    beforeEach(() => {
      vi.mocked(api.pickBackupImportFile).mockResolvedValue("b.sshbackup");
    });
    const open = async (secret = "ABCDE-FGHJK") => {
      fireEvent.click(screen.getByRole("button", { name: "Restore from backup…" }));
      fireEvent.change(await screen.findByRole("textbox", { name: "Recovery code" }), { target: { value: secret } });
      fireEvent.click(screen.getByRole("button", { name: "Open backup" }));
    };

    it("shows the typo and the generic message without naming a cause", async () => {
      renderSection();
      vi.mocked(api.startBackupImport).mockRejectedValueOnce({ message: "x", code: "BACKUP_CODE_TYPO" });
      await open("TYPO");
      expect(await screen.findByText("The code contains a typo.")).toBeTruthy();
      vi.mocked(api.startBackupImport).mockRejectedValueOnce({ message: "x", code: "BACKUP_CANNOT_OPEN" });
      fireEvent.change(screen.getByRole("textbox", { name: "Recovery code" }), { target: { value: "ABCDE-FGHJK" } });
      fireEvent.click(screen.getByRole("button", { name: "Open backup" }));
      expect(await screen.findByText("Wrong code or passphrase, or the file is damaged.")).toBeTruthy();
    });

    it("sends the code once and clears the field", async () => {
      renderSection();
      vi.mocked(api.startBackupImport).mockRejectedValueOnce({ message: "x", code: "BACKUP_CANNOT_OPEN" });
      await open("SECRET-CODE");
      await screen.findByText(/Wrong code/);
      expect(api.startBackupImport).toHaveBeenCalledWith("code", "SECRET-CODE");
      expect((screen.getByRole("textbox", { name: "Recovery code" }) as HTMLInputElement).value).toBe("");
    });

    it("counts first; cancel changes nothing; confirm shows the summary and reloads", async () => {
      vi.mocked(api.startBackupImport).mockResolvedValue({
        imported: null,
        preview: { local: COUNTS, file: FILE_COUNTS, openSessions: 2, safetyBackupDir: "/docs/Smart SSH backups" },
      });
      vi.mocked(api.backupKeyStatus).mockResolvedValue({ exists: true, hasCode: true, hasPassphrase: false });
      vi.mocked(api.confirmBackupImport).mockResolvedValue({
        counts: FILE_COUNTS,
        safetyBackup: "/docs/Smart SSH backups/before.sshbackup",
        previousDataFolder: "Previous data 2026-01-01",
        hostKeysPending: false,
      });
      const restored = renderSection();
      await open();
      const table = await screen.findByRole("table");
      const rows = Array.from(table.querySelectorAll("tbody tr")).map((r) => r.textContent);
      expect(rows).toContain("Servers1215");
      expect(rows).toContain("Rules3042");
      expect(screen.getByText("Open sessions that will be closed: 2")).toBeTruthy();
      expect(api.confirmBackupImport).not.toHaveBeenCalled();

      fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
      expect(api.cancelBackupImport).toHaveBeenCalled();
      expect(api.confirmBackupImport).not.toHaveBeenCalled();

      await open();
      fireEvent.click(await screen.findByRole("button", { name: "Replace data" }));
      expect(await screen.findByText(/before\.sshbackup/)).toBeTruthy();
      expect(screen.getByText(/Previous data 2026-01-01/)).toBeTruthy();
      expect(restored).not.toHaveBeenCalled();
      fireEvent.click(screen.getByRole("button", { name: "Done" }));
      expect(restored).toHaveBeenCalledTimes(1);
    });

    it("an empty device imports directly and shows the summary", async () => {
      vi.mocked(api.startBackupImport).mockResolvedValue({
        imported: { counts: FILE_COUNTS, safetyBackup: null, previousDataFolder: null, hostKeysPending: false },
        preview: null,
      });
      renderSection();
      await open();
      expect(await screen.findByText("Backup restored")).toBeTruthy();
      expect(api.confirmBackupImport).not.toHaveBeenCalled();
    });

    it("keeps a failed file choice visible across code/passphrase and re-opens the picker", async () => {
      renderSection();
      vi.mocked(api.pickBackupImportFile).mockRejectedValueOnce({ message: "no path" });
      fireEvent.click(screen.getByRole("button", { name: "Restore from backup…" }));
      const msg = "The file could not be opened from the chosen location.";
      expect(await screen.findByText(msg)).toBeTruthy();
      fireEvent.click(screen.getByRole("radio", { name: "Passphrase" }));
      fireEvent.click(screen.getByRole("radio", { name: "Recovery code" }));
      expect(screen.getByText(msg)).toBeTruthy();
      expect((screen.getByRole("button", { name: "Open backup" }) as HTMLButtonElement).disabled).toBe(true);
      fireEvent.click(screen.getByRole("button", { name: "Choose file again" }));
      await waitFor(() => expect(screen.queryByText(msg)).toBeNull());
      expect(api.pickBackupImportFile).toHaveBeenCalledTimes(2);
      expect(await screen.findByText("File: b.sshbackup")).toBeTruthy();
    });
  });
});
