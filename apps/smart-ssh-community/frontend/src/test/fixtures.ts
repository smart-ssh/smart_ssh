// Objects that many component tests build identically (issue #906).
import type { AiProviderConfigDto } from "../types";

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
