// Shared between the Playwright tests (Node) and the fake backend that runs
// in the browser page. Only plain, JSON-serialisable data crosses that
// boundary: the fixture is handed to the page via `addInitScript`, and the
// fake backend persists its state in `sessionStorage` so a reload keeps it.
import type {
  AiProviderConfigDto,
  GroupDto,
  HostKeyKind,
  ServerDto,
  SessionSummaryDto,
} from "../../src/types";

/** A canned answer for one command, replacing the model's default handler. */
export type CannedResponse =
  /** Resolve with this value (`undefined` is sent as `null`). */
  | { result: unknown }
  /** Reject with this value — the shape of a backend `CommandError`. */
  | { error: unknown }
  /** Keep the call pending until the test calls `__e2e.release(cmd, …)`. */
  | { hold: true };

/** What `connect` does for a server with a host key that is not yet trusted. */
export interface FakeHostKey {
  kind: HostKeyKind;
  fingerprint: string;
  expectedFingerprint?: string | null;
  /** Algorithm of the offered key; defaults to `null` (undeterminable). */
  keyType?: string | null;
}

export interface FakeBackendFixture {
  servers?: ServerDto[];
  groups?: GroupDto[];
  /** Sessions the backend already holds at start-up (`list_sessions`). */
  sessions?: SessionSummaryDto[];
  aiProviders?: AiProviderConfigDto[];
  /** Contents of the frontend settings store (`settings.json`). */
  settings?: Record<string, unknown>;
  /** What the OS reports as its locale (`plugin:os|locale`). */
  locale?: string;
  /** Per server id: `connect` asks for this host key before it succeeds. */
  hostKeys?: Record<string, FakeHostKey>;
  /** Per-test overrides, keyed by command name. */
  commands?: Record<string, CannedResponse>;
}

export interface RecordedCall {
  cmd: string;
  args: Record<string, unknown>;
}

/** The test-facing API the harness installs as `window.__e2e`. */
export interface E2eBridge {
  /** Every command the app invoked since the page loaded, in order. */
  calls: RecordedCall[];
  /** Commands the fake backend does not know. Must stay empty. */
  unknownCommands: string[];
  /** Emits a backend event to every registered listener; returns their count. */
  emit(event: string, payload: unknown): number;
  /** How many listeners are currently registered for `event`. */
  listenerCount(event: string): number;
  /** Replaces the handler of one command for the rest of the page's life. */
  setCommand(cmd: string, response: CannedResponse): void;
  /** Settles every held call of `cmd`; returns how many were held. */
  release(cmd: string, response: { result: unknown } | { error: unknown }): number;
  /** How many calls of `cmd` are currently held. */
  heldCount(cmd: string): number;
  /** A snapshot of the in-memory model. */
  snapshot(): unknown;
}

declare global {
  interface Window {
    __E2E_FIXTURE__?: FakeBackendFixture;
    __e2e?: E2eBridge;
  }
}
