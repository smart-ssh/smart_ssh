// In-memory stand-in for the Tauri backend, installed through
// `@tauri-apps/api/mocks`. It answers the commands the app sends at start-up
// and on the screens the end-to-end tests visit, from a small model
// (servers, groups, sessions, settings). Anything it does not know is
// rejected with a clear error and recorded, so a test fails instead of the
// app silently receiving `undefined`.
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import type {
  AiProviderConfigDto,
  GroupDto,
  ServerDto,
  SessionSummaryDto,
} from "../../src/types";
import type {
  CannedResponse,
  E2eBridge,
  FakeBackendFixture,
  FakeHostKey,
  RecordedCall,
} from "./fixtureTypes";

const PERSIST_KEY = "smart-ssh-e2e-fake-backend";

interface Model {
  servers: ServerDto[];
  groups: GroupDto[];
  sessions: SessionSummaryDto[];
  aiProviders: AiProviderConfigDto[];
  settings: Record<string, unknown>;
  locale: string;
  hostKeys: Record<string, FakeHostKey>;
  commands: Record<string, CannedResponse>;
  nextId: number;
}

type Args = Record<string, unknown>;
type Handler = (args: Args) => unknown;

interface Internals {
  runCallback(id: number, data: unknown): void;
}

function tauriInternals(): Internals {
  return (window as unknown as { __TAURI_INTERNALS__: Internals }).__TAURI_INTERNALS__;
}

/** A backend `CommandError` as the frontend expects it (`api.ts`). */
function commandError(message: string, code: string | null = null): { message: string; code: string | null } {
  return { message, code };
}

function loadModel(fixture: FakeBackendFixture): Model {
  const saved = sessionStorage.getItem(PERSIST_KEY);
  if (saved) return JSON.parse(saved) as Model;
  return {
    servers: fixture.servers ?? [],
    groups: fixture.groups ?? [],
    sessions: fixture.sessions ?? [],
    aiProviders: fixture.aiProviders ?? [],
    settings: fixture.settings ?? {},
    locale: fixture.locale ?? "en-US",
    hostKeys: fixture.hostKeys ?? {},
    commands: fixture.commands ?? {},
    nextId: 1,
  };
}

export function installFakeBackend(fixture: FakeBackendFixture): void {
  const model = loadModel(fixture);
  const persist = () => sessionStorage.setItem(PERSIST_KEY, JSON.stringify(model));
  persist();

  const calls: RecordedCall[] = [];
  const unknownCommands: string[] = [];
  const listeners = new Map<string, number[]>();
  const held = new Map<string, { resolve: (v: unknown) => void; reject: (e: unknown) => void }[]>();
  /** `connect` calls waiting for `confirm_host_key`, by session id. */
  const hostKeyWaiters = new Map<string, (trusted: boolean) => void>();
  let nextPromptId = 1;

  const freshId = (prefix: string) => `${prefix}-${model.nextId++}`;

  const emit = (event: string, payload: unknown): number => {
    const ids = [...(listeners.get(event) ?? [])];
    for (const id of ids) tauriInternals().runCallback(id, { event, id, payload });
    return ids.length;
  };

  const findServer = (id: unknown): ServerDto => {
    const server = model.servers.find((s) => s.id === id);
    if (!server) throw commandError(`server not found: ${String(id)}`, "SERVER_NOT_FOUND");
    return server;
  };

  const openSession = (server: ServerDto): string => {
    const sessionId = freshId("session");
    model.sessions.push({
      sessionId,
      serverId: server.id,
      serverName: server.name,
      status: "connected",
      hasPendingAction: false,
      mcp: null,
    });
    persist();
    return sessionId;
  };

  const store = model.settings;
  const handlers: Record<string, Handler> = {
    // --- Tauri plumbing --------------------------------------------------
    "plugin:event|listen": ({ event, handler }) => {
      const list = listeners.get(event as string) ?? [];
      list.push(handler as number);
      listeners.set(event as string, list);
      return handler;
    },
    "plugin:event|unlisten": ({ event, eventId }) => {
      const list = listeners.get(event as string) ?? [];
      listeners.set(
        event as string,
        list.filter((id) => id !== eventId),
      );
      return null;
    },
    "plugin:os|locale": () => model.locale,
    "plugin:store|load": () => 1,
    "plugin:store|get": ({ key }) => [store[key as string] ?? null, key as string in store],
    "plugin:store|has": ({ key }) => (key as string) in store,
    "plugin:store|set": ({ key, value }) => {
      store[key as string] = value;
      return null;
    },
    "plugin:store|delete": ({ key }) => {
      const existed = (key as string) in store;
      delete store[key as string];
      return existed;
    },
    // `save()` is what makes a setting survive a restart; the fake keeps the
    // model in `sessionStorage`, which survives a reload of the page.
    "plugin:store|save": () => {
      persist();
      return null;
    },

    // --- Start-up and window chrome --------------------------------------
    get_startup_state: () => ({
      screen: "unlocked",
      mode: "keychain",
      offersStartOver: false,
      failedUnlockAttempts: 0,
      language: model.locale.startsWith("de") ? "de" : "en",
    }),
    get_entitlements: () => ({
      tier: "free",
      features: [],
      seats: null,
      expiresAt: null,
      nonCommercial: false,
      licensee: null,
    }),
    get_platform: () => "linux",
    create_overlay_titlebar: () => "native",
    get_app_info: () => ({
      version: "0.0.0-e2e",
      commitHash: "e2e",
      versionDisplay: "0.0.0-e2e (e2e)",
      edition: "Community",
      buildType: "Dev",
    }),

    // --- Servers and groups ----------------------------------------------
    list_servers: ({ groupId }) =>
      groupId == null ? model.servers : model.servers.filter((s) => s.groupId === groupId),
    get_server: ({ id }) => findServer(id),
    list_groups: () => model.groups,
    // The in-memory model only holds readable servers; tests that need an
    // unusable one override this command.
    list_unusable_servers: () => [],
    list_known_tags: () => [...new Set(model.servers.flatMap((s) => s.tags))].sort(),
    move_server_to_group: ({ id, groupId }) => {
      findServer(id).groupId = (groupId as string | null) ?? null;
      persist();
      return null;
    },
    move_group: ({ id, parentId }) => {
      const group = model.groups.find((g) => g.id === id);
      if (!group) throw commandError(`group not found: ${String(id)}`);
      group.parentId = (parentId as string | null) ?? null;
      persist();
      return null;
    },

    large_note_dialog_threshold_chars: () => 4000,
    test_connection: () => ({ kind: "success", steps: [] }),

    // --- Filter rules ----------------------------------------------------
    list_rules: () => [],
    list_hard_blacklist: () => [],

    // --- Settings screens ------------------------------------------------
    list_ai_providers: () => model.aiProviders,
    discover_models: () => [],
    get_data_paths: () => [
      { id: "database", label: null, path: "/data/smart-ssh/smart-ssh.db", isDirectory: false },
      { id: "logs", label: null, path: "/data/smart-ssh/logs", isDirectory: true },
    ],
    get_keychain_status: () => ({ available: true, reason: null }),
    get_master_password_mode: () => "keychain",
    get_chat_session_retention_days: () => null,
    get_mcp_server_settings: () => ({
      enabled: false,
      endpoint: "http://127.0.0.1:7777",
      token: "e2e-token-not-a-secret",
      allowedServerIds: [],
      confirmTimeoutSecs: 120,
    }),

    // --- Sessions --------------------------------------------------------
    list_sessions: () => model.sessions,
    list_chat_sessions: () => [],
    connect: async ({ serverId }) => {
      const server = findServer(serverId);
      const hostKey = model.hostKeys[server.id];
      if (!hostKey) return openSession(server);
      const pendingSessionId = freshId("pending");
      const trusted = await new Promise<boolean>((resolve) => {
        hostKeyWaiters.set(pendingSessionId, resolve);
        emit("host-key-verification-needed", {
          sessionId: pendingSessionId,
          promptId: nextPromptId++,
          host: server.host,
          port: server.port,
          kind: hostKey.kind,
          fingerprint: hostKey.fingerprint,
          expectedFingerprint: hostKey.expectedFingerprint ?? null,
        });
      });
      if (!trusted) throw commandError("Host key was rejected", null);
      delete model.hostKeys[server.id];
      return openSession(server);
    },
    confirm_host_key: ({ sessionId, decision }) => {
      const waiter = hostKeyWaiters.get(sessionId as string);
      if (!waiter) throw commandError(`no host key prompt pending for ${String(sessionId)}`);
      hostKeyWaiters.delete(sessionId as string);
      waiter((decision as { decision: string }).decision === "trust");
      return null;
    },
    // An open session: empty history, a terminal that echoes nothing, an
    // empty home directory in the file browser.
    get_chat_history: () => [],
    list_prompt_history: () => [],
    open_terminal: () => ({ missingDirectory: null }),
    terminal_resize: () => null,
    terminal_input: () => null,
    sftp_start_directory: () => ({ path: ".", missingDirectory: null }),
    sftp_list: () => [],
    sftp_elevation_disable: () => null,
    sftp_elevation_status: () => null,
    // Approve/deny of a pending action; the result event is up to the test.
    respond_to_action: () => null,
    disconnect: ({ sessionId }) => {
      model.sessions = model.sessions.filter((s) => s.sessionId !== sessionId);
      persist();
      return null;
    },
  };

  const invoke = async (cmd: string, rawArgs: unknown): Promise<unknown> => {
    const args = (rawArgs ?? {}) as Args;
    if (!cmd.startsWith("plugin:event|")) calls.push({ cmd, args });
    const canned = model.commands[cmd];
    if (canned) {
      if ("hold" in canned) {
        return new Promise((resolve, reject) => {
          held.set(cmd, [...(held.get(cmd) ?? []), { resolve, reject }]);
        });
      }
      if ("error" in canned) throw canned.error;
      return canned.result ?? null;
    }
    const handler = handlers[cmd];
    if (!handler) {
      unknownCommands.push(cmd);
      const message = `[e2e fake backend] unknown command "${cmd}" — add a handler or a per-test override`;
      console.error(message, args);
      throw commandError(message, "E2E_UNKNOWN_COMMAND");
    }
    const result = await handler(args);
    return result ?? null;
  };

  mockWindows("main");
  mockIPC((cmd, args) => invoke(cmd, args));

  const bridge: E2eBridge = {
    calls,
    unknownCommands,
    emit,
    listenerCount: (event) => listeners.get(event)?.length ?? 0,
    setCommand: (cmd, response) => {
      model.commands[cmd] = response;
      persist();
    },
    release: (cmd, response) => {
      const waiting = held.get(cmd) ?? [];
      held.delete(cmd);
      for (const { resolve, reject } of waiting) {
        if ("error" in response) reject(response.error);
        else resolve(response.result ?? null);
      }
      return waiting.length;
    },
    heldCount: (cmd) => held.get(cmd)?.length ?? 0,
    snapshot: () => JSON.parse(JSON.stringify(model)),
  };
  window.__e2e = bridge;
}
