// Issue #1185 (Spec 0005): the login prompt dialog and its host.
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { answerAuthPrompt, cancelAuthPrompt } from "../api/api";
import { onAuthPromptEnded, onAuthPromptNeeded } from "../api/events";
import type { AuthPromptEndedEvent, AuthPromptNeededEvent } from "../api/types";
import { testI18n } from "../testI18n";
import { AuthPromptHost } from "./AuthPromptHost";

vi.mock("../api/api", async () => (await import("../test/apiMock")).createApiMock());
vi.mock("../api/events", () => ({
  onAuthPromptNeeded: vi.fn(() => Promise.resolve(() => {})),
  onAuthPromptEnded: vi.fn(() => Promise.resolve(() => {})),
}));

const kbd: AuthPromptNeededEvent = {
  promptId: "p1",
  hop: "root@prod-1:22",
  kind: "keyboard_interactive",
  name: "Two factor",
  instructions: "Use <b>your</b> token",
  prompts: [
    { text: "Password: ", echo: false },
    { text: "Verification code: ", echo: true },
  ],
};

function renderHost() {
  render(
    <I18nextProvider i18n={testI18n}>
      <AuthPromptHost />
    </I18nextProvider>,
  );
  const needed = vi.mocked(onAuthPromptNeeded).mock.calls.at(-1)![0];
  const ended = vi.mocked(onAuthPromptEnded).mock.calls.at(-1)![0];
  return {
    show: (e: AuthPromptNeededEvent) => act(() => needed(e)),
    end: (e: AuthPromptEndedEvent) => act(() => ended(e)),
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(answerAuthPrompt).mockResolvedValue(undefined);
  vi.mocked(cancelAuthPrompt).mockResolvedValue(undefined);
});

describe("AuthPromptHost", () => {
  it("masks echo-off prompts, shows echo-on prompts plain and focuses the first field", async () => {
    const { show } = renderHost();
    show(kbd);
    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).getByRole("heading")).toHaveTextContent("Anmeldung bei root@prod-1:22");
    expect(within(dialog).getByLabelText("Password:")).toHaveAttribute("type", "password");
    expect(within(dialog).getByLabelText("Verification code:")).toHaveAttribute("type", "text");
    await waitFor(() => expect(within(dialog).getByLabelText("Password:")).toHaveFocus());
    expect(within(dialog).getByText(/nicht gespeichert/)).toBeInTheDocument();
  });

  it("shows server-supplied name and instructions as literal text, not HTML", async () => {
    const { show } = renderHost();
    show({ ...kbd, name: "<img src=x onerror=alert(1)>" });
    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).getByTestId("auth-prompt-instructions")).toHaveTextContent(
      "Use <b>your</b> token",
    );
    expect(within(dialog).getByTestId("auth-prompt-name")).toHaveTextContent(
      "<img src=x onerror=alert(1)>",
    );
    expect(dialog.querySelector("b, img")).toBeNull();
  });

  it("sends the answers once on Enter and keeps no copy", async () => {
    const { show } = renderHost();
    show(kbd);
    const dialog = await screen.findByRole("dialog");
    const pw = within(dialog).getByLabelText("Password:") as HTMLInputElement;
    const code = within(dialog).getByLabelText("Verification code:") as HTMLInputElement;
    fireEvent.change(pw, { target: { value: "s3cret-pw" } });
    fireEvent.change(code, { target: { value: "424242" } });
    fireEvent.submit(pw.closest("form")!);
    expect(answerAuthPrompt).toHaveBeenCalledTimes(1);
    expect(answerAuthPrompt).toHaveBeenCalledWith("p1", ["s3cret-pw", "424242"]);
    // The fields were cleared before the dialog went away, and nothing in
    // the document still holds the answers.
    expect(pw.value).toBe("");
    expect(code.value).toBe("");
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(document.body.innerHTML).not.toContain("s3cret-pw");
    expect(document.body.innerHTML).not.toContain("424242");
  });

  it("cancels on Escape and on the Cancel button, once", async () => {
    const { show } = renderHost();
    show(kbd);
    const dialog = await screen.findByRole("dialog");
    fireEvent.keyDown(dialog, { key: "Escape" });
    await waitFor(() => expect(cancelAuthPrompt).toHaveBeenCalledWith("p1"));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    show({ ...kbd, promptId: "p2" });
    const second = await screen.findByRole("dialog");
    fireEvent.click(within(second).getByRole("button", { name: "Abbrechen" }));
    await waitFor(() => expect(cancelAuthPrompt).toHaveBeenCalledWith("p2"));
    expect(answerAuthPrompt).not.toHaveBeenCalled();
  });

  it("asks for the password of a multi-step login and opens again for a second round", async () => {
    const { show } = renderHost();
    show({ ...kbd, kind: "password", name: "", instructions: "", prompts: [] });
    let dialog = await screen.findByRole("dialog");
    const pw = within(dialog).getByLabelText("Passwort") as HTMLInputElement;
    expect(pw).toHaveAttribute("type", "password");
    fireEvent.change(pw, { target: { value: "pw1" } });
    fireEvent.submit(pw.closest("form")!);
    expect(answerAuthPrompt).toHaveBeenCalledWith("p1", ["pw1"]);
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());

    show({ ...kbd, promptId: "p2", prompts: [{ text: "Code: ", echo: true }] });
    dialog = await screen.findByRole("dialog");
    expect(within(dialog).getByLabelText("Code:")).toHaveValue("");
  });

  it("closes only for the matching prompt id when the backend ends the prompt", async () => {
    const { show, end } = renderHost();
    show(kbd);
    await screen.findByRole("dialog");
    end({ promptId: "other", reason: "cancelled" });
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    end({ promptId: "p1", reason: "abandoned" });
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  });
});
