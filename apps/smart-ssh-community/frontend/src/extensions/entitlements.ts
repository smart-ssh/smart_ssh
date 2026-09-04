/** Spiegelt `ssh_manager_core::entitlements::{Tier, Feature, Entitlements}`
 * (Spec 0037, Abschnitt 2) — Feldnamen `camelCase` gemäß dem
 * `#[serde(rename_all = "camelCase")]` auf `Entitlements` selbst. */
export type Tier = "free" | "personal" | "pro" | "team" | "business" | "enterprise";

export type Feature =
  | "shared_inventory"
  | "shared_notes"
  | "curated_rule_packs"
  | "org_policy"
  | "multi_server_actions"
  | "managed_ai"
  | "org_ai_policy"
  | "team_agents"
  | "session_history"
  | "activity_report"
  | "session_handover"
  | "cloud_sync"
  | "document_export"
  | "audit_export"
  | "sso"
  | "self_hosted";

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
