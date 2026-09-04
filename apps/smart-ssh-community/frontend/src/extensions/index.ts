export {
  registerRoute,
  registerPanel,
  registerSettingsSection,
  registerCommandPaletteAction,
  listRoutes,
  listPanels,
  listSettingsSections,
  listCommandPaletteActions,
  resetRegistryForTests,
} from "./registry";
export type {
  RouteContribution,
  PanelContribution,
  SettingsSectionContribution,
  CommandPaletteActionContribution,
} from "./registry";

export { useEntitlements } from "./useEntitlements";
export type { UseEntitlementsResult } from "./useEntitlements";

export type { Entitlements, Feature, Tier, FeatureLockedPayload } from "./entitlements";

export { publishFeatureLocked, subscribeFeatureLocked } from "./featureLockedBus";
export { FeatureLockedDialog } from "./FeatureLockedDialog";
