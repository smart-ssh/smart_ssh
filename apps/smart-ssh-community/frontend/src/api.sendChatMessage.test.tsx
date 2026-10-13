// Issue #471: `send_chat_message` carries the attachment ids; a `FeatureLocked`
// rejection goes through the existing feature-locked bus and its dialog.
import { render, screen } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { FeatureLockedDialog } from "./extensions/FeatureLockedDialog";
import { testI18n } from "./testI18n";

const invokeMock = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));

import { sendChatMessage } from "./api";

describe("sendChatMessage attachments", () => {
  beforeEach(() => invokeMock.mockReset());

  it("passes attachmentIds only when present", async () => {
    invokeMock.mockResolvedValue(undefined);
    await sendChatMessage("s1", "hi");
    expect(invokeMock).toHaveBeenLastCalledWith("send_chat_message", { sessionId: "s1", text: "hi" });
    await sendChatMessage("s1", "hi", ["a", "b"]);
    expect(invokeMock).toHaveBeenLastCalledWith("send_chat_message", {
      sessionId: "s1",
      text: "hi",
      attachmentIds: ["a", "b"],
    });
  });

  it("opens the feature-locked dialog for a locked attachment feature", async () => {
    render(
      <I18nextProvider i18n={testI18n}>
        <FeatureLockedDialog />
      </I18nextProvider>,
    );
    invokeMock.mockImplementation((cmd: string) =>
      cmd !== "send_chat_message"
        ? Promise.resolve(undefined)
        : Promise.reject({
        message: "locked",
        feature_locked: { feature: "chat_attachments", tier: "free" },
      }),
    );
    const call = sendChatMessage("s1", "hi", ["a"]);
    await expect(call).rejects.toMatchObject({ message: "locked" });
    expect(await screen.findByRole("button")).toBeInTheDocument();
    expect(document.body.textContent).toContain("chat_attachments");
  });
});
