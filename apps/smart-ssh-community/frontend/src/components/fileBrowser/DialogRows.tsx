import type { ReactNode } from "react";

export interface DialogRow {
  label: string;
  value: ReactNode;
  /** Optional tone for the value (e.g. failed count). */
  valueClassName?: string;
}

/** Label/value rows for counts and sizes in file dialogs (same look as the properties dialog). */
export function DialogRows({ rows, className = "mb-3" }: { rows: DialogRow[]; className?: string }) {
  return (
    <dl className={`${className} divide-y divide-surface-raised/70 text-sm`} data-testid="dialog-rows">
      {rows.map((r) => (
        <div key={r.label} className="flex justify-between gap-4 py-1">
          <dt className="shrink-0 text-fg-subtle">{r.label}</dt>
          <dd className={`min-w-0 text-right font-mono break-all ${r.valueClassName ?? "text-fg"}`}>{r.value}</dd>
        </div>
      ))}
    </dl>
  );
}
