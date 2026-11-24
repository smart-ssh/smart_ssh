// Issue #1371: every transcript item is flat — no rounded corners, no shadows —
// including the user-message block and the confirmation card body.
import { render } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { describe, expect, it, vi } from "vitest";
import { testI18n } from "../testI18n";
import { ChatItemView, type ChatItem } from "./ChatPanel";

vi.mock("../api/api", async () => (await import("../test/apiMock")).createApiMock({}));

const action = {
  type: "action",
  id: "a",
  actionId: "act",
  action: { SuggestCommand: { command: "ls" } },
  decision: "AutoExec",
  responded: false,
  previousNoteContent: null,
  usesStoredSudoPassword: false,
  previousFileContent: null,
  previousFileSize: null,
  previousFileTooLarge: false,
  targetName: null,
  riskAssessment: null,
  riskSecondOpinionPending: false,
  secondOpinionUnavailable: false,
  startedAt: null,
  origin: { kind: "internal" },
} as unknown as Extract<ChatItem, { type: "action" }>;

const ITEMS: ChatItem[] = [
  { type: "user", id: "u", text: "hello", attachments: [{ name: "f.txt", kind: "text" }] },
  { type: "user", id: "uq", text: "queued", queued: true },
  { type: "assistant", id: "as", text: "answer `code`\n\n```sh\nls\n```", truncated: true },
  action,
  { ...action, decision: { Confirm: { reason: "r", code: "c" } } },
  { ...action, decision: { Confirm: { reason: "r", code: "c" } }, responded: true, userDecision: "deny" as const },
  { ...action, decision: { Deny: { code: "x", reason: "r" } } },
  { ...action, decision: "AutoExec", settled: "timeout" as const },
  { type: "error", id: "e", message: "boom", code: null, retryable: true },
  { type: "contextCompacted", id: "c" },
  { type: "autoContinuationLimitReached", id: "l", limit: 3 },
  { type: "emptyResponse", id: "em" },
  { type: "refusedResponse", id: "rf" },
  { type: "webResearchUnavailable", id: "w" },
  { type: "responseCancelled", id: "rc" },
  { type: "aiBudgetWaiting", id: "b", waitSeconds: 5 },
  { type: "document", id: "d", title: "Doc", contentMarkdown: "# T" },
  {
    type: "question",
    id: "q",
    questionId: "qq",
    question: "Which?",
    options: [{ label: "A", description: "d" }],
    multiSelect: false,
  },
  {
    type: "historyCommandResult",
    id: "h",
    command: "ls",
    stdout: "out",
    stderr: "err",
    exitCode: 1,
    cancelled: false,
    truncated: true,
  },
  { type: "historyRejected", id: "hr", command: "rm x", reason: "no" },
  { type: "historyActionResult", id: "ha", text: "done" },
  {
    type: "webActivity",
    id: "wa",
    activity: {
      kind: "search",
      input: "q",
      results: [{ title: "t", url: "https://e.x" }],
      cited: [],
      contentTruncated: false,
      errorCode: null,
    },
  },
  { type: "historyAttachments", id: "hat", items: [{ name: "f.txt", kind: "text" }] },
];

describe("flat transcript (#1371)", () => {
  it.each(ITEMS.map((i, n) => [`${n} ${i.type}`, i] as const))(
    "renders %s without rounded corners or shadows",
    (_name, item) => {
      const { container } = render(
        <I18nextProvider i18n={testI18n}>
          <ChatItemView
            item={item}
            onRespond={vi.fn()}
            onExport={vi.fn()}
            onContinueTruncated={vi.fn()}
            serverId="s"
            sessionId="sess"
          />
        </I18nextProvider>,
      );
      expect(container.firstElementChild).not.toBeNull();
      const cls = Array.from(container.querySelectorAll("*:not(button)"))
        .map((e) => e.getAttribute("class") ?? "")
        .join(" ");
      expect(cls).not.toMatch(/(^|\s)(rounded|shadow)(-|\s|$)/);
    },
  );
});
