import { useId, useRef } from "react";
import type { FormEvent, MouseEvent as ReactMouseEvent, ReactNode, RefObject } from "react";

import { useDialogFocusTrap } from "../../hooks/useDialogFocusTrap";
import { ModalBackdrop, type ModalBackdropLayer } from "../ModalBackdrop";

export interface DialogFrameProps {
  /** See `ModalBackdrop`: `"app"` portals to `document.body`. */
  layer: ModalBackdropLayer;
  /** Heading of the standard layout. Not used with `bare`. */
  title?: ReactNode;
  /** Escape closes the dialog (and so does the Cancel/Close button the caller puts into `actions`). */
  onClose: () => void;
  /** Action row, rendered right-aligned; secondary/cancel first, primary last. */
  actions?: ReactNode;
  /** When set, body and actions sit in a `<form>`, so Enter in a field runs
   * the primary (`type="submit"`) action. */
  onSubmit?: (event: FormEvent<HTMLFormElement>) => void;
  /** Element focused on open. Default: the dialog itself. */
  initialFocusRef?: RefObject<HTMLElement | null>;
  role?: "dialog" | "alertdialog";
  size?: "sm" | "md" | "lg" | "xl";
  /** Keep the top of the window free at title-bar height, for screens
   * where a drag strip lies over the backdrop (start-up dialogs, Spec 0014 §5). */
  reserveTitlebar?: boolean;
  /** `"centered"` (default): framed dialog in the middle (full window when narrow).
   * `"sheet"`: panel positioned by `panelClassName` over a dimmed backdrop (navigation drawer). */
  layout?: "centered" | "sheet";
  /** The caller renders the whole panel content (own heading, own padding,
   * no action row) and styles the panel with `panelClassName`. Needs `labelledBy` or `ariaLabel`. */
  bare?: boolean;
  panelClassName?: string;
  labelledBy?: string;
  describedBy?: string;
  ariaLabel?: string;
  /** Re-sets the initial focus when it changes (a new event shown in the same dialog). */
  resetKey?: unknown;
  /** Dialog that opens nested dialogs from inside, see `useDialogFocusTrap`. */
  yieldToNested?: boolean;
  /** Element that gets the focus back on close; default the one focused at open. */
  returnFocusTo?: HTMLElement | null;
  /** A click on the backdrop outside the panel calls this (sheet layout). */
  onBackdropClick?: () => void;
  children?: ReactNode;
}

const SIZES = { sm: "max-w-sm", md: "max-w-lg", lg: "max-w-2xl", xl: "max-w-4xl" } as const;

/** Shared modal dialog: backdrop (`ModalBackdrop`), focus trap and Escape
 * (`useDialogFocusTrap`), title, scrolling body, action row. Only the body
 * scrolls; title and actions stay in the window (`docs/ui-guidelines.md`). */
export function DialogFrame({
  layer,
  title,
  onClose,
  actions,
  onSubmit,
  initialFocusRef,
  role = "dialog",
  size = "md",
  reserveTitlebar = false,
  layout = "centered",
  bare = false,
  panelClassName,
  labelledBy,
  describedBy,
  ariaLabel,
  resetKey = null,
  yieldToNested = false,
  returnFocusTo,
  onBackdropClick,
  children,
}: DialogFrameProps) {
  const titleId = useId();
  const containerRef = useRef<HTMLDivElement>(null);
  const { onKeyDown } = useDialogFocusTrap({
    containerRef,
    initialFocusRef: initialFocusRef ?? containerRef,
    onEscape: onClose,
    resetKey,
    yieldToNested,
    returnFocusTo,
  });

  // Safari/WebKit on macOS does not focus a button on click, so Escape would
  // never reach the dialog: after a click inside, pull the focus in.
  const handleClick = (event: ReactMouseEvent<HTMLDivElement>) => {
    const container = containerRef.current;
    if (!container || !(event.target instanceof Element) || !container.contains(event.target)) return;
    if (container.contains(document.activeElement)) return;
    const button = event.target.closest("button");
    (button && container.contains(button) ? button : container).focus();
  };
  const backdropClass =
    layout === "sheet"
      ? `fixed inset-0 z-50 bg-overlay/60`
      : `fixed inset-0 z-50 flex items-center justify-center bg-overlay/70 narrow:items-stretch ${
          reserveTitlebar ? "px-4 pb-4 pt-13 narrow:px-0 narrow:pb-0" : "p-4 narrow:p-0 narrow:pt-9"
        }`;
  const panelClass = bare
    ? `${panelClassName ?? ""} focus:outline-none`
    : `flex max-h-[85vh] w-full ${SIZES[size]} flex-col rounded border border-border-strong bg-surface p-6 shadow-xl narrow:max-h-none narrow:max-w-none narrow:rounded-none narrow:border-0 narrow:p-4 focus:outline-none`;

  const content = (
    <>
      <div className="flex min-h-0 flex-1 flex-col overflow-y-auto text-sm text-fg-muted">
        {children}
      </div>
      {actions ? (
        <div className="mt-4 flex flex-wrap justify-end gap-2 narrow:mt-3 narrow:shrink-0">{actions}</div>
      ) : null}
    </>
  );

  return (
    <ModalBackdrop layer={layer} className={backdropClass}>
      {onBackdropClick ? (
        // eslint-disable-next-line jsx-a11y/no-static-element-interactions, jsx-a11y/click-events-have-key-events -- backdrop dismiss; keyboard users use Escape
        <div data-testid="dialog-backdrop-dismiss" className="absolute inset-0" onClick={onBackdropClick} />
      ) : null}
      {/* `role` is a dynamic value (dialog | alertdialog), which the linter cannot see; the key handler only wraps Tab (focus trap). */}
      {/* eslint-disable-next-line jsx-a11y/no-static-element-interactions */}
      <div
        ref={containerRef}
        role={role}
        aria-modal="true"
        aria-labelledby={bare ? labelledBy : titleId}
        aria-describedby={describedBy}
        aria-label={ariaLabel}
        tabIndex={-1}
        onKeyDown={onKeyDown}
        onClick={handleClick}
        className={panelClass}
      >
        {bare ? (
          children
        ) : (
          <>
        <h2
          id={titleId}
          className="font-heading mb-3 shrink-0 text-lg break-words font-semibold text-fg"
        >
          {title}
        </h2>
        {onSubmit ? (
          <form onSubmit={onSubmit} className="flex min-h-0 flex-1 flex-col">
            {content}
          </form>
        ) : (
          content
        )}
          </>
        )}
      </div>
    </ModalBackdrop>
  );
}
