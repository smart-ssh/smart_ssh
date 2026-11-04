import type common from "./locales/en/common.json";
import type { ExtensionNamespaces } from "./i18nTypes";

// Issue #911: `t("…")` keys are checked against the English bundle. See the
// "Typed translation keys" note in the header of `i18n.ts` for extension
// namespaces and dynamic keys.
declare module "i18next" {
  interface CustomTypeOptions {
    defaultNS: "common";
    resources: { common: typeof common } & ExtensionNamespaces;
  }
}
