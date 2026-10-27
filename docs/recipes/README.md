# Recipes

A recipe is a checklist for a change that always touches the same set of
files. It exists so nobody forgets a step; it does not replace the project
rules in [`AGENTS.md`](../../AGENTS.md) or the specs in `docs/specs/`.

Every recipe has the same five sections:

1. **Purpose** – what kind of change the recipe covers.
2. **Files to touch** – a checklist with repo paths, in the order to work.
3. **Required tests** – what must exist before the change is done.
4. **Guard that catches a forgotten step** – the script or test that fails
   (and what it reports). Where a step has no guard, the recipe says so.
5. **Example** – a merged PR in this repo that did it right.

## Index

| Recipe | Use it when |
|---|---|
| [`new-tauri-command.md`](new-tauri-command.md) | The frontend needs a new backend entry point |
| [`new-ui-text.md`](new-ui-text.md) | A user-visible string is added or changed |
| [`new-migration.md`](new-migration.md) | The SQLite schema changes |

Paths in recipes are checked by extracting every backticked path and testing
that it exists, for example:

```bash
grep -oh '`[A-Za-z0-9_./<>-]*/[A-Za-z0-9_./-]*`' docs/recipes/*.md | tr -d '`' | grep -v '[<>]' | sort -u | while read -r p; do [ -e "$p" ] || echo "missing: $p"; done
```

(Paths containing a `<placeholder>` are patterns and skipped.)
