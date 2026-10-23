import { useId, useState, type ReactNode } from "react";

/**
 * Small tooltip for a focusable inline element. Shows `content` on mouse
 * hover and keyboard focus, hides on blur, mouse leave and Escape. The text
 * is always exposed to assistive technology via `aria-describedby`. The
 * bubble uses fixed positioning (above the trigger) so it is not clipped by
 * scrolling containers and does not affect the trigger's layout.
 */
export function Tooltip({
  content,
  className = "",
  children,
}: {
  content: string;
  className?: string;
  children: ReactNode;
}) {
  const id = useId();
  const [pos, setPos] = useState<{ left: number; bottom: number } | null>(null);

  const show = (el: HTMLElement) => {
    const r = el.getBoundingClientRect();
    const maxLeft = Math.max(8, window.innerWidth - 288);
    setPos({
      left: Math.min(Math.max(8, r.left), maxLeft),
      bottom: window.innerHeight - r.top + 6,
    });
  };

  return (
    <span
      // biome/oxlint: a focusable, described inline element; not interactive.
      tabIndex={0}
      aria-describedby={id}
      className={className}
      onMouseEnter={(e) => show(e.currentTarget)}
      onMouseLeave={() => setPos(null)}
      onFocus={(e) => show(e.currentTarget)}
      onBlur={() => setPos(null)}
      onKeyDown={(e) => {
        if (e.key === "Escape" && pos) {
          e.stopPropagation();
          setPos(null);
        }
      }}
    >
      {children}
      <span
        id={id}
        role="tooltip"
        hidden={!pos}
        style={pos ? { position: "fixed", left: pos.left, bottom: pos.bottom } : undefined}
        className="z-50 w-max max-w-72 rounded border border-slate-600 bg-slate-900 px-2 py-1 text-left font-sans text-xs font-normal tracking-normal text-slate-100 normal-case shadow-lg"
      >
        {content}
      </span>
    </span>
  );
}
