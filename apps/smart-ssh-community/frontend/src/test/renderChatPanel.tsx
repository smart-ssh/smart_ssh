// Renders `ChatPanel` the way the ChatPanel tests need it (issue #906):
// i18n provider, default session/server ids and a no-op `onActionSettled`.
// Returns the render result plus the mocked `api` module (the file must have
// installed it with `vi.mock("../api", …createApiMock…)`).
import { render } from "@testing-library/react";
import type { ComponentProps } from "react";
import { I18nextProvider } from "react-i18next";
import { vi } from "vitest";
import * as api from "../api";
import { ChatPanel } from "../components/ChatPanel";
import { testI18n } from "../testI18n";

export function renderChatPanel(
  props: Partial<ComponentProps<typeof ChatPanel>> = {},
) {
  const onActionSettled = vi.fn();
  const result = render(
    <I18nextProvider i18n={testI18n}>
      <ChatPanel
        sessionId="session-1"
        serverId="server-1"
        onActionSettled={onActionSettled}
        {...props}
      />
    </I18nextProvider>,
  );
  return { ...result, api: vi.mocked(api), onActionSettled };
}
