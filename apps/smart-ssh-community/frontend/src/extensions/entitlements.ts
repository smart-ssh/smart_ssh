import type { Entitlements, Feature, FeatureLocked, TierId } from "../generated/ipc";

/** Generated from `ssh_manager_core::entitlements` (Spec 0037, section 2);
 * see `src/generated/ipc.ts` (issue #919). */
export type { Entitlements, Feature };

/** A tier id (`TierId` on the Rust side). */
export type Tier = TierId;

/** The only tier the core knows; editions register their own (see `tiers.ts`). */
export const FREE_TIER: Tier = "free";

/** Embedded in `CommandError.feature_locked` (Spec 0037, sections 2/3);
 * `CommandError` has no camelCase rename, hence `feature_locked` in `api.ts`. */
export type FeatureLockedPayload = FeatureLocked;
