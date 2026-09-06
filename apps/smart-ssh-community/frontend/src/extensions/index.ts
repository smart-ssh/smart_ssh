export {
  registerRoute,
  registerPanel,
  registerSettingsSection,
  registerCommandPaletteAction,
  registerDocumentAction,
  listRoutes,
  listPanels,
  listSettingsSections,
  listCommandPaletteActions,
  listDocumentActions,
  resetRegistryForTests,
} from "./registry";
export type {
  RouteContribution,
  PanelContribution,
  SettingsSectionContribution,
  CommandPaletteActionContribution,
  DocumentAction,
  DocumentContext,
} from "./registry";

export { useEntitlements } from "./useEntitlements";
export type { UseEntitlementsResult } from "./useEntitlements";

export type { Entitlements, Feature, Tier, FeatureLockedPayload } from "./entitlements";

export { publishFeatureLocked, subscribeFeatureLocked } from "./featureLockedBus";
export { FeatureLockedDialog } from "./FeatureLockedDialog";
