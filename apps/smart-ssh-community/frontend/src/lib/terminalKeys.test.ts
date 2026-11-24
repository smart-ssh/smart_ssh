import { describe, expect, it } from "vitest";
import { applyModifiers, keySequence, type TerminalKeyId } from "./terminalKeys";

const none = { ctrl: false, alt: false };

describe("terminal key row sequences", () => {
  const cases: [TerminalKeyId, string][] = [
    ["esc", "\x1b"],
    ["tab", "\t"],
    ["up", "\x1b[A"],
    ["down", "\x1b[B"],
    ["right", "\x1b[C"],
    ["left", "\x1b[D"],
    ["home", "\x1b[H"],
    ["end", "\x1b[F"],
    ["pgup", "\x1b[5~"],
    ["pgdn", "\x1b[6~"],
    ["pipe", "|"],
    ["tilde", "~"],
    ["slash", "/"],
    ["dash", "-"],
  ];
  it.each(cases)("%s sends the xterm sequence", (key, seq) => {
    expect(keySequence(key, none)).toBe(seq);
  });

  it("uses the SS3 form for cursor keys in application mode", () => {
    expect(keySequence("up", none, true)).toBe("\x1bOA");
    expect(keySequence("end", none, true)).toBe("\x1bOF");
    expect(keySequence("pgup", none, true)).toBe("\x1b[5~");
  });

  it("encodes modifiers on row keys like xterm", () => {
    expect(keySequence("up", { ctrl: true, alt: false })).toBe("\x1b[1;5A");
    expect(keySequence("left", { ctrl: false, alt: true })).toBe("\x1b[1;3D");
    expect(keySequence("pgdn", { ctrl: true, alt: true })).toBe("\x1b[6;7~");
    expect(keySequence("tab", { ctrl: false, alt: true })).toBe("\x1b\t");
  });
});

describe("sticky modifiers", () => {
  it("Ctrl+c is ETX", () => expect(applyModifiers("c", { ctrl: true, alt: false })).toBe("\x03"));
  it("Ctrl+C is ETX", () => expect(applyModifiers("C", { ctrl: true, alt: false })).toBe("\x03"));
  it("Ctrl+[ is ESC, Ctrl+space is NUL", () => {
    expect(applyModifiers("[", { ctrl: true, alt: false })).toBe("\x1b");
    expect(applyModifiers(" ", { ctrl: true, alt: false })).toBe("\x00");
  });
  it("Alt+b is ESC b", () => expect(applyModifiers("b", { ctrl: false, alt: true })).toBe("\x1bb"));
  it("Ctrl+Alt+d is ESC EOT", () => expect(applyModifiers("d", { ctrl: true, alt: true })).toBe("\x1b\x04"));
  it("leaves unmodified and multi-character input alone", () => {
    expect(applyModifiers("c", none)).toBe("c");
    expect(applyModifiers("ls", { ctrl: true, alt: true })).toBe("ls");
  });
});
