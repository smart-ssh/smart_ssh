import type { CSSProperties } from "react";

/**
 * Flat, angular SVG icon set (docs/design/angular/icons): 24 × 24 grid, square
 * caps, mitred joins, `currentColor`. Paths are inlined (no fetch, no sprite).
 *
 * Icons drawn on the same grid because the design set lacks them:
 * `home`, `arrow-down`, `arrow-left`, `chev-u`, `globe`, `scissors`, `lock-open`, `square`, `square-check`.
 */
export const ICON_PATHS = {
  "activity": "M2 12h5l3-8 4 16 3-8h5",
  "arrow-down": "M12 4v16 M5 13l7 7 7-7",
  "arrow-left": "M20 12H4 M11 5l-7 7 7 7",
  "bolt": "M13 2L4 14h7l-2 8 9-12h-7z",
  "chat": "M3 4h18v13H10l-5 4v-4H3z",
  "check": "M4 12l5 5L20 6",
  "chev-d": "M6 9l6 6 6-6",
  "chev-r": "M9 6l6 6-6 6",
  "chev-u": "M6 15l6-6 6 6",
  "clock": "M3 3h18v18H3z M12 7v5h4",
  "copy": "M8 8h12v12H8z M4 16V4h12",
  "download": "M12 4v12 M6 10l6 6 6-6 M4 20h16",
  "edit": "M4 20h4L20 8l-4-4L4 16z M14 6l4 4",
  "export": "M12 14V3 M7 8l5-5 5 5 M3 15v6h18v-6",
  "file": "M5 2h9l5 5v15H5z M14 2v5h5",
  "file-check": "M5 2h9l5 5v15H5z M14 2v5h5 M8 15l2.5 2.5L16 12",
  "file-code": "M5 2h9l5 5v15H5z M14 2v5h5 M10 12l-2 2.5 2 2.5 M14 12l2 2.5-2 2.5",
  "file-text": "M5 2h9l5 5v15H5z M14 2v5h5 M8 12h8 M8 16h8",
  "file-x": "M5 2h9l5 5v15H5z M14 2v5h5 M9 12l6 6 M15 12l-6 6",
  "fingerprint": "M4 9V4h5 M20 9V4h-5 M4 15v5h5 M20 15v5h-5 M8 9h8v6H8z",
  "folder": "M3 5h6l2 2h10v13H3z M3 10h18",
  "folder-link": "M3 5h6l2 2h10v13H3z M8 17v-3h7 M13 12l2 2-2 2",
  "folder-open": "M3 20V5h6l2 2h8v3 M3 20l3-10h16l-3 10z",
  "globe": "M3 3h18v18H3z M3 12h18 M12 3l-5 9 5 9 M12 3l5 9-5 9",
  "grip": "M9 5h1 M14 5h1 M9 12h1 M14 12h1 M9 19h1 M14 19h1",
  "home": "M3 12l9-9 9 9 M5 10v11h14V10 M10 21v-6h4v6",
  "import": "M12 3v11 M7 9l5 5 5-5 M3 15v6h18v-6",
  "info": "M3 3h18v18H3z M12 11v6 M12 7v.5",
  "key": "M2 9h8v8H2z M10 13h11 M17 13v4 M21 13v3",
  "link": "M10 14l4-4 M9 7l3-3h5l3 3v5l-3 3 M15 17l-3 3H7l-3-3v-5l3-3",
  "local": "M2 4h20v13H2z M8 21h8 M12 17v4 M6 8l3 2.5L6 13 M11 13h4",
  "lock": "M5 11h14v10H5z M8 11V6l2-3h4l2 3v5 M12 15v2",
  "lock-open": "M5 11h14v10H5z M8 11V6l2-3h4l2 3 M12 15v2",
  "menu": "M4 6h16 M4 12h16 M4 18h16",
  "mic": "M9 3h6v10H9z M5 10v3l3 4h8l3-4v-3 M12 17v4 M8 21h8",
  "more": "M5 12h1 M11.5 12h1 M18 12h1",
  "move": "M3 5h6l2 2h10v13H3z M8 14h7 M12 11l3 3-3 3",
  "notes": "M5 3h14v18H5z M8 8h8 M8 12h8 M8 16h5",
  "parent": "M12 20V4 M5 11l7-7 7 7",
  "play": "M7 4l12 8-12 8z",
  "plug": "M9 2v5 M15 2v5 M6 7h12v5l-3 4H9l-3-4z M12 16v6",
  "plus": "M12 5v14 M5 12h14",
  "refresh": "M3 9V5h15 M15 2l3 3-3 3 M21 15v4H6 M9 16l-3 3 3 3",
  "root": "M3 3h18v18H3z M3 9h18",
  "scissors": "M6 3l12 14 M18 3L6 17 M4 17h4v4H4z M16 17h4v4h-4z",
  "search": "M4 4h11v11H4z M15 15l5 5",
  "send": "M3 11L21 3l-7 18-3-7z M11 14L21 3",
  "server": "M3 4h18v7H3z M3 13h18v7H3z M7 7.5h2 M7 16.5h2 M13 7.5h4 M13 16.5h4",
  "shield": "M12 2l8 3v8l-8 9-8-9V5z M8.5 12l2.5 2.5 4.5-4.5",
  "sliders": "M4 6h9 M17 6h3 M15 3v6 M4 12h3 M11 12h9 M9 9v6 M4 18h11 M19 18h1 M17 15v6",
  "spark": "M12 3l2.5 6.5L21 12l-6.5 2.5L12 21l-2.5-6.5L3 12l6.5-2.5z",
  "split": "M3 4h18v16H3z M12 4v16",
  "square": "M4 4h16v16H4z",
  "square-check": "M4 4h16v16H4z M8 12l3 3 5-6",
  "stop": "M6 6h12v12H6z",
  "symlink": "M5 2h9l5 5v15H5z M14 2v5h5 M8 18v-4h6 M12 12l2 2-2 2",
  "tag": "M3 3h9l9 9-9 9-9-9z M7 7h2v2H7z",
  "terminal": "M3 4h18v16H3z M7 9l3 3-3 3 M12 15h5",
  "trash": "M4 6h16 M9 6V3h6v3 M6 6l1 15h10l1-15 M10 10v7 M14 10v7",
  "upload": "M12 16V4 M6 10l6-6 6 6 M4 20h16",
  "user": "M8 3h8v8H8z M4 21v-4l3-3h10l3 3v4",
  "warn": "M12 3l10 18H2z M12 10v5 M12 18v.5",
  "x": "M6 6l12 12 M18 6L6 18",
} as const;

export type IconName = keyof typeof ICON_PATHS;
export const ICON_NAMES = Object.keys(ICON_PATHS) as IconName[];

export interface IconProps {
  name: IconName;
  /** Rendered size in px; default 16. */
  size?: 16 | 20 | 24;
  className?: string;
  style?: CSSProperties;
  /** Accessible name. Without it the icon is decorative (`aria-hidden`). */
  label?: string;
}

export function Icon({ name, size = 16, className, style, label }: IconProps) {
  const a11y = label
    ? ({ role: "img", "aria-label": label } as const)
    : ({ "aria-hidden": "true" } as const);
  return (
    <svg
      viewBox="0 0 24 24"
      width={size}
      height={size}
      fill="none"
      stroke="currentColor"
      strokeWidth={size === 16 ? 1.5 : 2}
      strokeLinecap="square"
      strokeLinejoin="miter"
      className={["shrink-0", className].filter(Boolean).join(" ")}
      style={style}
      data-icon={name}
      focusable="false"
      {...a11y}
    >
      <path d={ICON_PATHS[name]} />
    </svg>
  );
}
