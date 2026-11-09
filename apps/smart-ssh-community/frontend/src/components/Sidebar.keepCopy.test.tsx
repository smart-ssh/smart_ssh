// Spec 0103 §3a: in the narrow manage tree the "⋯" dialog of a copy server or
// copy group offers "Keep as regular entry"; other rows do not.
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { testI18n } from "../testI18n";
import type { GroupDto, ServerDto } from "../api/types";
import { Sidebar } from "./Sidebar";
import { keepGroupAsRegularEntry, keepServerAsRegularEntry } from "../api/api";
import { setNarrow, setWide } from "../test/matchMedia";

vi.mock("../api/api", async () => {
  return (await import("../test/apiMock")).createApiMock({
    keepServerAsRegularEntry: vi.fn(() => Promise.resolve()),
    keepGroupAsRegularEntry: vi.fn((_id: string, confirm: boolean) =>
      Promise.resolve({ applied: confirm, groups: 2, servers: 3 }),
    ),
  });
});

const KEEP = testI18n.t("copyMarker.keep");
const grp = (id: string, name: string, isCopy: boolean): GroupDto => ({
  id,
  name,
  parentId: null,
  notes: "",
  isCopy,
});
const srv = (id: string, name: string, isCopy: boolean): ServerDto => ({
  id,
  name,
  host: "h.example",
  port: 22,
  username: "u",
  groupId: null,
  tags: [],
  authKind: "agent",
  identityFilePath: null,
  jumpHost: null,
  notes: "",
  hasSudoPassword: false,
  sudoPasswordUnknown: false,
  isLocal: false,
  postIngestPolicy: "balanced",
  aiInjectionCheckEnabled: false,
  sftpServerPath: null,
  elevationAllowed: true,
  isCopy,
  startDirectory: null,
});

function renderSidebar(onKept = vi.fn()) {
  render(
    <I18nextProvider i18n={testI18n}>
      <Sidebar
        groups={[grp("g1", "old-group", true), grp("g2", "plain-group", false)]}
        servers={[srv("s1", "old-srv", true), srv("s2", "plain-srv", false)]}
        selection={null}
        onSelect={vi.fn()}
        onImportSshConfig={vi.fn()}
        onExportSshConfig={vi.fn()}
        onMove={vi.fn()}
        onKeptAsRegular={onKept}
      />
    </I18nextProvider>,
  );
  return onKept;
}
const more = (name: string) =>
  screen.getByRole("button", { name: testI18n.t("treeDrag.moveTo", { name }) });

describe("Sidebar: keep as regular entry (narrow)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    setNarrow();
  });
  afterEach(() => setWide());

  it("keeps a copy server from the ⋯ dialog and reloads", async () => {
    const onKept = renderSidebar();
    fireEvent.click(more("old-srv"));
    fireEvent.click(await screen.findByRole("button", { name: KEEP }));
    await waitFor(() => expect(keepServerAsRegularEntry).toHaveBeenCalledWith("s1"));
    await waitFor(() => expect(onKept).toHaveBeenCalled());
  });

  it("confirms before keeping a copy group; cancel changes nothing", async () => {
    const onKept = renderSidebar();
    fireEvent.click(more("old-group"));
    fireEvent.click(await screen.findByRole("button", { name: KEEP }));
    await screen.findByText(testI18n.t("copyMarker.groupConfirmBody", { groups: 2, servers: 3 }));
    fireEvent.click(screen.getByRole("button", { name: testI18n.t("common.cancel") }));
    expect(keepGroupAsRegularEntry).not.toHaveBeenCalledWith("g1", true);
    expect(onKept).not.toHaveBeenCalled();

    fireEvent.click(more("old-group"));
    fireEvent.click(await screen.findByRole("button", { name: KEEP }));
    await screen.findByText(testI18n.t("copyMarker.groupConfirmBody", { groups: 2, servers: 3 }));
    const buttons = screen.getAllByRole("button", { name: KEEP });
    fireEvent.click(buttons[buttons.length - 1]);
    await waitFor(() => expect(keepGroupAsRegularEntry).toHaveBeenCalledWith("g1", true));
    await waitFor(() => expect(onKept).toHaveBeenCalled());
  });

  it("offers no keep action for non-copy rows", async () => {
    renderSidebar();
    fireEvent.click(more("plain-srv"));
    await screen.findByRole("button", { name: testI18n.t("mainScreen.ungrouped") });
    expect(screen.queryByRole("button", { name: KEEP })).toBeNull();
  });
});
