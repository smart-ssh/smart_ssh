import { useId, useRef } from "react";
import type { FormEvent, ReactNode, RefObject } from "react";

import { useDialogFocusTrap } from "../../useDialogFocusTrap";
import { ModalBackdrop, type ModalBackdropLayer } from "../ModalBackdrop";

export interface DialogFrameProps {
  /** See `ModalBackdrop`: `"app"` portals to `document.body`. */
  layer: ModalBackdropLayer;
  title: ReactNode;
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
  children,
}: DialogFrameProps) {
  const titleId = useId();
  const containerRef = useRef<HTMLDivElement>(null);
  const { onKeyDown } = useDialogFocusTrap({
    containerRef,
    initialFocusRef: initialFocusRef ?? containerRef,
    onEscape: onClose,
    resetKey: null,
  });

  const content = (
    <>
      <div className="flex min-h-0 flex-1 flex-col overflow-y-auto text-sm text-slate-300">
        {children}
      </div>
      {actions ? (
        <div className="mt-4 flex justify-end gap-2">{actions}</div>
      ) : null}
    </>
  );

  return (
    <ModalBackdrop
      layer={layer}
      className={`fixed inset-0 z-50 flex items-center justify-center bg-black/70 ${
        reserveTitlebar ? "px-4 pb-4 pt-13" : "p-4"
      }`}
    >
      {/* `role` is a dynamic value (dialog | alertdialog), which the linter cannot see; the key handler only wraps Tab (focus trap). */}
      {/* eslint-disable-next-line jsx-a11y/no-static-element-interactions */}
      <div
        ref={containerRef}
        role={role}
        aria-modal="true"
        aria-labelledby={titleId}
        tabIndex={-1}
        onKeyDown={onKeyDown}
        className={`flex max-h-[85vh] w-full ${SIZES[size]} flex-col rounded border border-slate-600 bg-slate-900 p-6 shadow-xl focus:outline-none`}
      >
        <h2
          id={titleId}
          className="font-heading mb-3 text-lg font-semibold text-slate-100"
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
      </div>
    </ModalBackdrop>
  );
}
