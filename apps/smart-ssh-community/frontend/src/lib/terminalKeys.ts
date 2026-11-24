// Issue #678, Spec 0114: byte sequences of the touch key row and the sticky
// Ctrl/Alt modifiers. The sequences are the ones xterm.js sends for the
// physical keys (cursor keys switch to the SS3 form in application mode).

export type TerminalKeyId =
  | "esc"
  | "tab"
  | "up"
  | "down"
  | "left"
  | "right"
  | "home"
  | "end"
  | "pgup"
  | "pgdn"
  | "pipe"
  | "tilde"
  | "slash"
  | "dash";

export interface Modifiers {
  ctrl: boolean;
  alt: boolean;
}

const CSI_FINAL: Partial<Record<TerminalKeyId, string>> = {
  up: "A",
  down: "B",
  right: "C",
  left: "D",
  home: "H",
  end: "F",
};
const CSI_TILDE: Partial<Record<TerminalKeyId, string>> = { pgup: "5", pgdn: "6" };
const PLAIN: Partial<Record<TerminalKeyId, string>> = {
  esc: "\x1b",
  tab: "\t",
  pipe: "|",
  tilde: "~",
  slash: "/",
  dash: "-",
};

/** xterm's modifier parameter: 1 + shift(1) + alt(2) + ctrl(4). */
function modParam(m: Modifiers): number {
  return 1 + (m.alt ? 2 : 0) + (m.ctrl ? 4 : 0);
}

/** Sequence for a row key, including a pending Ctrl/Alt as xterm encodes it. */
export function keySequence(
  key: TerminalKeyId,
  mods: Modifiers = { ctrl: false, alt: false },
  applicationCursor = false,
): string {
  const final = CSI_FINAL[key];
  const tilde = CSI_TILDE[key];
  const p = modParam(mods);
  if (final) {
    if (p > 1) return `\x1b[1;${p}${final}`;
    return applicationCursor ? `\x1bO${final}` : `\x1b[${final}`;
  }
  if (tilde) return p > 1 ? `\x1b[${tilde};${p}~` : `\x1b[${tilde}~`;
  const plain = PLAIN[key] ?? "";
  return applyModifiers(plain, mods);
}

/** Ctrl code of one character, or the character itself when it has none. */
function ctrlChar(ch: string): string {
  const c = ch.charCodeAt(0);
  if (ch >= "a" && ch <= "z") return String.fromCharCode(c - 96);
  if (ch >= "A" && ch <= "Z") return String.fromCharCode(c - 64);
  if (ch === " " || ch === "@") return "\x00";
  if (ch >= "[" && ch <= "_") return String.fromCharCode(c - 64);
  if (ch === "?") return "\x7f";
  return ch;
}

/**
 * Applies pending sticky modifiers to soft-keyboard input. Only a single
 * character (one key press) is changed; longer input (paste, IME) passes
 * through unmodified.
 */
export function applyModifiers(data: string, mods: Modifiers): string {
  if ((!mods.ctrl && !mods.alt) || [...data].length !== 1) return data;
  const out = mods.ctrl ? ctrlChar(data) : data;
  return mods.alt ? `\x1b${out}` : out;
}
