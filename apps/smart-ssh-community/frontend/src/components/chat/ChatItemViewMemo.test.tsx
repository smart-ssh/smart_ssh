// Issue #924: a streamed text delta re-renders only the changed item.
import { act, render, screen } from "@testing-library/react";
import { useState } from "react";
import { I18nextProvider } from "react-i18next";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  registerDocumentAction,
  resetRegistryForTests,
  unregisterDocumentAction,
} from "../../extensions/registry";
import { testI18n } from "../../testI18n";
import type { ChatItem } from "./chatItem";

const renders = vi.hoisted(() => new Map<string, number>());

vi.mock("./AssistantMessageView", () => ({
  AssistantMessageView: ({ text }: { text: string }) => {
    const key = text.split(" ")[0];
    renders.set(key, (renders.get(key) ?? 0) + 1);
    return <p>{text}</p>;
  },
}));

import { ChatItemView } from "./ChatItemView";

const noop = vi.fn();
const noopAsync = vi.fn(async () => {});
let setItemsExternal: (updater: (items: ChatItem[]) => ChatItem[]) => void = () => {};

function Harness({ initial }: { initial: ChatItem[] }) {
  const [items, setItems] = useState(initial);
  setItemsExternal = setItems;
  return (
    <I18nextProvider i18n={testI18n}>
      {items.map((item) => (
        <ChatItemView
          key={item.id}
          item={item}
          onRespond={noop}
          onAcceptWithRule={noop}
          onExport={noopAsync}
          onContinueTruncated={noop}
          serverId="s"
          sessionId="sess"
        />
      ))}
    </I18nextProvider>
  );
}

afterEach(() => {
  resetRegistryForTests();
  renders.clear();
});

describe("ChatItemView memoisation (#924)", () => {
  it("re-renders only the last item per text delta in a 200 item chat", () => {
    const items: ChatItem[] = Array.from({ length: 200 }, (_, i) => ({
      type: "assistant",
      id: `a${i}`,
      text: `m${i} text`,
    }));
    render(<Harness initial={items} />);
    expect(renders.get("m0")).toBe(1);
    expect(renders.get("m199")).toBe(1);

    for (let d = 1; d <= 10; d++) {
      act(() => {
        setItemsExternal((cur) => {
          const next = cur.slice();
          next[199] = { type: "assistant", id: "a199", text: `m199 text${"x".repeat(d)}` };
          return next;
        });
      });
    }
    expect(renders.get("m199")).toBe(11);
    for (let i = 0; i < 199; i++) expect(renders.get(`m${i}`)).toBe(1);
  });

  it("shows and hides a document action registered after the card was rendered", () => {
    const doc: ChatItem = { type: "document", id: "d1", title: "T", contentMarkdown: "# T" };
    render(<Harness initial={[doc]} />);
    expect(screen.queryByRole("button", { name: "Late action" })).toBeNull();
    act(() => registerDocumentAction({ id: "late", label: "Late action", onInvoke: vi.fn() }));
    expect(screen.getByRole("button", { name: "Late action" })).toBeInTheDocument();
    act(() => unregisterDocumentAction("late"));
    expect(screen.queryByRole("button", { name: "Late action" })).toBeNull();
  });
});
