# ADR 0155 — Entitlement tier ids are open; editions register their own

## Status

Accepted

Betrifft: Issue #928, Spec 0037, ADR 0153

## Problem

`Tier` was a closed enum of six tiers, mirrored by hand as a string union in
the frontend. The core only ever uses the free state.

## Decision

1. `Tier` is replaced by `TierId`, an opaque identifier (`Cow<'static, str>`,
   serialised as the plain string). The core defines one constant,
   `TierId::FREE` (`"free"`). Every value the enum serialised stays valid;
   `Entitlements.tier`, `entitlements:changed` and the `FeatureLocked`
   payload are wire-compatible. No migration.
2. Unlike feature ids, tier ids are not pattern-validated and unknown ones are
   never dropped: the tier is a single required field, and rejecting it would
   make the whole entitlements state undecodable (fail-closed gating is
   unaffected, it depends on the features only).
3. Frontend: `Tier` is `string`; `FREE_TIER` is the only constant. Editions
   call `registerTier({ id, labelKey })`; `tierLabel` returns the translated
   label, or the id itself as generic fallback for an unregistered tier.
4. There is no Rust-side tier registration: labels are presentation only.
   A guard test (`tierListGuard.test.ts`) fails if a tier enum or union
   returns.
