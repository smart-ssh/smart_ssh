# UI guidelines

Conventions for buttons, dialogs and form fields. The shared building blocks
live in `apps/smart-ssh-community/frontend/src/components/ui/` (`Button`,
`DialogFrame`, `Field`) and implement everything below; use them instead of
hand-written Tailwind class strings. Behaviour of the product is described in
`docs/specs/`; this document is about how screens look and respond.

The theme tokens (slate surfaces, indigo accent, emerald/amber/red for
allow/confirm/deny per Spec 0009, nearly square radii, Barlow / Barlow
Condensed / JetBrains Mono) are defined in `src/index.css`. The app has a dark
theme only.

## Buttons

| Variant | Use for | Look |
|---|---|---|
| `primary` | The one action the user most likely wants in this view (Save, Connect, Close in an info dialog). At most one per dialog or form. | Indigo fill, dark text |
| `secondary` | Every other action, Cancel. The default. | Slate fill, border |
| `danger` | Destructive or irreversible actions (Delete, Reject). Never the initial focus of a dialog that approves something risky. | Red tint |
| `ghost` | Low-emphasis actions inside content, link-like actions. | No fill, accent text |

Sizes: `md` (default) for dialogs and forms, `sm` for toolbars and inline
actions. A button's label says what it does ("Delete server", not "OK").
`Button` defaults to `type="button"`; only the primary action of a form is
`type="submit"`.

Allow / Confirm / Deny colours (Spec 0009) are reserved for policy decisions
and risk indicators. Do not use them for ordinary buttons.

## Dialogs

Use `DialogFrame`: backdrop (`ModalBackdrop`), focus trap and Escape
(`useDialogFocusTrap`), a title, a body and an action row.

- **Actions** sit in a row at the bottom, right-aligned. Order from left to
  right: secondary/cancel first, then the primary action last. A destructive
  action replaces the primary one (`danger`) and keeps the same position.
- **Only the body scrolls.** Title and action row stay inside the window and
  reachable, also at 800×560.
- **Initial focus** goes to the safest button (Cancel/Close), or to the first
  field of a form. When the dialog has no better target, the dialog itself is
  focused.
- Dialogs of the whole app use `layer="app"`; only dialogs that disappear with
  their session tab use `layer="sessionTab"`.
- Dialogs that need a decision with risk (host key, command confirmation) use
  `role="alertdialog"` and are never closed by a click on the backdrop.

## Spacing

Tailwind's 4 px scale, with these steps: `1` (4 px) inside compact controls,
`1.5`/`2` between an icon and its label or between related controls, `3` between
form fields and between paragraphs, `4` between a dialog body and its action
row, `6` as dialog padding. Prefer `gap-*` and `space-y-*` over single margins.

## Form fields

Use `Field`. Layout, top to bottom: label, control, help text, error text.

- The label is always visible and associated with the control (`htmlFor`/`id`);
  a placeholder is never the label.
- Help text is short, below the control, linked via `aria-describedby`.
- An error replaces nothing: it appears below the help text, is announced
  (`role="alert"`), sets `aria-invalid` on the control and says how to fix the
  problem.
- Controls use `FIELD_CONTROL_CLASS` so inputs look alike.

## Focus ring

Every interactive element shows a 2 px indigo ring (`indigo-400`) on
`:focus-visible`; never remove an outline without replacing it. The ring has at
least 3:1 contrast against the surface it sits on.

## Text size and contrast

- Minimum text size is `text-xs` (12 px), used for secondary information only.
  Body text is `text-sm` (14 px).
- Contrast targets (WCAG 2.1 AA): 4.5:1 for text (3:1 for large text), 3:1 for
  UI component boundaries and focus indicators. Disabled controls are exempt.
  Secondary text uses `slate-400` or lighter. The axe scans in the browser tests
  check this on every covered screen (Spec 0055).

## Truncation

Long single-line values (host names, paths, titles) are cut with an ellipsis
(`truncate`) and carry the full text in a `title` attribute, so hovering shows
it. Never truncate the only place where a value can be read in full: use
wrapping (`break-words`) or a scrollable area there.

## Keyboard

- Tab order follows the visual order; do not use positive `tabindex`.
- Escape closes a dialog (it counts as Cancel).
- Enter in a form field runs the primary action: dialogs with fields pass
  `onSubmit` to `DialogFrame` and make the primary button `type="submit"`.
- Arrow keys move within lists, trees and tab bars (one Tab stop for the whole
  widget); Home/End jump to the first/last item.
- Holding Enter from a previous dialog must not trigger a button of the next one
  (`useDialogFocusTrap` ignores repeated Enter).

## Proposing regroupings

The UI reviews of single areas fix small things directly (spacing, labels,
focus, contrast, truncation). They add no functions. Moving or regrouping
elements is only proposed: the review issue or PR describes

1. the current layout and the problem (with a before screenshot),
2. the proposed layout (sketch or description),
3. what a user must relearn,

and a maintainer decides.

## Screenshots and accessibility scans in browser tests

- `captureScreen(page, "name")` (`e2e/support/screenshots.ts`) writes the
  current screen at 1280×800 and 800×560 to `e2e/screenshots/<label>/`. Set
  `SCREENSHOT_LABEL=before` or `after` to keep two sets. Use it to attach
  before/after images to a review PR.
- Add a screen to the axe scan with one line in the `SCREENS` table in
  `e2e/tests/accessibility.spec.ts`.
