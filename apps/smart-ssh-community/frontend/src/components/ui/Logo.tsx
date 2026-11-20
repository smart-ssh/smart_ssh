export interface LogoProps {
  /** `"mark"`: the glyph only; `"lockup"`: glyph plus the "SMART SSH" wordmark. */
  variant?: "mark" | "lockup";
  /** Height of the glyph in px. */
  size?: number;
  /** Set when visible text next to the logo already names the app: the logo is then decorative. */
  decorative?: boolean;
  className?: string;
}

/** The "Trace" logo: an S drawn as one orthogonal trace between two square
 * terminals. The stroke follows the text colour (`currentColor`, set it to
 * `text-fg-strong`), the squares use the accent token, so it follows both
 * themes. Without `decorative` it is an image named "Smart SSH". */
export function Logo({ variant = "mark", size = 20, decorative = false, className = "" }: LogoProps) {
  const a11y = decorative ? { "aria-hidden": true as const } : { role: "img", "aria-label": "Smart SSH" };
  const glyph = (
    <>
      <path
        d="M48 14H16v18h32v18H16"
        fill="none"
        stroke="currentColor"
        strokeWidth={7}
        strokeLinecap="square"
        strokeLinejoin="miter"
      />
      <path d="M42 8h12v12H42z M10 44h12v12H10z" className="fill-accent" />
    </>
  );
  if (variant === "mark") {
    return (
      <svg
        xmlns="http://www.w3.org/2000/svg"
        viewBox="0 0 64 64"
        width={size}
        height={size}
        className={`shrink-0 text-fg-strong ${className}`}
        {...a11y}
      >
        {glyph}
      </svg>
    );
  }
  return (
    <span
      className={`inline-flex items-center gap-2 text-fg-strong ${className}`}
      {...(decorative ? {} : { role: "img", "aria-label": "Smart SSH" })}
    >
      <svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64" width={size} height={size} className="shrink-0" aria-hidden="true">
        {glyph}
      </svg>
      <span
        aria-hidden="true"
        className="font-['Barlow_Condensed'] font-bold uppercase tracking-[0.12em]"
        style={{ fontSize: size * 0.88, lineHeight: 1 }}
      >
        SMART<span className="text-accent">SSH</span>
      </span>
    </span>
  );
}
