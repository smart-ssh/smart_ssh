// Issue #911: type-level tests for the typed translation keys. Checked by
// `tsc -b` (build and lint); there is no runtime behaviour and no test runner
// picks the file up.
import i18next from "i18next";
import { useTranslation } from "react-i18next";

declare module "./i18nTypes" {
  // Stand-in for an extension namespace (see `ExtensionNamespaces`).
  interface ExtensionNamespaces {
    exampleExt: { greeting: string };
  }
}

export function useTypeChecks() {
  // A key that exists in `locales/en/common.json` type-checks.
  i18next.t("nav.connect");

  // An unknown key is a type error.
  // @ts-expect-error -- "does.not.exist" is not in the English bundle
  i18next.t("does.not.exist");

  // Extension namespaces: explicit `ns`, checked against the extension's bundle.
  i18next.t("greeting", { ns: "exampleExt" });
  // @ts-expect-error -- "missing" is not in the extension's bundle
  i18next.t("missing", { ns: "exampleExt" });
  const { t } = useTranslation("exampleExt");
  t("greeting");
}
