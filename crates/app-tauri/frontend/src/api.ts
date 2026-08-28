import { invoke } from "@tauri-apps/api/core";
import type { AiProviderConfigDto, AiProviderConfigInput, ServerDto } from "./types";

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
