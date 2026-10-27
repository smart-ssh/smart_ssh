export {
  registerSettingsSection,
  registerDocumentAction,
  listSettingsSections,
  listSettingsSectionsForCategory,
  unregisterSettingsSection,
  listDocumentActions,
  unregisterDocumentAction,
  subscribeDocumentActions,
  registerFirstRunNoticeExtension,
  listFirstRunNoticeExtensions,
  unregisterFirstRunNoticeExtension,
  subscribeFirstRunNoticeExtensions,
  registerServerSettingsTab,
  unregisterServerSettingsTab,
  listServerSettingsTabs,
  registerComposerAction,
  registerComposerDropHandler,
  unregisterComposerDropHandler,
  subscribeSettingsSections,
  subscribeComposer,
  registerServerFormSection,
  registerGroupFormSection,
  registerManageView,
  registerSessionHeaderItem,
  unregisterSessionHeaderItem,
  listSessionHeaderItems,
  listServerFormSections,
  listGroupFormSections,
  listManageViews,
  subscribeFormSlots,
  resetRegistryForTests,
} from "./registry";
export type {
  SettingsSectionContribution,
  ServerFormSectionContext,
  ServerFormSectionContribution,
  GroupFormSectionContext,
  GroupFormSectionContribution,
  ManageViewContribution,
  SessionHeaderItemContribution,
  DocumentAction,
  DocumentContext,
  FirstRunNoticeExtension,
  FirstRunNoticeExtensionContext,
  FirstRunNoticeContinueHandler,
} from "./registry";

/** Issue #611: asks the app to resume a saved chat session of a server,
 * exactly like choosing it in the session picker. */
export { requestResumeChatSession } from "../navigationBus";

export { useEntitlements } from "./useEntitlements";
export type { UseEntitlementsResult } from "./useEntitlements";

export type { Entitlements, Feature, Tier, FeatureLockedPayload } from "./entitlements";
export { FREE_TIER } from "./entitlements";
export { registerTier, tierLabel } from "./tiers";
export type { TierContribution } from "./tiers";

export { publishFeatureLocked, subscribeFeatureLocked } from "./featureLockedBus";
export { FeatureLockedDialog } from "./FeatureLockedDialog";

/** Issue #160: beigesteuerte modale Dialoge nutzen denselben Backdrop wie
 * die eingebauten — dann bleibt die Titelleiste auch bei ihnen ziehbar. */
export { ModalBackdrop } from "../components/ModalBackdrop";
