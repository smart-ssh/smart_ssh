// Spiegelt die Rust-DTOs aus `crates/app-tauri/src/dto.rs` (Spec 0007,
// Abschnitt 8.2). Feldnamen camelCase, weil die DTO-Structs dort
// `#[serde(rename_all = "camelCase")]` tragen; `ProviderType`-Werte
// bleiben snake_case (exakt wie im SQL-`CHECK`-Constraint), weil dieser
// eine Typ stattdessen `#[serde(rename = "...")]` pro Variante nutzt.

export type ProviderType =
  | "openai"
  | "anthropic"
  | "generic_openai_compatible"
  | "ollama";

export interface ServerDto {
  id: string;
  name: string;
  host: string;
  port: number;
  username: string;
  tags: string[];
}

export interface AiProviderConfigDto {
  id: string;
  providerType: ProviderType;
  displayName: string;
  baseUrl: string | null;
  model: string;
  supportsNativeToolCalling: boolean;
  isActive: boolean;
}

export interface AiProviderConfigInput {
  providerType: ProviderType;
  displayName: string;
  baseUrl: string | null;
  model: string;
  supportsNativeToolCalling: boolean;
  apiKey: string;
}

export const PROVIDER_TYPE_LABELS: Record<ProviderType, string> = {
  openai: "OpenAI",
  anthropic: "Anthropic",
  generic_openai_compatible: "Generisch (OpenAI-kompatibel)",
  ollama: "Ollama",
};

// Nur bei diesen beiden Typen ist Base-URL relevant (Spec 0007, Abschnitt
// 8.3: "Base-URL-Feld nur bei generic_openai_compatible/ollama sichtbar").
export function needsBaseUrl(type: ProviderType): boolean {
  return type === "generic_openai_compatible" || type === "ollama";
}

// --- Teil 2: Session/Terminal/Chat ------------------------------------
//
// `AiAction`/`NoteTarget`/`Decision` (core::profiles/core::filter) tragen
// kein `#[serde(rename_all/rename)]` — sie serialisieren mit Serdes
// Standard-Außen-Tagging: Unit-Varianten als bloßer String (`"AutoExec"`),
// Varianten mit Feldern als Objekt mit dem Variantennamen als einzigem
// Schlüssel (`{"Confirm": {"reason": "..."}}`). Feldnamen bleiben
// snake_case (`new_content`, nicht `newContent`). Die eigenen
// Event-Payloads in `crate::events` tragen dagegen `rename_all =
// "camelCase"`.

export type NoteTarget = { Server: string } | { Group: string };

export type AiAction =
  | { SuggestCommand: { command: string } }
  | { ProposeNoteUpdate: { target: NoteTarget; new_content: string } };

export type Decision =
  | "AutoExec"
  | { Confirm: { reason: string } }
  | { Deny: { reason: string } };

export type ConnectionStatus = "connected" | "disconnected";

export interface ConnectionStatusChangedEvent {
  sessionId: string;
  status: ConnectionStatus;
  reason: string | null;
}

export type HostKeyKind = "unknown" | "mismatch";

export interface HostKeyVerificationNeededEvent {
  sessionId: string;
  host: string;
  port: number;
  kind: HostKeyKind;
  fingerprint: string;
  /** Nur bei `kind === "mismatch"` gesetzt. */
  expectedFingerprint: string | null;
}

export interface TerminalOutputEvent {
  sessionId: string;
  /** Base64-kodiert (s. `crate::events`-Modul-Kommentar). */
  data: string;
}

export interface ChatTextDeltaEvent {
  sessionId: string;
  delta: string;
}

export interface ChatActionProposedEvent {
  sessionId: string;
  actionId: string;
  action: AiAction;
  decision: Decision;
}

export type ActionResultPayload =
  | { kind: "command"; command: string; stdout: string; stderr: string; exitCode: number | null }
  | { kind: "noteUpdate"; summary: string };

export interface ChatActionResultEvent {
  sessionId: string;
  actionId: string;
  result: ActionResultPayload;
}

export interface ChatErrorEvent {
  sessionId: string;
  message: string;
}

/** Antwort auf `host-key-verification-needed` (`confirm_host_key`). */
export type HostKeyUserDecision = { decision: "trust" } | { decision: "reject" };

/** Antwort auf ein `chat-action-proposed` mit `decision: Confirm` (`respond_to_action`). */
export type ActionUserDecision =
  | { decision: "approve" }
  | { decision: "deny" }
  | { decision: "editThenApprove"; command: string };
