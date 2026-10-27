# ADR 0153 — Entitlement feature ids are open; editions register their own

## Status

Accepted

Betrifft: Issue #876, Spec 0037

## Problem

`Feature` was a closed enum of 19 variants. The public core checked none of
them in production code, and most were referenced nowhere. The list described
products the core does not contain, and every new edition feature meant a core
change.

## Decision

1. The public core only contains feature ids it checks itself. It currently
   has none.
2. `Feature` is an opaque identifier (`Cow<'static, str>`, `const fn new`,
   serialised as the plain id string). A `&'static str` with interning was
   rejected: it needs global state and makes deserialisation depend on
   registration order. `Copy` is dropped; `require`/`has` take `&Feature`.
3. Ids match `^[a-z][a-z0-9_]{0,63}$`. A single invalid id fails to
   deserialise; inside `Entitlements.features` unknown or malformed entries
   are skipped and the rest is kept.
4. Editions register the features they gate with
   `Wiring::entitlement_features`. Only registered features reach the
   frontend; the rest is dropped (debug log, id only). Gating itself is
   unchanged: fail closed, never influences `FilterEngine::evaluate()`.
5. Transition: the six ids extensions use today and `is_service_bound()`
   stay as `#[deprecated]` items and are removed in a later release.
