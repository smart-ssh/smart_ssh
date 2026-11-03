import { useTranslation } from "react-i18next";
import type { PostIngestPolicy } from "../../types";
import { TabPanel } from "../ui";
import type { ServerModel } from "./formTypes";

interface Props {
  idBase: string;
  active: boolean;
  isCreate: boolean;
  model: ServerModel;
}

/** Security tab: post-ingest policy, injection check and stored host keys. */
export function SecurityTab({ idBase, active, isCreate, model }: Props) {
  const { t } = useTranslation();
  const {
    isLocal,
    storedHostKeys,
    postIngestPolicy,
    setPostIngestPolicy,
    aiInjectionCheckEnabled,
    setAiInjectionCheckEnabled,
    secondOpinionAvailable,
  } = model;
  return (
    <TabPanel idBase={idBase} id="security" active={active} className="space-y-3">
        <fieldset className="rounded border border-border p-3">
          <legend className="px-1 text-sm text-fg-muted">
            {t("serverForm.postIngestFieldset")}
          </legend>
          <p className="mb-2 text-xs text-fg-subtle">{t("serverForm.postIngestHint")}</p>
          <div className="space-y-2">
            {(["strict", "balanced", "standard"] as PostIngestPolicy[]).map((option) => (
              <label key={option} className="flex items-start gap-2 text-sm text-fg-muted">
                <input
                  type="radio"
                  name="postIngestPolicy"
                  value={option}
                  checked={postIngestPolicy === option}
                  onChange={() => setPostIngestPolicy(option)}
                  className="mt-1"
                />
                <span>
                  <span className="font-medium">{t(`serverForm.postIngest.${option}.label`)}</span>
                  <br />
                  <span className="text-xs text-fg-subtle">
                    {t(`serverForm.postIngest.${option}.description`)}
                  </span>
                </span>
              </label>
            ))}
          </div>
        </fieldset>

        <fieldset className="rounded border border-border p-3">
          <legend className="px-1 text-sm text-fg-muted">
            {t("serverForm.aiInjectionCheckFieldset")}
          </legend>
          <label className="flex items-start gap-2 text-sm text-fg-muted">
            <input
              type="checkbox"
              checked={aiInjectionCheckEnabled}
              disabled={!secondOpinionAvailable}
              onChange={(e) => setAiInjectionCheckEnabled(e.target.checked)}
              className="mt-1"
            />
            <span>{t("serverForm.aiInjectionCheckLabel")}</span>
          </label>
          <p className="mt-2 text-xs text-fg-subtle">
            {secondOpinionAvailable
              ? t("serverForm.aiInjectionCheckHint")
              : t("serverForm.aiInjectionCheckUnavailableHint")}
          </p>
        </fieldset>

        {!isCreate && !isLocal && storedHostKeys !== null && (
          <section aria-labelledby="stored-host-keys-heading" className="space-y-1">
            <h3 id="stored-host-keys-heading" className="text-sm font-medium text-fg-muted">
              {t("serverForm.hostKeys.title")}
            </h3>
            {storedHostKeys.length === 0 ? (
              <p className="text-xs text-fg-subtle">{t("serverForm.hostKeys.none")}</p>
            ) : (
              <ul className="space-y-1">
                {storedHostKeys.map((k) => (
                  <li key={`${k.algorithm}:${k.fingerprint}`} className="text-xs text-fg-muted">
                    <span className="font-mono">{k.algorithm}</span>{" "}
                    <span className="select-text break-all font-mono">{k.fingerprint}</span>
                  </li>
                ))}
              </ul>
            )}
          </section>
        )}
    </TabPanel>
  );
}
