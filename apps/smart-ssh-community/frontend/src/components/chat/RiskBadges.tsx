import { useTranslation } from "react-i18next";
import type { RiskAssessment } from "../../api/types";
import { Tooltip } from "../ui";

export const RISK_LEVEL_BADGE_CLASS: Record<Exclude<RiskAssessment["serverRisk"], "none">, string> = {
  yellow: "bg-warning-subtle-hover text-warning-fg-soft",
  red: "bg-danger-subtle-hover text-danger-fg-soft",
};

/** Spec 0026, Abschnitt 4: zwei getrennte Badges ("Server"/"Daten"), nur
 * sichtbar bei Level ≠ `none`, Tooltip mit der Begründung. Bewusst KEIN
 * drittes "grün"-Badge (s. Spec Abschnitt 1) — Abwesenheit heißt "laut
 * bekannten Mustern unauffällig", kein Sicherheitsversprechen (s.
 * `RiskHintFootnote` für den entsprechenden Text).
 *
 * Spec 0029: sitzt jetzt inline in derselben Zeile wie das Aktions-Label
 * und der Entscheidungs-Badge, statt als eigener Block über dem
 * Kommando-Text — deshalb kein eigenes `mb-*` mehr hier (das übernimmt die
 * Zeile als Ganzes), und `null` statt eines leeren Containers, wenn beide
 * Achsen `none` sind (kein Layout-Sprung, Spec Abschnitt 2). */
export function RiskBadge({
  reason,
  level,
  label,
}: {
  reason: string | null | undefined;
  level: "yellow" | "red";
  label: string;
}) {
  const cls = `font-heading px-1.5 py-0.5 text-xs font-semibold tracking-wide uppercase ${RISK_LEVEL_BADGE_CLASS[level]}`;
  if (!reason) return <span className={cls}>{label}</span>;
  return (
    <Tooltip content={reason} className={cls}>
      {label}
      <span aria-hidden="true" className="ml-1 normal-case">
        ⓘ
      </span>
    </Tooltip>
  );
}

export function RiskBadges({
  assessment,
  pending,
  unavailable,
}: {
  assessment: RiskAssessment | null;
  pending: boolean;
  unavailable: boolean;
}) {
  const { t } = useTranslation();
  if (!assessment) return null;

  const hasServerBadge = assessment.serverRisk !== "none";
  const hasDataBadge = assessment.dataRisk !== "none";
  if (!hasServerBadge && !hasDataBadge && !pending) return null;

  return (
    <span className="flex flex-wrap items-center gap-1.5">
      {hasServerBadge && (
        <RiskBadge
          reason={assessment.serverRiskReason}
          level={assessment.serverRisk as "yellow" | "red"}
          label={t("actionCard.riskServerLabel")}
        />
      )}
      {hasDataBadge && (
        <RiskBadge
          reason={assessment.dataRiskReason}
          level={assessment.dataRisk as "yellow" | "red"}
          label={t("actionCard.riskDataLabel")}
        />
      )}
      {unavailable && (
        <output
          title={t("actionCard.riskSecondOpinionUnavailableHint")}
          className="font-sans text-xs text-warning-fg-soft"
        >
          {t("actionCard.riskSecondOpinionUnavailable")}
        </output>
      )}
      {pending && (
        <span
          title={t("actionCard.riskSecondOpinionPending")}
          className="inline-flex items-center gap-1 text-xs text-fg-subtle"
        >
          <span className="h-1.5 w-1.5 animate-pulse rounded-full bg-fg-faint" />
        </span>
      )}
    </span>
  );
}

/** Der zurückhaltende Fußnotentext aus der bisherigen `RiskBadges` — bleibt
 * eine eigene, unterhalb der Label-Zeile stehende Zeile (Spec 0029 regelt
 * nur die Position der Badges selbst, s. dortiger Abschnitt 2; der
 * vollständige Satz würde die Label-Zeile sprengen). `null`, wenn kein
 * Badge gezeigt wird — derselbe "kein leerer Platz"-Grundsatz wie bei
 * `RiskBadges`. */
export function RiskHintFootnote({ assessment }: { assessment: RiskAssessment | null }) {
  const { t } = useTranslation();
  if (!assessment) return null;
  if (assessment.serverRisk === "none" && assessment.dataRisk === "none") return null;
  return <p className="mb-1 text-right text-xs text-fg-subtle">{t("actionCard.riskHint")}</p>;
}

