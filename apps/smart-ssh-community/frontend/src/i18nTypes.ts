import type { ParseKeys } from "i18next";

/** Resource bundles of extension namespaces, keyed by namespace. Empty in the
 * core; an extension adds its namespace by declaration merging, which makes
 * `useTranslation("ns")` / `t(key, { ns })` type-check for it:
 *
 *     declare module "<path to>/i18nTypes" {
 *       interface ExtensionNamespaces { myExt: typeof import("./locales/en/myExt.json") }
 *     }
 */
// eslint-disable-next-line @typescript-eslint/no-empty-object-type
export interface ExtensionNamespaces {}

/** Every key that exists in the English `common` bundle (Issue #911). */
export type TranslationKey = ParseKeys;

/** The shape of `t` that helper functions take as a parameter. Unlike the
 * old `(key: string, …)` signatures it only accepts known keys, and both the
 * real `TFunction` and plain test fakes are assignable to it. */
export type TranslateFn = (key: TranslationKey, options?: Record<string, unknown>) => string;

/** Narrow escape hatch for keys assembled at run time (`errors.${code}` and
 * the like). Callers must make sure the resulting key exists; the locale
 * parity test and the unit tests of the caller cover the concrete values. */
export function dynamicKey(key: string): TranslationKey {
  return key as TranslationKey;
}
