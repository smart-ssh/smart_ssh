// Issue #48 / Spec 0103: Drag-and-drop in der Verwalten-Sidebar. jsdom kennt
// kein Layout — `document.elementFromPoint` wird je Test auf das Element
// gesetzt, über dem der Zeiger „steht".
import { fireEvent, render, screen } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { afterEach, describe, expect, it, vi } from "vitest";
import { testI18n } from "../testI18n";
import type { GroupDto, ServerDto } from "../types";
import { Sidebar, type Selection } from "./Sidebar";

function group(id: string, name: string, parentId: string | null): GroupDto {
  return { id, name, parentId, notes: "" };
}

function server(id: string, name: string, groupId: string | null, isLocal = false): ServerDto {
  return {
    id,
    name,
    host: "example.invalid",
    port: 22,
    username: "user",
    groupId,
    tags: [],
    authKind: "agent",
    identityFilePath: null,
    jumpHost: null,
    notes: "",
    hasSudoPassword: false,
    sudoPasswordUnknown: false,
    isLocal,
    postIngestPolicy: "balanced",
    aiInjectionCheckEnabled: false,
    sftpServerPath: null,
  };
}

const groups = [group("g-prod", "Prod", null), group("g-web", "Web", "g-prod")];
const servers = [
  server("local", "Localhost", null, true),
  server("s-free", "free-1", null),
  server("s-prod", "prod-1", "g-prod"),
];

function renderSidebar(selection: Selection | null = null) {
  const onMove = vi.fn();
  const onSelect = vi.fn();
  render(
    <I18nextProvider i18n={testI18n}>
      <Sidebar
        groups={groups}
        servers={servers}
        selection={selection}
        onSelect={onSelect}
        onImportSshConfig={vi.fn()}
        onExportSshConfig={vi.fn()}
        onMove={onMove}
      />
    </I18nextProvider>,
  );
  return { onMove, onSelect };
}

const row = (text: string) => screen.getByText(new RegExp(text)) as HTMLElement;

/** Ein vollständiges Ziehen von `source` auf `over`. */
function dragOnto(source: HTMLElement, over: HTMLElement | null) {
  document.elementFromPoint = vi.fn(() => over);
  fireEvent.pointerDown(source, { button: 0, buttons: 1, pointerId: 1, clientX: 10, clientY: 10 });
  fireEvent.pointerMove(source, { buttons: 1, pointerId: 1, clientX: 40, clientY: 60 });
  fireEvent.pointerUp(source, { button: 0, buttons: 0, pointerId: 1, clientX: 40, clientY: 60 });
  fireEvent.click(source);
}

describe("Sidebar drag and drop (issue #48)", () => {
  afterEach(() => {
    // @ts-expect-error -- jsdom hat die Funktion von Haus aus nicht.
    delete document.elementFromPoint;
  });

  it("moves an ungrouped server into a group without selecting it", () => {
    const { onMove, onSelect } = renderSidebar();

    dragOnto(row("free-1"), row("Web"));

    expect(onMove).toHaveBeenCalledWith(
      { kind: "server", id: "s-free", label: "free-1" },
      { kind: "group", id: "g-web" },
    );
    expect(onSelect).not.toHaveBeenCalled();
  });

  it("moves a grouped server to the top level when dropped on an ungrouped row", () => {
    const { onMove } = renderSidebar();

    dragOnto(row("prod-1"), row("free-1"));

    expect(onMove).toHaveBeenCalledWith(
      { kind: "server", id: "s-prod", label: "prod-1" },
      { kind: "root" },
    );
  });

  it("moves a subgroup to the top level and passes a cycle on to be rejected", () => {
    const { onMove } = renderSidebar();

    dragOnto(row("Web"), row("free-1"));
    expect(onMove).toHaveBeenLastCalledWith(
      { kind: "group", id: "g-web", label: "Web" },
      { kind: "root" },
    );

    dragOnto(row("Prod"), row("Web"));
    expect(onMove).toHaveBeenLastCalledWith(
      { kind: "group", id: "g-prod", label: "Prod" },
      { kind: "group", id: "g-web" },
    );
  });

  it("does nothing for a drop where the item already is, or on no target", () => {
    const { onMove } = renderSidebar();

    dragOnto(row("prod-1"), row("Prod"));
    dragOnto(row("free-1"), null);
    dragOnto(row("free-1"), row("Localhost"));

    expect(onMove).not.toHaveBeenCalled();
  });

  it("cannot drag the local pseudo-server", () => {
    const { onMove, onSelect } = renderSidebar();

    dragOnto(row("Localhost"), row("Prod"));

    expect(onMove).not.toHaveBeenCalled();
    // Ohne Zieh-Handler bleibt es ein normaler Klick.
    expect(onSelect).toHaveBeenCalledWith({ kind: "server", id: "local" });
  });

  it("keeps a plain click (no movement) a selection", () => {
    const { onMove, onSelect } = renderSidebar();
    const source = row("free-1");

    fireEvent.pointerDown(source, { button: 0, buttons: 1, pointerId: 1, clientX: 10, clientY: 10 });
    fireEvent.pointerMove(source, { buttons: 1, pointerId: 1, clientX: 12, clientY: 11 });
    fireEvent.pointerUp(source, { button: 0, buttons: 0, pointerId: 1, clientX: 12, clientY: 11 });
    fireEvent.click(source);

    expect(onMove).not.toHaveBeenCalled();
    expect(onSelect).toHaveBeenCalledWith({ kind: "server", id: "s-free" });
  });

  it("cancels the drag with Escape", () => {
    const { onMove } = renderSidebar();
    const source = row("free-1");
    document.elementFromPoint = vi.fn(() => row("Prod"));

    fireEvent.pointerDown(source, { button: 0, buttons: 1, pointerId: 1, clientX: 10, clientY: 10 });
    fireEvent.pointerMove(source, { buttons: 1, pointerId: 1, clientX: 40, clientY: 60 });
    fireEvent.keyDown(window, { key: "Escape" });
    fireEvent.pointerUp(source, { button: 0, buttons: 0, pointerId: 1, clientX: 40, clientY: 60 });

    expect(onMove).not.toHaveBeenCalled();
  });
});

describe("Sidebar new items in the current folder (issue #49)", () => {
  const addServer = () => fireEvent.click(screen.getByRole("button", { name: testI18n.t("sidebar.addServer") }));
  const addGroup = () => fireEvent.click(screen.getByRole("button", { name: testI18n.t("sidebar.addGroup") }));

  it("prefills the selected folder for a new server and a new folder", () => {
    const { onSelect } = renderSidebar({ kind: "group", id: "g-web" });
    addServer();
    expect(onSelect).toHaveBeenLastCalledWith({ kind: "newServer", groupId: "g-web" });
    addGroup();
    expect(onSelect).toHaveBeenLastCalledWith({ kind: "newGroup", parentId: "g-web" });
  });

  it("prefills the folder of the selected server", () => {
    const { onSelect } = renderSidebar({ kind: "server", id: "s-prod" });
    addServer();
    expect(onSelect).toHaveBeenLastCalledWith({ kind: "newServer", groupId: "g-prod" });
    addGroup();
    expect(onSelect).toHaveBeenLastCalledWith({ kind: "newGroup", parentId: "g-prod" });
  });

  it("prefills no folder with nothing selected or an ungrouped server selected", () => {
    const { onSelect } = renderSidebar(null);
    addServer();
    expect(onSelect).toHaveBeenLastCalledWith({ kind: "newServer", groupId: null });
    addGroup();
    expect(onSelect).toHaveBeenLastCalledWith({ kind: "newGroup", parentId: null });
  });
});

describe("Sidebar tree entries are keyboard-operable (issue #112)", () => {
  // Native Buttons lösen Enter/Leertaste selbst als Klick aus — jsdom
  // simuliert diese Aktivierung nicht, darum prüft der Test das Element.
  it("renders groups and servers as native buttons that select on click", () => {
    const { onSelect } = renderSidebar();

    const groupEntry = screen.getByRole("button", { name: /Prod/ });
    const serverEntry = screen.getByRole("button", { name: /free-1/ });
    for (const entry of [groupEntry, serverEntry]) {
      expect(entry.tagName).toBe("BUTTON");
      expect(entry.getAttribute("type")).toBe("button");
    }

    fireEvent.click(groupEntry);
    expect(onSelect).toHaveBeenLastCalledWith({ kind: "group", id: "g-prod" });
    fireEvent.click(serverEntry);
    expect(onSelect).toHaveBeenLastCalledWith({ kind: "server", id: "s-free" });
  });
});
