# ADR 0136: Session extension point (context section, known secrets, removal)

## Status

Accepted · (issue #463, Spec 0038)

## Decisions

- **Trait gets a precomputed scope only** (server id, group chain root-first,
  tags), not the `ProfileStore`. The extension does not depend on
  `app-logic` internals.
- **One extension, `Option` in `Wiring` and `AppState`.** `None` is the
  Community default; every call site is a no-op without it, so prompt and
  redactor are byte-identical to before.
- **The section reuses the `server_note` fence** (own heading, label
  `Extension`) instead of a new `UntrustedKind`. A new kind would change the
  fence marker set and the prompt text that explains the fences (Spec 0039)
  for no gain in safety: the trust level is the same as for notes. The
  section is appended to the stable base part of the system context, after
  the core paragraphs and allow rules, so compaction never touches it. It
  counts as untrusted content ingested, like notes.
- **Redact, then fence** (ADR 0034): the section runs through the same
  per-server redactor as the session, including the extension's own secrets.
- **Known secrets are escaped literals**; empty values are skipped (an empty
  pattern would match everywhere). They are added to the sudo-password
  pattern, never replacing it.
- **Rebuild** is `app_shell::rebuild_server_redactors(app, server_id)`; it
  swaps the redactor of every open session of the server through
  `SwappableRedactor`. History is already re-redacted before each send.
- **Removal callbacks** run in the app-logic delete flows after the store
  delete. A group delete reports the group and each descendant group (they
  are removed by `ON DELETE CASCADE`); servers in the group are only
  unassigned, not deleted, so they get no callback.
- **Async context hook with the prompt language** (#526): `context_section`
  is an `#[async_trait]` method (like `OutgoingTransform::prepare_outgoing`)
  and receives the session's `PromptLanguage`, the value that picks the
  heading. Extension tables sit behind an async database handle, so a
  synchronous hook would need `block_in_place` or a duplicate cache.
  `append_extension_section` is async and both callers await it; order and
  redact-then-fence are unchanged. `known_secrets` and the removal callbacks
  stay synchronous.
