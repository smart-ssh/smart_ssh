// Issue #693 (Spec 0039, §5.2): progress and verdict of the injection check
// under a command result, a file read and a web research card.
import { act, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  onChatActionProposed,
  onChatActionResult,
  onChatInjectionCheck,
  onChatWebActivity,
} from "../api/events";
import type {
  ChatActionProposedEvent,
  ChatActionResultEvent,
  ChatInjectionCheckEvent,
  ChatWebActivityEvent,
} from "../api/types";
import { testI18n } from "../testI18n";
import { renderChatPanel } from "../test/renderChatPanel";
import { splitInjectionReason } from "./ChatPanel";

import "../test/scrollTo";

vi.mock("../api/api", async () =>
  (await import("../test/apiMock")).createApiMock(),
);

vi.mock("../api/events", async () =>
  (await import("../test/eventsMock")).createEventsMock(),
);

vi.mock("../settings/riskSettings", () => ({
  loadRiskClassifierSettings: vi.fn(() =>
    Promise.resolve({ enabled: false, providerId: null }),
  ),
}));

function handlerOf<T>(listener: unknown): (event: T) => void {
  const calls = vi.mocked(listener as (h: (event: T) => void) => unknown).mock
    .calls;
  return calls[calls.length - 1][0];
}

const SESSION = "session-1";

function proposed(
  action: ChatActionProposedEvent["action"],
): ChatActionProposedEvent {
  return {
    sessionId: SESSION,
    actionId: "action-1",
    action,
    decision: "AutoExec",
    previousNoteContent: null,
    usesStoredSudoPassword: false,
    autoExecRules: [],
    previousFileContent: null,
    previousFileSize: null,
    targetName: null,
    riskAssessment: null,
    origin: { kind: "internal" },
  };
}

function check(
  itemId: string,
  state: Record<string, unknown>,
): ChatInjectionCheckEvent {
  return { sessionId: SESSION, itemId, ...state } as ChatInjectionCheckEvent;
}

async function renderPanel() {
  renderChatPanel({ sessionId: SESSION });
  await waitFor(() => expect(onChatInjectionCheck).toHaveBeenCalled());
}

const COMMAND = { SuggestCommand: { command: "cat log" } };
const FILE = { ReadRemoteFile: { path: "/etc/motd" } };
const RESULTS: Record<string, ChatActionResultEvent["result"]> = {
  command: {
    kind: "command",
    command: "cat log",
    stdout: "hello",
    stderr: "",
    exitCode: 0,
    cancelled: false,
    truncated: false,
  },
  file: { kind: "fileRead", path: "/etc/motd", content: "hello" },
};

async function showItem(kind: "command" | "file" | "web"): Promise<string> {
  await renderPanel();
  if (kind === "web") {
    act(() =>
      handlerOf<ChatWebActivityEvent>(onChatWebActivity)({
        sessionId: SESSION,
        activityId: "web-1",
        activity: {
          kind: "search",
          input: "q",
          results: [],
          cited: [],
          contentTruncated: false,
          errorCode: null,
        },
      }),
    );
    return "web-1";
  }
  act(() =>
    handlerOf<ChatActionProposedEvent>(onChatActionProposed)(
      proposed(kind === "command" ? COMMAND : FILE),
    ),
  );
  act(() =>
    handlerOf<ChatActionResultEvent>(onChatActionResult)({
      sessionId: SESSION,
      actionId: "action-1",
      result: RESULTS[kind],
    }),
  );
  return "action-1";
}

describe.each(["command", "file", "web"] as const)(
  "ChatPanel injection check status under a %s (issue #693)",
  (kind) => {
    beforeEach(() => {
      vi.clearAllMocks();
    });

    it("shows a spinner while running and nothing after a clean verdict", async () => {
      const id = await showItem(kind);
      const emit = handlerOf<ChatInjectionCheckEvent>(onChatInjectionCheck);
      act(() => emit(check(id, { state: "running" })));
      expect(
        await screen.findByTestId("injection-check-running"),
      ).toHaveTextContent(testI18n.t("chat.injectionChecking"));
      act(() => emit(check(id, { state: "clean" })));
      expect(screen.queryByTestId("injection-check-running")).toBeNull();
      expect(screen.queryByTestId("injection-check-suspected")).toBeNull();
    });

    it("shows the warning with the reason, literally, after a suspected verdict", async () => {
      const id = await showItem(kind);
      const emit = handlerOf<ChatInjectionCheckEvent>(onChatInjectionCheck);
      act(() => emit(check(id, { state: "running" })));
      act(() =>
        emit(
          check(id, {
            state: "suspected",
            reason: "<b>bold</b> **md** [x](http://e.x)",
          }),
        ),
      );
      const box = await screen.findByTestId("injection-check-suspected");
      expect(box).toHaveTextContent(testI18n.t("chat.injectionSuspected"));
      expect(box).toHaveTextContent("<b>bold</b> **md** [x](http://e.x)");
      expect(box.querySelector("b, a")).toBeNull();
      expect(screen.queryByTestId("injection-check-running")).toBeNull();
    });

    it("shows the unavailable notice", async () => {
      const id = await showItem(kind);
      act(() =>
        handlerOf<ChatInjectionCheckEvent>(onChatInjectionCheck)(
          check(id, { state: "unavailable" }),
        ),
      );
      expect(
        await screen.findByTestId("injection-check-unavailable"),
      ).toHaveTextContent(testI18n.t("errors.AI_INJECTION_CHECK_UNAVAILABLE"));
    });

    it("ignores events of another session", async () => {
      const id = await showItem(kind);
      act(() =>
        handlerOf<ChatInjectionCheckEvent>(onChatInjectionCheck)({
          ...check(id, { state: "running" }),
          sessionId: "other",
        }),
      );
      expect(screen.queryByTestId("injection-check-running")).toBeNull();
    });
  },
);

describe("splitInjectionReason (issue #693)", () => {
  it("splits the reason only for the injection code, at the last marker", () => {
    expect(
      splitInjectionReason(
        "FILTER_INJECTION_SUSPECTED_REQUIRES_CONFIRM",
        "base\nReason: why",
      ),
    ).toEqual({ text: "base", injectionReason: "why" });
    expect(
      splitInjectionReason(
        "FILTER_POST_INGEST_REQUIRES_CONFIRM",
        "base\nReason: why",
      ),
    ).toEqual({
      text: "base\nReason: why",
      injectionReason: null,
    });
    expect(
      splitInjectionReason(
        "FILTER_INJECTION_SUSPECTED_REQUIRES_CONFIRM",
        "base",
      ),
    ).toEqual({
      text: "base",
      injectionReason: null,
    });
  });
});
