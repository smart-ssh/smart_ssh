// Objects that many component tests build identically (issue #906).
import type { AiProviderConfigDto, ChatErrorEvent } from "../api/types";

/** The active Anthropic provider stub used by the chat tests. */
export function anthropicProvider(
  overrides: Partial<AiProviderConfigDto> = {},
): AiProviderConfigDto {
  return {
    id: "p1",
    providerType: "anthropic",
    displayName: "Claude",
    baseUrl: null,
    model: "claude",
    supportsNativeToolCalling: true,
    isActive: true,
    extraHeaders: [],
    attestationUrl: null,
    ...overrides,
  } as AiProviderConfigDto;
}

/** A `chat-error` payload; the backend always sends every field (`null` when unset). */
export function chatError(
  p: Partial<ChatErrorEvent> & Pick<ChatErrorEvent, "sessionId" | "message">,
): ChatErrorEvent {
  return {
    code: null,
    providerErrorType: null,
    httpStatus: null,
    providerMessage: null,
    providerName: null,
    timeoutSecs: null,
    accessReturns: null,
    ...p,
  };
}
