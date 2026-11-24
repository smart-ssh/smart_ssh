// Detection logic of the colour guard (issue #805). Kept apart from the test so
// it can be unit-tested with fixtures.

const COLOUR_NAMES =
  "slate|gray|grey|zinc|neutral|stone|red|orange|amber|yellow|lime|green|emerald|teal|cyan|sky|blue|indigo|violet|purple|fuchsia|pink|rose|steel";
const UTILITIES =
  "bg|text|border|border-[xytblrse]|ring|ring-offset|outline|fill|stroke|divide|placeholder|from|via|to|accent|caret|decoration|shadow|inset-ring";

const PALETTE_CLASS = new RegExp(
  `(?<![\\w-])(?:${UTILITIES})-(?:${COLOUR_NAMES})-\\d{2,3}(?![\\w-])`,
  "g",
);
const BLACK_WHITE_CLASS = new RegExp(
  `(?<![\\w-])(?:${UTILITIES})-(?:white|black)(?![\\w-])`,
  "g",
);
const HEX = /#(?:[0-9a-fA-F]{8}|[0-9a-fA-F]{6}|[0-9a-fA-F]{3,4})(?![\w-])/g;
const FUNCTIONAL = /\b(?:rgba?|hsla?|oklch|oklab|lab|lch|color-mix)\(/g;

/** Removes `//` and `/* *\/` comments (issue numbers like `#696` live there). */
export function stripComments(src: string): string {
  return src
    .replace(/\/\*[\s\S]*?\*\//g, " ")
    .replace(/(^|[\s;,{}(])\/\/[^\n]*/g, "$1");
}

/** Returns every hard-coded colour found in a TS/TSX source. */
export function findHardCodedColours(src: string): string[] {
  const code = stripComments(src);
  return [PALETTE_CLASS, BLACK_WHITE_CLASS, HEX, FUNCTIONAL].flatMap(
    (re) => code.match(re) ?? [],
  );
}
