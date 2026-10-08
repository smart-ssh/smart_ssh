// Builders for fake-backend data. Defaults mirror what the real backend
// sends for an ordinary password server; tests override only what matters.
import type { AiProviderConfigDto, GroupDto, ServerDto } from "../../src/types";
import type { FakeBackendFixture } from "../harness/fixtureTypes";

export function server(overrides: Partial<ServerDto> & Pick<ServerDto, "id" | "name">): ServerDto {
  return {
    host: `${overrides.id}.example.test`,
    port: 22,
    username: "deploy",
    groupId: null,
    tags: [],
    authKind: "password",
    identityFilePath: null,
    jumpHost: null,
    notes: "",
    hasSudoPassword: false,
    sudoPasswordUnknown: false,
    isLocal: false,
    postIngestPolicy: "balanced",
    aiInjectionCheckEnabled: false,
    sftpServerPath: null,
    startDirectory: null,
    ...overrides,
  };
}

export function group(overrides: Partial<GroupDto> & Pick<GroupDto, "id" | "name">): GroupDto {
  return { parentId: null, notes: "", ...overrides };
}

export function aiProvider(overrides: Partial<AiProviderConfigDto> = {}): AiProviderConfigDto {
  return {
    id: "provider-1",
    providerType: "anthropic",
    displayName: "Test provider",
    baseUrl: null,
    model: "test-model",
    supportsNativeToolCalling: true,
    isActive: true,
    extraHeaders: [],
    attestationUrl: null,
    maxTokensOverride: null,
    webResearchEnabled: false,
    ...overrides,
  };
}

/** Settings of a user who already confirmed the first-run notice. */
export const ACKNOWLEDGED = { first_run_notice_acknowledged: true, language: "en" } as const;

/** A backend with nothing in it: first start, no servers, no provider. */
export const EMPTY: FakeBackendFixture = {};

/** A backend with a small inventory, an active provider and the notice confirmed. */
export function populated(): FakeBackendFixture {
  return {
    settings: { ...ACKNOWLEDGED },
    aiProviders: [aiProvider()],
    groups: [
      group({ id: "g-prod", name: "Production" }),
      group({ id: "g-web", name: "Web", parentId: "g-prod" }),
      group({ id: "g-lab", name: "Lab" }),
    ],
    servers: [
      server({ id: "s-db", name: "db-primary", groupId: "g-prod", tags: ["db"] }),
      server({ id: "s-web1", name: "web-1", groupId: "g-web" }),
      server({ id: "s-lab", name: "lab-box", groupId: "g-lab" }),
      server({ id: "s-loose", name: "standalone" }),
    ],
  };
}
