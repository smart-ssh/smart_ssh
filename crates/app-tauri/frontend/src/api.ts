import { invoke } from "@tauri-apps/api/core";
import type {
  ActionUserDecision,
  AiProviderConfigDto,
  AiProviderConfigInput,
  HostKeyUserDecision,
  ServerDto,
} from "./types";

/** Von `crate::error::CommandError` (`crates/app-tauri/src/error.rs`). */
export interface CommandErrorPayload {
  message: string;
}

/** Extrahiert eine anzeigbare Meldung aus einem abgelehnten `invoke()`. */
export function commandErrorMessage(err: unknown): string {
  if (typeof err === "object" && err !== null && "message" in err) {
    const message = (err as CommandErrorPayload).message;
    if (typeof message === "string") return message;
  }
  return String(err);
}

export const listServers = () => invoke<ServerDto[]>("list_servers");

export const listAiProviders = () => invoke<AiProviderConfigDto[]>("list_ai_providers");

export const addAiProvider = (config: AiProviderConfigInput) =>
  invoke<string>("add_ai_provider", { config });

export const updateAiProvider = (id: string, config: AiProviderConfigInput) =>
  invoke<void>("update_ai_provider", { id, config });

export const deleteAiProvider = (id: string) => invoke<void>("delete_ai_provider", { id });

export const setActiveAiProvider = (id: string) =>
  invoke<void>("set_active_ai_provider", { id });

// --- Teil 2: Session/Terminal/Chat --------------------------------------

export const connect = (serverId: string) => invoke<string>("connect", { serverId });

export const confirmHostKey = (sessionId: string, decision: HostKeyUserDecision) =>
  invoke<void>("confirm_host_key", { sessionId, decision });

export const openTerminal = (sessionId: string) => invoke<void>("open_terminal", { sessionId });

export const terminalInput = (sessionId: string, data: Uint8Array) =>
  invoke<void>("terminal_input", { sessionId, data: Array.from(data) });

export const terminalResize = (sessionId: string, cols: number, rows: number) =>
  invoke<void>("terminal_resize", { sessionId, cols, rows });

export const sendChatMessage = (sessionId: string, text: string) =>
  invoke<void>("send_chat_message", { sessionId, text });

export const respondToAction = (
  sessionId: string,
  actionId: string,
  decision: ActionUserDecision,
) => invoke<void>("respond_to_action", { sessionId, actionId, decision });

export const disconnect = (sessionId: string) => invoke<void>("disconnect", { sessionId });
