import { useId } from "react";
import type { ReactNode } from "react";

/** Attributes the control inside a `Field` must receive. */
export interface FieldControlProps {
  id: string;
  "aria-describedby"?: string;
  "aria-invalid"?: true;
}

export interface FieldProps {
  label: ReactNode;
  /** Help text under the control, linked via `aria-describedby`. */
  help?: ReactNode;
  /** Error text under the control; announced (`role="alert"`), marks the control invalid. */
  error?: ReactNode;
  /** Render the control with the given props, e.g. `{(p) => <input {...p} />}`. */
  children: (control: FieldControlProps) => ReactNode;
  className?: string;
}

/** Form field layout: label above, control, help text, error text
 * (`docs/ui-guidelines.md`). Wires `htmlFor`/`id`/`aria-describedby`/
 * `aria-invalid` so callers cannot forget them. */
export function Field({ label, help, error, children, className }: FieldProps) {
  const id = useId();
  const helpId = `${id}-help`;
  const errorId = `${id}-error`;
  const describedBy = [help ? helpId : null, error ? errorId : null]
    .filter(Boolean)
    .join(" ");
  return (
    <div
      className={["flex flex-col gap-1", className].filter(Boolean).join(" ")}
    >
      <label htmlFor={id} className="text-sm font-medium text-slate-200">
        {label}
      </label>
      {children({
        id,
        "aria-describedby": describedBy || undefined,
        "aria-invalid": error ? true : undefined,
      })}
      {help ? (
        <p id={helpId} className="text-xs text-slate-400">
          {help}
        </p>
      ) : null}
      {error ? (
        <p id={errorId} role="alert" className="text-xs text-red-300">
          {error}
        </p>
      ) : null}
    </div>
  );
}

/** Class string for text inputs/selects/textareas, so fields look alike. */
export const FIELD_CONTROL_CLASS =
  "rounded border border-slate-600 bg-slate-950 px-2.5 py-1.5 text-sm text-slate-100 " +
  "placeholder:text-slate-500 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-indigo-400 " +
  "aria-[invalid=true]:border-red-500 disabled:opacity-50";
