export interface CornerTicksProps {
  tone?: "accent" | "danger";
  /** `"top"`: the two top corners (dialogs); `"all"`: all four (unlock card). */
  corners?: "top" | "all";
  /** Length of the marks in px. */
  size?: number;
}

const TICK = {
  tl: "border-l-2 border-t-2",
  tr: "border-r-2 border-t-2",
  bl: "border-b-2 border-l-2",
  br: "border-b-2 border-r-2",
} as const;
const OFFSET = {
  tl: "-left-2 -top-2",
  tr: "-right-2 -top-2",
  bl: "-bottom-2 -left-2",
  br: "-bottom-2 -right-2",
} as const;

/** Decorative L-shaped corner marks (2 px) just outside the corners of the
 * positioned parent. Purely visual, hidden from assistive technology. */
export function CornerTicks({ tone = "accent", corners = "top", size = 12 }: CornerTicksProps) {
  const keys = (corners === "all" ? ["tl", "tr", "bl", "br"] : ["tl", "tr"]) as (keyof typeof TICK)[];
  const colour = tone === "danger" ? "border-danger" : "border-accent";
  return (
    <>
      {keys.map((k) => (
        <span
          key={k}
          aria-hidden="true"
          data-corner-tick={k}
          style={{ width: size, height: size }}
          className={`pointer-events-none absolute ${OFFSET[k]} ${TICK[k]} ${colour}`}
        />
      ))}
    </>
  );
}
