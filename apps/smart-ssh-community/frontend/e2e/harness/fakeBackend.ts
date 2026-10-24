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
  ChatSessionSummaryDto,
} from "../../src/types";
import type {
  CannedResponse,
  E2eBridge,
  FakeBackendFixture,
  FakeHostKey,
  RecordedCall,
} from "./fixtureTypes";

const MCP_SETTINGS = {
  enabled: false,
  endpoint: "http://127.0.0.1:7777",
  token: "e2e-token-not-a-secret",
  allowedServerIds: [] as string[],
  confirmTimeoutSecs: 120,
};

const PERSIST_KEY = "smart-ssh-e2e-fake-backend";

interface Model {
  servers: ServerDto[];
  groups: GroupDto[];
  sessions: SessionSummaryDto[];
  chatSessions: Record<string, ChatSessionSummaryDto[]>;
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
    chatSessions: fixture.chatSessions ?? {},
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
  let backupKey = { exists: false, hasCode: false, hasPassphrase: false };
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
    answer_startup_prompt: () => null,
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
    list_stored_host_keys: () => [],
    list_groups: () => model.groups,
    // The in-memory model only holds readable servers; tests that need an
    // unusable one override this command.
    list_unusable_servers: () => [],
    list_known_tags: () => [...new Set(model.servers.flatMap((s) => s.tags))].sort(),
    explain_confirmation: () => ({ segments: [], suggestions: [] }),
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

    keep_server_as_regular_entry: ({ id }) => {
      findServer(id).isCopy = false;
      persist();
      return null;
    },
    keep_group_as_regular_entry: ({ id, confirm }) => {
      const inside = new Set<string>([String(id)]);
      for (let grew = true; grew; ) {
        grew = false;
        for (const g of model.groups) {
          if (g.parentId && inside.has(g.parentId) && !inside.has(g.id)) {
            inside.add(g.id);
            grew = true;
          }
        }
      }
      const groups = model.groups.filter((g) => inside.has(g.id) && g.isCopy);
      const servers = model.servers.filter((s) => s.groupId && inside.has(s.groupId) && s.isCopy);
      if (confirm) {
        for (const x of [...groups, ...servers]) x.isCopy = false;
        persist();
      }
      return { applied: Boolean(confirm), groups: groups.length, servers: servers.length };
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
    // --- Encrypted backup (Spec 0109, #666) ------------------------------
    // Stateful enough for export -> import: the key exists after setup, a
    // "TYPO" secret reports a typo, "wrong" cannot open, anything else opens.
    backup_key_status: () => ({ ...backupKey }),
    create_backup_key_with_code: () => {
      backupKey = { exists: true, hasCode: true, hasPassphrase: backupKey.hasPassphrase };
      return "ABCDE-FGHJK-MNPQR-STVWX-YZ234-56789-ABCDE";
    },
    new_backup_code: () => {
      backupKey = { ...backupKey, exists: true, hasCode: true };
      return "23456-789AB-CDEFG-HJKMN-PQRST-VWXYZ-23456";
    },
    set_backup_passphrase: () => {
      backupKey = { ...backupKey, exists: true, hasPassphrase: true };
      return null;
    },
    export_encrypted_backup: () => {
      if (!backupKey.exists) throw commandError("no key", "BACKUP_KEY_REQUIRED");
      return {
        path: "/home/e2e/backups/smart-ssh-backup.sshbackup",
        servers: model.servers.length,
        groups: model.groups.length,
        rules: 0,
        notes: 0,
        aiProviders: model.aiProviders.length,
        secrets: 0,
        hostKeys: 0,
      };
    },
    pick_backup_import_file: () => "smart-ssh-backup.sshbackup",
    start_backup_import: ({ secret }) => {
      if (String(secret).startsWith("TYPO")) throw commandError("typo", "BACKUP_CODE_TYPO");
      if (String(secret).startsWith("wrong")) throw commandError("cannot open", "BACKUP_CANNOT_OPEN");
      return {
        imported: null,
        preview: {
          local: { servers: model.servers.length, groups: model.groups.length, rules: 0, notes: 0, aiProviders: 0 },
          file: { servers: 15, groups: 2, rules: 42, notes: 11, aiProviders: 1 },
          openSessions: model.sessions.length,
          safetyBackupDir: "/home/e2e/Documents/Smart SSH backups",
        },
      };
    },
    confirm_backup_import: () => {
      if (!backupKey.exists) throw commandError("no key", "BACKUP_KEY_REQUIRED");
      return {
        counts: { servers: 15, groups: 2, rules: 42, notes: 11, aiProviders: 1 },
        safetyBackup: "/home/e2e/Documents/Smart SSH backups/smart-ssh-before-import.sshbackup",
        previousDataFolder: "Previous data 2026-01-01",
        hostKeysPending: false,
      };
    },
    cancel_backup_import: () => null,
    get_chat_session_retention_days: () => null,
    get_mcp_server_settings: () => MCP_SETTINGS,
    // The timeout field commits on blur, so tabbing through it calls this.
    set_mcp_server_confirm_timeout_secs: () => MCP_SETTINGS,

    // --- Sessions --------------------------------------------------------
    list_sessions: () => model.sessions,
    list_chat_sessions: ({ serverId }) => model.chatSessions[serverId as string] ?? [],
    resume_chat_session: ({ serverId }) => openSession(findServer(serverId)),
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
          keyType: hostKey.keyType ?? null,
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
    send_chat_message: () => null,
    get_chat_history: () => [],
    list_prompt_history: () => [],
    open_terminal: () => ({ missingDirectory: null }),
    terminal_resize: () => null,
    terminal_input: () => null,
    session_is_root: () => false,
    sftp_start_directory: () => ({ path: ".", missingDirectory: null }),
    sftp_list: () => [],
    sftp_local_paths_are_folders: (args) =>
      (args.localPaths as string[]).map(() => false),
    clipboard_has_files: () => false,
    clipboard_write_text: () => null,
    sftp_elevation_disable: () => ({ disabled: true, runningOperations: [] }),
    sftp_cancel_transfer: () => true,
    sftp_elevation_status: () => null,
    // Approve/deny of a pending action; the result event is up to the test.
    respond_to_action: () => null,
    // Issue #355: the status events are up to the test.
    cancel_connect: () => null,
    reconnect_session: () => null,
    stop_auto_reconnect: () => null,
    get_selected_speech_model: () => "base",
    get_speech_language: () => "auto",
    get_speech_model_status: (args) => ({ model: (args as { model: string }).model, state: "not_downloaded" }),
    set_selected_speech_model: () => null,
    set_speech_language: () => null,
    start_speech_model_download: () => null,
    cancel_speech_model_download: () => null,
    delete_speech_model: () => null,
    start_voice_recording: () => null,
    stop_voice_recording: () => "",
    cancel_voice_recording: () => null,
    disconnect: ({ sessionId }) => {
      model.sessions = model.sessions.filter((s) => s.sessionId !== sessionId);
      persist();
      return null;
    },
  };

  let inFlight = 0;
  const invoke = async (cmd: string, rawArgs: unknown): Promise<unknown> => {
    inFlight++;
    try {
      return await invokeInner(cmd, rawArgs);
    } finally {
      inFlight--;
    }
  };
  const invokeInner = async (cmd: string, rawArgs: unknown): Promise<unknown> => {
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
    settle: async () => {
      // Quiet = no call in flight and no new call over two animation frames,
      // so the app's start-up data has been rendered.
      const deadline = performance.now() + 10_000;
      for (let quiet = 0; quiet < 2; ) {
        if (performance.now() > deadline) {
          throw new Error(`settle(): start-up commands still in flight after 10 s (${inFlight} pending)`);
        }
        const before = calls.length;
        await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
        const held_ = [...held.values()].reduce((n, l) => n + l.length, 0);
        // A deliberately held call is not "loading".
        quiet = inFlight - held_ === 0 && calls.length === before ? quiet + 1 : 0;
      }
    },
    heldCount: (cmd) => held.get(cmd)?.length ?? 0,
    snapshot: () => JSON.parse(JSON.stringify(model)),
  };
  window.__e2e = bridge;
}
