/**
 * The only supported import surface for extensions and embedding apps
 * (import it as `smart-ssh/public-api`). Internal module paths are not part of
 * the contract and may be renamed at any time.
 *
 * Stability rule: exports of this file change only together with a changelog
 * entry and an update of Spec 0038 (section 4).
 *
 * Re-exports only; no code lives here.
 */

export * from "./extensions";
export type { Entitlements, Feature, Tier, FeatureLockedPayload } from "./extensions/entitlements";

export { bootstrap } from "./bootstrap";
export type { BootstrapOptions } from "./bootstrap";

export * from "./components/ui";

export {
  invokeCommand,
  commandErrorMessage,
  commandErrorCode,
  commandErrorFeatureLocked,
  commandErrorConnectLog,
  listServers,
  getStartupState,
} from "./api";

export { TECHNICAL_INPUT_PROPS } from "./technicalInputProps";
export { useIsNarrow } from "./hooks/useIsNarrow";
export { resolveSectionLabel } from "./resolveSectionLabel";
export { pickAndReadTextFile } from "./fileDialog";
export { SUPPORTED_LANGUAGES } from "./languages";
export { settingsStore, initI18n } from "./i18n";

export type { AiProviderConfigDto, ChatSessionSummaryDto, StartupStateDto } from "./types";
