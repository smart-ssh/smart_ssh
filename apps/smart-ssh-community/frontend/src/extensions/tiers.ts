/** Tier ids are defined by the edition that registers them (Spec 0037).
 * The core knows only `FREE_TIER`; every other id arrives from an edition's
 * entitlements and is shown with its registered label, or generically. */

export interface TierContribution {
  /** The tier id as it appears in `Entitlements.tier`. */
  id: string;
  /** i18n key (namespace `common`) of the tier's display label. */
  labelKey: string;
}

const tiers = new Map<string, TierContribution>();

export function registerTier(tier: TierContribution): void {
  tiers.set(tier.id, tier);
}

/** Test helper: forget all registered tiers. */
export function resetTiersForTests(): void {
  tiers.clear();
}

/** Display label of a tier id: the registered label translated with `t`,
 * otherwise the generic fallback (the id itself). */
export function tierLabel(id: string, t: (key: string) => string): string {
  const reg = tiers.get(id);
  return reg ? t(reg.labelKey) : id;
}
