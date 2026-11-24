import { useTranslation } from "react-i18next";
import type { PostIngestPolicy } from "../../api/types";
import { Icon, TabPanel } from "../ui";
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
        <fieldset className="min-w-0 rounded border border-border p-3">
          <legend className="px-1 text-label">{t("serverForm.postIngestFieldset")}</legend>
          <p className="mb-3 text-xs text-fg-subtle">{t("serverForm.postIngestHint")}</p>
          <div className="grid gap-2 sm:grid-cols-3 narrow:grid-cols-1">
            {(["strict", "balanced", "standard"] as PostIngestPolicy[]).map((option) => {
              const selected = postIngestPolicy === option;
              return (
                <label
                  key={option}
                  className={`flex cursor-pointer items-start gap-2 border p-3 text-sm focus-within:outline focus-within:outline-2 focus-within:outline-accent narrow:min-h-11 ${
                    selected
                      ? "border-accent bg-surface-raised text-fg-strong"
                      : "border-border-strong bg-surface-sunken text-fg-muted hover:border-fg-subtle"
                  }`}
                >
                  <input
                    type="radio"
                    name="postIngestPolicy"
                    value={option}
                    checked={selected}
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
              );
            })}
          </div>
        </fieldset>

        <fieldset className="min-w-0 rounded border border-border p-3">
          <legend className="px-1 text-label">{t("serverForm.aiInjectionCheckFieldset")}</legend>
          <label className="flex items-start gap-2 text-sm text-fg-muted narrow:min-h-11">
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
          <section aria-labelledby="stored-host-keys-heading" className="space-y-2">
            <h3 id="stored-host-keys-heading" className="text-label">
              {t("serverForm.hostKeys.title")}
            </h3>
            {storedHostKeys.length === 0 ? (
              <p className="text-xs text-fg-subtle">{t("serverForm.hostKeys.none")}</p>
            ) : (
              <ul className="divide-y divide-border border border-border">
                {storedHostKeys.map((k) => (
                  <li
                    key={`${k.algorithm}:${k.fingerprint}`}
                    className="flex items-start gap-2 px-3 py-2 text-xs text-fg-muted"
                  >
                    <Icon name="key" size={16} className="mt-0.5 shrink-0 text-fg-subtle" />
                    <div className="min-w-0 flex-1">
                      <div className="font-mono">{k.algorithm}</div>
                      <div className="select-text break-all font-mono">{k.fingerprint}</div>
                    </div>
                  </li>
                ))}
              </ul>
            )}
          </section>
        )}
    </TabPanel>
  );
}
