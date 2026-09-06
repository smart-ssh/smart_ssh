export {
  registerSettingsSection,
  registerDocumentAction,
  listSettingsSections,
  listDocumentActions,
  resetRegistryForTests,
} from "./registry";
export type {
  SettingsSectionContribution,
  DocumentAction,
  DocumentContext,
} from "./registry";

export { useEntitlements } from "./useEntitlements";
export type { UseEntitlementsResult } from "./useEntitlements";

export type { Entitlements, Feature, Tier, FeatureLockedPayload } from "./entitlements";

export { publishFeatureLocked, subscribeFeatureLocked } from "./featureLockedBus";
export { FeatureLockedDialog } from "./FeatureLockedDialog";
