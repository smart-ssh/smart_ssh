// @vitest-environment node
//
// Issue #217: every text-like <input>/<textarea> in src/components must
// spread TECHNICAL_INPUT_PROPS (system text correction off) unless it is a
// free-text field on the allowlist below.
import fs from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";

const COMPONENTS_DIR = path.join(__dirname, "components");
const NON_TEXT_TYPES = new Set(["radio", "checkbox", "number", "button", "file", "range"]);

/** Free-text fields (prose for humans or the AI): system correction stays on. */
const FREE_TEXT_ALLOWLIST: { file: string; marker: string }[] = [
  { file: "ChatPanel.tsx", marker: 'placeholder={t("chat.inputPlaceholder")}' },
  { file: "NotesPanel.tsx", marker: "textarea" },
  { file: "ServerForm.tsx", marker: "ref={localNotesRef}" },
];

/** Opening tags of input/textarea elements found in JSX source (not comments). */
export function findFieldTags(source: string): string[] {
  const tags: string[] = [];
  const re = /<(input|textarea)(?=[\s/>])/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(source)) !== null) {
    const lineStart = source.lastIndexOf("\n", m.index) + 1;
    const prefix = source.slice(lineStart, m.index);
    if (/^\s*(\/\/|\*|\/\*|\{\/\*)/.test(prefix) || prefix.includes("`") || prefix.includes("//")) continue;
    let depth = 0;
    let i = m.index + m[0].length;
    for (; i < source.length; i++) {
      const c = source[i];
      if (c === "{") depth++;
      else if (c === "}") depth--;
      else if (c === ">" && depth === 0 && source[i - 1] !== "=") break;
    }
    tags.push(source.slice(m.index, i + 1));
  }
  return tags;
}

export function violations(file: string, source: string): string[] {
  return findFieldTags(source)
    .filter((tag) => {
      const type = /type="([^"]+)"/.exec(tag)?.[1];
      if (type && NON_TEXT_TYPES.has(type)) return false;
      if (tag.includes("{...TECHNICAL_INPUT_PROPS}")) return false;
      const free = FREE_TEXT_ALLOWLIST.some((a) => a.file === file && tag.includes(a.marker));
      return !free;
    })
    .map((tag) => `${file}: ${tag.split("\n").slice(0, 3).join(" ").trim()}`);
}

describe("text correction guard (issue #217)", () => {
  it("every technical text field spreads TECHNICAL_INPUT_PROPS", () => {
    const files = fs
      .readdirSync(COMPONENTS_DIR)
      .filter((f) => f.endsWith(".tsx") && !f.includes(".test."));
    const found = files.flatMap((f) =>
      violations(f, fs.readFileSync(path.join(COMPONENTS_DIR, f), "utf8")),
    );
    expect(found).toEqual([]);
  });

  it("flags a text input without the shared props", () => {
    expect(violations("X.tsx", '<input type="text" value={a} />')).toHaveLength(1);
    expect(violations("X.tsx", "<textarea value={a} />")).toHaveLength(1);
    expect(violations("X.tsx", "<input\n {...TECHNICAL_INPUT_PROPS}\n value={a} />")).toHaveLength(0);
    expect(violations("X.tsx", '<input type="checkbox" checked />')).toHaveLength(0);
  });

  it("keeps free-text fields off the shared props", () => {
    for (const { file, marker } of FREE_TEXT_ALLOWLIST) {
      const src = fs.readFileSync(path.join(COMPONENTS_DIR, file), "utf8");
      const tag = findFieldTags(src).find((t) => t.includes(marker));
      expect(tag, `${file} ${marker}`).toBeDefined();
      expect(tag).not.toContain("TECHNICAL_INPUT_PROPS");
    }
  });
});
