// Issue #1385: the move-to dialog is a bordered tree list. One click on a
// row moves at once; the item's present location is marked "current" and
// cannot be chosen.
import { fireEvent, render, screen, within } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { GroupDto, ServerDto } from "../api/types";
import { testI18n } from "../testI18n";
import { MoveToDialog } from "./MoveToDialog";

function group(id: string, name: string, parentId: string | null = null): GroupDto {
  return { id, name, parentId } as GroupDto;
}
function server(id: string, name: string, groupId: string | null): ServerDto {
  return { id, name, groupId, isLocal: false } as ServerDto;
}

const groups = [group("g-prod", "production"), group("g-db", "databases", "g-prod"), group("g-stg", "staging")];

beforeEach(async () => {
  await testI18n.changeLanguage("en");
});

function renderDialog(item: Parameters<typeof MoveToDialog>[0]["item"], servers: ServerDto[]) {
  const onMove = vi.fn();
  const onClose = vi.fn();
  render(
    <I18nextProvider i18n={testI18n}>
      <MoveToDialog item={item} groups={groups} servers={servers} onMove={onMove} onClose={onClose} />
    </I18nextProvider>,
  );
  return { onMove, onClose, dialog: screen.getByRole("dialog") };
}

describe("MoveToDialog", () => {
  it("marks the server's current group as 'current' and disables it", () => {
    const { dialog } = renderDialog({ kind: "server", id: "s1", label: "db-prod" }, [server("s1", "db-prod", "g-prod")]);
    const current = within(dialog).getByRole("button", { name: /production/ });
    expect(current).toBeDisabled();
    expect(current).toHaveTextContent("current");
    expect(within(dialog).getByRole("button", { name: /staging/ })).toBeEnabled();
    expect(within(dialog).getAllByText("current")).toHaveLength(1);
    // Nested group rows carry tree connector lines.
    expect(within(dialog).getByRole("button", { name: /databases/ }).querySelectorAll('[data-testid="move-connector"]')).toHaveLength(1);
  });

  it("moves with a single click on a group row, without a separate Move button", () => {
    const { dialog, onMove, onClose } = renderDialog({ kind: "server", id: "s1", label: "db-prod" }, [server("s1", "db-prod", "g-prod")]);
    expect(within(dialog).queryByRole("button", { name: "Move" })).toBeNull();
    fireEvent.click(within(dialog).getByRole("button", { name: /staging/ }));
    expect(onMove).toHaveBeenCalledWith({ kind: "server", id: "s1", label: "db-prod" }, { kind: "group", id: "g-stg" });
    expect(onClose).toHaveBeenCalled();
  });

  it("offers the top level, marked current for a server without group, and moves there in one click otherwise", () => {
    const first = renderDialog({ kind: "server", id: "s2", label: "plain" }, [server("s2", "plain", null)]);
    const root = within(first.dialog).getByRole("button", { name: /top level \(no group\)/ });
    expect(root).toBeDisabled();
    expect(root).toHaveTextContent("current");
  });

  it("moves a server from a group to the top level with one click", () => {
    const { dialog, onMove } = renderDialog({ kind: "server", id: "s1", label: "db-prod" }, [server("s1", "db-prod", "g-prod")]);
    const root = within(dialog).getByRole("button", { name: /top level \(no group\)/ });
    expect(root).toBeEnabled();
    fireEvent.click(root);
    expect(onMove).toHaveBeenCalledWith(expect.anything(), { kind: "root" });
  });

  it("does not mark a group itself as current, only its parent", () => {
    const { dialog } = renderDialog({ kind: "group", id: "g-db", label: "databases" }, []);
    expect(within(dialog).getByRole("button", { name: /production/ })).toHaveTextContent("current");
    const self = within(dialog).getByRole("button", { name: /databases/ });
    expect(self).toBeDisabled();
    expect(self).not.toHaveTextContent("current");
  });
});
