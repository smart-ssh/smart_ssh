// Spec 0046, Fund 2: das Regel-Test-Panel muss die Tags des gewählten
// Servers automatisch in die Simulation übernehmen, sonst zeigt es für ein
// Kommando, das über eine tag-scoped Regel `AutoExec` wäre, fälschlich
// "keine Regel matcht".
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { describe, expect, it, vi } from "vitest";
import { testI18n } from "../testI18n";
import type { EvaluationTraceDto, RuleDto, ServerDto } from "../types";
import { TestPanel } from "./FilterRulesView";

const evaluateExplainedMock = vi.fn<
  (command: string, ctx: { serverId: string | null; tags: string[] }) => Promise<EvaluationTraceDto>
>();

vi.mock("../api", () => ({
  commandErrorMessage: (err: unknown) => String(err),
  evaluateExplained: (command: string, ctx: { serverId: string | null; tags: string[] }) =>
    evaluateExplainedMock(command, ctx),
}));

function taggedServer(): ServerDto {
  return {
    id: "server-1",
    name: "prod-1",
    host: "prod-1.internal",
    port: 22,
    username: "deploy",
    groupId: null,
    tags: ["production"],
    authKind: "agent",
    jumpHost: null,
    notes: "",
    hasSudoPassword: false,
    isLocal: false,
    postIngestPolicy: "balanced",
    aiInjectionCheckEnabled: false,
  };
}

const autoExecTrace: EvaluationTraceDto = {
  decision: "AutoExec",
  matchedRule: "rule-1",
  matchedHardBlacklistEntry: null,
  subCommandTraces: [],
};

const noRuleTrace: EvaluationTraceDto = {
  decision: { Confirm: { reason: "keine Regel gefunden", code: "FILTER_NO_RULE_MATCHED" } },
  matchedRule: null,
  matchedHardBlacklistEntry: null,
  subCommandTraces: [],
};

function renderPanel(servers: ServerDto[]) {
  return render(
    <I18nextProvider i18n={testI18n}>
      <TestPanel servers={servers} rules={[] as RuleDto[]} />
    </I18nextProvider>,
  );
}

describe("TestPanel server-tag derivation (Spec 0046, Fund 2)", () => {
  it("sends the selected server's own tags to evaluateExplained", async () => {
    evaluateExplainedMock.mockResolvedValue(autoExecTrace);
    const server = taggedServer();
    renderPanel([server]);

    fireEvent.change(screen.getByLabelText(/Server simulieren|Simulate server/), {
      target: { value: server.id },
    });
    fireEvent.change(screen.getByPlaceholderText("ls -la && rm -rf /tmp/x"), {
      target: { value: "systemctl restart nginx" },
    });
    fireEvent.click(screen.getByRole("button", { name: /^(Testen|Test)$/ }));

    await waitFor(() => expect(evaluateExplainedMock).toHaveBeenCalled());
    expect(evaluateExplainedMock).toHaveBeenCalledWith("systemctl restart nginx", {
      serverId: server.id,
      tags: ["production"],
    });
  });

  it("shows AutoExec (not 'no rule matched') for a command a tag-scoped rule would allow", async () => {
    evaluateExplainedMock.mockResolvedValue(autoExecTrace);
    const server = taggedServer();
    renderPanel([server]);

    fireEvent.change(screen.getByLabelText(/Server simulieren|Simulate server/), {
      target: { value: server.id },
    });
    fireEvent.change(screen.getByPlaceholderText("ls -la && rm -rf /tmp/x"), {
      target: { value: "systemctl restart nginx" },
    });
    fireEvent.click(screen.getByRole("button", { name: /^(Testen|Test)$/ }));

    await waitFor(() => expect(screen.getByText("AutoExec")).toBeInTheDocument());
  });

  it("still shows 'no rule matched' when no server is selected (regression baseline)", async () => {
    evaluateExplainedMock.mockResolvedValue(noRuleTrace);
    renderPanel([taggedServer()]);

    fireEvent.change(screen.getByPlaceholderText("ls -la && rm -rf /tmp/x"), {
      target: { value: "systemctl restart nginx" },
    });
    fireEvent.click(screen.getByRole("button", { name: /^(Testen|Test)$/ }));

    await waitFor(() => expect(evaluateExplainedMock).toHaveBeenCalled());
    expect(evaluateExplainedMock).toHaveBeenCalledWith("systemctl restart nginx", {
      serverId: null,
      tags: [],
    });
  });
});
