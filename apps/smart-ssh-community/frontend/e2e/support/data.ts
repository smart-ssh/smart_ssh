// Builders for fake-backend data. Defaults mirror what the real backend
// sends for an ordinary password server; tests override only what matters.
import type { AiProviderConfigDto, ConnectStepRecord, GroupDto, ServerDto } from "../../src/types";
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

/**
 * A step log of a connection through `hops` jump hosts that fails at the
 * authentication of the last hop.
 */
export function failingStepLog(hops: number): ConnectStepRecord[] {
  const records: ConnectStepRecord[] = [];
  for (let hopIndex = 0; hopIndex < hops; hopIndex++) {
    const host = `hop-${hopIndex}.example.test`;
    const hop = `deploy@${host}:22`;
    const last = hopIndex === hops - 1;
    const ok = { state: "ok" } as const;
    records.push(
      { hopIndex, hop, step: { kind: "dnsResolution", host, port: 22, addresses: ["192.0.2.10"] }, status: ok, durationMs: 3 },
      { hopIndex, hop, step: { kind: "tcpConnect", address: "192.0.2.10", port: 22 }, status: ok, durationMs: 12 },
      {
        hopIndex,
        hop,
        step: { kind: "handshake", serverVersion: "SSH-2.0-OpenSSH_9.6", kex: "curve25519-sha256", hostKeyAlgorithm: "ssh-ed25519", cipher: "chacha20-poly1305@openssh.com", mac: null },
        status: ok,
        durationMs: 40,
      },
      { hopIndex, hop, step: { kind: "hostKeyCheck", keyType: "ssh-ed25519", fingerprint: `SHA256:hop${hopIndex}fingerprint`, result: "known" }, status: ok, durationMs: 1 },
      {
        hopIndex,
        hop,
        step: { kind: "authentication", method: "password", remainingMethods: ["publickey"], partialSuccess: false },
        status: last ? { state: "failed", code: "AUTH_FAILED" } : ok,
        durationMs: 210,
      },
    );
    if (!last) records.push({ hopIndex, hop, step: { kind: "tunnelOpen", host: `hop-${hopIndex + 1}.example.test`, port: 22 }, status: ok, durationMs: 5 });
  }
  return records;
}
