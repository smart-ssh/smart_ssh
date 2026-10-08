export {
  registerSettingsSection,
  registerDocumentAction,
  listSettingsSections,
  listDocumentActions,
  registerFirstRunNoticeExtension,
  listFirstRunNoticeExtensions,
  resetRegistryForTests,
} from "./registry";
export type {
  SettingsSectionContribution,
  DocumentAction,
  DocumentContext,
  FirstRunNoticeExtension,
  FirstRunNoticeExtensionContext,
  FirstRunNoticeContinueHandler,
} from "./registry";

export { useEntitlements } from "./useEntitlements";
export type { UseEntitlementsResult } from "./useEntitlements";

export type { Entitlements, Feature, Tier, FeatureLockedPayload } from "./entitlements";

export { publishFeatureLocked, subscribeFeatureLocked } from "./featureLockedBus";
export { FeatureLockedDialog } from "./FeatureLockedDialog";

/** Issue #160: beigesteuerte modale Dialoge nutzen denselben Backdrop wie
 * die eingebauten — dann bleibt die Titelleiste auch bei ihnen ziehbar. */
export { ModalBackdrop } from "../components/ModalBackdrop";
