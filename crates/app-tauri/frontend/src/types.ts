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
