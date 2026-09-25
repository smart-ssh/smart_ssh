// Spec 0075, §3.2.5/§3.2.7: Export-Ergebnis-Dialog. Kein Vorschau-Dialog
// nötig (anders als der Import, §3.1.7) — die Spec verlangt hier nur eine
// Meldung nach dem Schreiben, mit Pfad, `Include`-Zeile und einer
// umbenannten-Alias-Liste, falls es welche gab.
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { exportSshConfig } from "../api";
import { testI18n } from "../testI18n";
import { SshConfigExportDialog } from "./SshConfigExportDialog";

vi.mock("../api", () => ({
  commandErrorMessage: (err: unknown) => String(err),
  exportSshConfig: vi.fn(),
}));

beforeEach(() => {
  vi.clearAllMocks();
});

describe("SshConfigExportDialog (Spec 0075, §3.2.5/§3.2.7)", () => {
  it("shows the path, the renamed aliases, and the Include hint", async () => {
    vi.mocked(exportSshConfig).mockResolvedValue({
      path: "/tmp/smart-ssh-export.conf",
      exported: [
        { originalName: "prod web", alias: "prod-web", renamed: true },
        { originalName: "web2", alias: "web2", renamed: false },
      ],
      includeHint: "Include /tmp/smart-ssh-export.conf",
    });
    const onClose = vi.fn();
    render(
      <I18nextProvider i18n={testI18n}>
        <SshConfigExportDialog onClose={onClose} />
      </I18nextProvider>,
    );

    expect(await screen.findByText(/2 Server exportiert/)).toBeInTheDocument();
    expect(screen.getByText(/prod web → prod-web/)).toBeInTheDocument();
    // Der unveränderte Server erscheint NICHT in der Umbenennungsliste.
    expect(screen.queryByText(/web2 → web2/)).not.toBeInTheDocument();
    expect(screen.getByText("Include /tmp/smart-ssh-export.conf")).toBeInTheDocument();
  });

  it("does not show the renamed-aliases section when nothing was renamed", async () => {
    vi.mocked(exportSshConfig).mockResolvedValue({
      path: "/tmp/x.conf",
      exported: [{ originalName: "web1", alias: "web1", renamed: false }],
      includeHint: "Include /tmp/x.conf",
    });
    render(
      <I18nextProvider i18n={testI18n}>
        <SshConfigExportDialog onClose={vi.fn()} />
      </I18nextProvider>,
    );

    await screen.findByText(/1 Server exportiert/);
    expect(screen.queryByText("Umbenannte Aliase (ungültige Zeichen im Servernamen):")).not.toBeInTheDocument();
  });

  it("closes without showing a dialog when the user cancels the native save dialog", async () => {
    vi.mocked(exportSshConfig).mockResolvedValue(null);
    const onClose = vi.fn();
    render(
      <I18nextProvider i18n={testI18n}>
        <SshConfigExportDialog onClose={onClose} />
      </I18nextProvider>,
    );

    await waitFor(() => expect(onClose).toHaveBeenCalled());
  });

  it("shows an error instead of crashing when export_ssh_config fails", async () => {
    vi.mocked(exportSshConfig).mockRejectedValue(new Error("disk full"));
    render(
      <I18nextProvider i18n={testI18n}>
        <SshConfigExportDialog onClose={vi.fn()} />
      </I18nextProvider>,
    );

    expect(await screen.findByText("Error: disk full")).toBeInTheDocument();
  });

  it("copies the Include line to the clipboard", async () => {
    vi.mocked(exportSshConfig).mockResolvedValue({
      path: "/tmp/x.conf",
      exported: [{ originalName: "web1", alias: "web1", renamed: false }],
      includeHint: "Include /tmp/x.conf",
    });
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.assign(navigator, { clipboard: { writeText } });

    render(
      <I18nextProvider i18n={testI18n}>
        <SshConfigExportDialog onClose={vi.fn()} />
      </I18nextProvider>,
    );

    fireEvent.click(await screen.findByText("Kopieren"));
    await waitFor(() => expect(writeText).toHaveBeenCalledWith("Include /tmp/x.conf"));
  });
});
