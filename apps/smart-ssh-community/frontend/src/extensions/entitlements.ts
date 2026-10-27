/** Spiegelt `ssh_manager_core::entitlements::{TierId, Feature, Entitlements}`
 * (Spec 0037, Abschnitt 2) — Feldnamen `camelCase` gemäß dem
 * `#[serde(rename_all = "camelCase")]` auf `Entitlements` selbst. */
export type Tier = string;

/** The only tier the core knows; editions register their own (see `tiers.ts`). */
export const FREE_TIER: Tier = "free";

/** Feature ids are defined by the edition that registers them (Spec 0037). */
export type Feature = string;

export interface Entitlements {
  tier: Tier;
  features: Feature[];
  seats: number | null;
  expiresAt: string | null;
  nonCommercial: boolean;
  licensee: string | null;
}

/** Von `crate::entitlements::FeatureLocked` (`ssh_manager_core`) —
 * eingebettet in `crate::error::CommandError.feature_locked` (Spec 0037,
 * Abschnitt 2/3 — `CommandError` selbst hat kein `camelCase`-Rename, anders
 * als `Entitlements` oben, daher der Feldname `feature_locked` statt
 * `featureLocked` beim Zugriff über `CommandErrorPayload` in `api.ts`). */
export interface FeatureLockedPayload {
  feature: Feature;
  tier: Tier;
}
