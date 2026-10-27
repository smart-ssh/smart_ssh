import { act, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { resetChatSessionIdsForTests, setChatSessionId } from "../chatSessionIds";
import {
  registerSessionHeaderItem,
  resetRegistryForTests,
  unregisterSessionHeaderItem,
} from "./registry";
import { SessionHeaderItems } from "./SessionHeaderItems";
import { requestResumeChatSession, subscribeRequestResumeChatSession } from "../navigationBus";

function Probe({ id }: { id: string }) {
  return (props: { runtimeSessionId: string; serverId: string; chatSessionId: string | null }) => (
    <span data-testid={id}>
      {props.runtimeSessionId}|{props.serverId}|{String(props.chatSessionId)}
    </span>
  );
}
const probe = (id: string) => (Probe({ id }) as unknown) as React.ComponentType<{
  runtimeSessionId: string;
  serverId: string;
  chatSessionId: string | null;
}>;

describe("SessionHeaderItems (Issue #611)", () => {
  afterEach(() => {
    resetRegistryForTests();
    resetChatSessionIdsForTests();
  });

  it("renders nothing without registrations", () => {
    const { container } = render(<SessionHeaderItems runtimeSessionId="s1" serverId="srv" />);
    expect(container).toBeEmptyDOMElement();
  });

  it("passes ids and follows chat session changes (null for a local session)", () => {
    registerSessionHeaderItem({ id: "a", component: probe("a") });
    render(<SessionHeaderItems runtimeSessionId="s1" serverId="srv" />);
    expect(screen.getByTestId("a")).toHaveTextContent("s1|srv|null");
    act(() => setChatSessionId("s1", "chat-1"));
    expect(screen.getByTestId("a")).toHaveTextContent("s1|srv|chat-1");
    act(() => setChatSessionId("s1", "chat-2"));
    expect(screen.getByTestId("a")).toHaveTextContent("s1|srv|chat-2");
    act(() => setChatSessionId("s1", null));
    expect(screen.getByTestId("a")).toHaveTextContent("s1|srv|null");
  });

  it("orders by `order` and removes an unregistered item", () => {
    registerSessionHeaderItem({ id: "late", order: 5, component: probe("late") });
    registerSessionHeaderItem({ id: "early", order: 1, component: probe("early") });
    render(<SessionHeaderItems runtimeSessionId="s1" serverId="srv" />);
    const ids = Array.from(
      document.querySelectorAll("[data-extension-session-header-item]"),
    ).map((e) => e.getAttribute("data-extension-session-header-item"));
    expect(ids).toEqual(["early", "late"]);
    act(() => unregisterSessionHeaderItem("early"));
    expect(screen.queryByTestId("early")).toBeNull();
    expect(screen.getByTestId("late")).toBeInTheDocument();
  });
});

describe("requestResumeChatSession (Issue #611)", () => {
  it("notifies subscribers with the server and chat session id", () => {
    const listener = vi.fn();
    const off = subscribeRequestResumeChatSession(listener);
    requestResumeChatSession("srv", "chat-1");
    off();
    requestResumeChatSession("srv", "chat-2");
    expect(listener).toHaveBeenCalledTimes(1);
    expect(listener).toHaveBeenCalledWith({ serverId: "srv", chatSessionId: "chat-1" });
  });

  it("isolates an item that throws while rendering (issue #880)", () => {
    const spy = vi.spyOn(console, "error").mockImplementation(() => {});
    const Broken = (): never => {
      throw new Error("broken header item");
    };
    registerSessionHeaderItem({ id: "bad", order: 1, component: Broken });
    registerSessionHeaderItem({ id: "good", order: 2, component: probe("good") });
    render(<SessionHeaderItems runtimeSessionId="s1" serverId="srv" />);
    expect(screen.getByTestId("good")).toBeInTheDocument();
    expect(spy.mock.calls.some((c) => String(c[0]).includes("session-header-item:bad"))).toBe(true);
    spy.mockRestore();
  });
});
