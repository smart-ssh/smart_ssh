/**
 * The only supported import surface for extensions and embedding apps
 * (import it as `smart-ssh/public-api`). Internal module paths are not part of
 * the contract and may be renamed at any time.
 *
 * Stability rule: exports of this file change only together with a changelog
 * entry and an update of Spec 0038 (section 4).
 *
 * Re-exports only; no code lives here.
 *
 * Deprecated re-export stubs at the old flat `src/` paths (issue #916), kept so
 * embedding apps keep building: api, types, languages, navigationBus,
 * fileDialog, technicalInputProps, resolveSectionLabel.
 */

export * from "./extensions";
export type { Entitlements, Feature, Tier, FeatureLockedPayload } from "./extensions/entitlements";

export { showToast } from "./buses/toastBus";
export type { ToastInput } from "./buses/toastBus";

export { startActivity } from "./lib/startActivity";
export type { ActivityHandle, StartActivitySpec } from "./lib/startActivity";
export { useActivities, useRunningActivity, useIsActivityRunning } from "./hooks/useActivities";
export type { Activity, ActivityState } from "./api/activitiesApi";

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
  listGroups,
  listAiProviders,
  listChatSessions,
} from "./api/api";

export { TECHNICAL_INPUT_PROPS } from "./lib/technicalInputProps";
export {
  detectFallbackPlatform,
  platformCapabilities,
  usePlatform,
  isMacLike,
  isWindowsLike,
} from "./lib/platform";
export type { Platform, PlatformCapabilities } from "./lib/platform";
export { useIsNarrow } from "./hooks/useIsNarrow";
export { resolveSectionLabel } from "./lib/resolveSectionLabel";
export { pickAndReadTextFile } from "./api/fileDialog";
export { SUPPORTED_LANGUAGES } from "./lib/languages";
export { settingsStore, initI18n, setUiPreference } from "./i18n";

export type {
  AiProviderConfigDto,
  ChatSessionSummaryDto,
  GroupDto,
  StartupStateDto,
} from "./api/types";
export type { ComposerContext, ComposerAction } from "./extensions/registry";
