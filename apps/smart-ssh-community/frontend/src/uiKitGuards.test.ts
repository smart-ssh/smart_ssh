// @vitest-environment node
//
// Issue #914: ratchets for the UI kit. Raw `<button` elements outside
// `components/ui/` may only decrease (use `Button` / `MenuItem`), and hand-built
// modals (`<dialog`, `fixed inset-0`) are limited to `components/ui/`,
// `components/ModalBackdrop.tsx` and a shrinking allowlist.
import fs from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";
import rawButtonBaseline from "./uiKitRawButtons.json";
import dialogAllowlist from "./uiKitDialogAllowlist.json";

type Baseline = Record<string, { count: number; reason: string }>;
type Allowlist = Record<string, string>;

const SRC = __dirname;

function productionTsx(): string[] {
  const out: string[] = [];
  const walk = (dir: string) => {
    for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
      const full = path.join(dir, e.name);
      if (e.isDirectory()) walk(full);
      else if (e.name.endsWith(".tsx") && !/\.(test|spec)\.tsx$/.test(e.name)) {
        out.push(path.relative(SRC, full).split(path.sep).join("/"));
      }
    }
  };
  walk(SRC);
  return out.sort();
}

const inKit = (file: string) => file.startsWith("components/ui/");

export function countRawButtons(text: string): number {
  return (text.match(/<button\b/g) ?? []).length;
}

/** Violations of the raw-button ratchet for `files` (file → source text). */
export function rawButtonViolations(files: Record<string, string>, baseline: Baseline): string[] {
  const bad: string[] = [];
  for (const [file, text] of Object.entries(files)) {
    if (inKit(file)) continue;
    const n = countRawButtons(text);
    const entry = baseline[file];
    if (n === 0) continue;
    if (!entry) {
      bad.push(`${file}: ${n} raw <button> and no entry in uiKitRawButtons.json (use Button or MenuItem from components/ui, or add an entry with a reason)`);
    } else if (n > entry.count) {
      bad.push(`${file}: ${n} raw <button> > allowed ${entry.count} (use Button or MenuItem from components/ui)`);
    }
  }
  return bad;
}

const DIALOG_PATTERN = /<dialog\b|fixed inset-0/;

export function dialogViolations(files: Record<string, string>, allowlist: Allowlist): string[] {
  const bad: string[] = [];
  for (const [file, text] of Object.entries(files)) {
    if (inKit(file) || file === "components/ModalBackdrop.tsx") continue;
    if (DIALOG_PATTERN.test(text) && !(file in allowlist)) {
      bad.push(`${file}: contains <dialog or "fixed inset-0" (use DialogFrame from components/ui, or add it to uiKitDialogAllowlist.json with a reason)`);
    }
  }
  return bad;
}

function readAll(): Record<string, string> {
  return Object.fromEntries(productionTsx().map((f) => [f, fs.readFileSync(path.join(SRC, f), "utf8")]));
}

describe("raw <button> ratchet", () => {
  it("passes for the repository", () => {
    expect(rawButtonViolations(readAll(), rawButtonBaseline as Baseline)).toEqual([]);
  });

  it("only ever shrinks: no entry above the real count, none for a clean or missing file", () => {
    const files = readAll();
    const stale = Object.entries(rawButtonBaseline as Baseline)
      .filter(([f, e]) => !(f in files) || countRawButtons(files[f]) < e.count)
      .map(([f]) => f);
    expect(stale, "lower or remove these entries in src/uiKitRawButtons.json").toEqual([]);
  });

  it("fails when a raw button is added to a fixture", () => {
    const baseline: Baseline = { "components/A.tsx": { count: 1, reason: "x" } };
    expect(rawButtonViolations({ "components/A.tsx": "<button>a</button>" }, baseline)).toEqual([]);
    expect(
      rawButtonViolations({ "components/A.tsx": "<button>a</button><button>b</button>" }, baseline),
    ).toHaveLength(1);
    const msg = rawButtonViolations({ "components/New.tsx": "<button>a</button>" }, baseline)[0];
    expect(msg).toContain("MenuItem");
    expect(msg).toContain("Button");
    expect(rawButtonViolations({ "components/ui/Button.tsx": "<button>a</button>" }, baseline)).toEqual([]);
  });
});

describe("dialog guard", () => {
  it("passes for the repository", () => {
    expect(dialogViolations(readAll(), dialogAllowlist as Allowlist)).toEqual([]);
  });

  it("allowlist entries are still needed", () => {
    const files = readAll();
    const stale = Object.keys(dialogAllowlist).filter((f) => !(f in files) || !DIALOG_PATTERN.test(files[f]));
    expect(stale, "remove these entries from src/uiKitDialogAllowlist.json").toEqual([]);
  });

  it("fails for a fixture with <dialog or fixed inset-0 outside the allowed places", () => {
    expect(dialogViolations({ "components/X.tsx": "<dialog open />" }, {})).toHaveLength(1);
    expect(dialogViolations({ "components/X.tsx": '<div className="fixed inset-0">' }, {})).toHaveLength(1);
    expect(dialogViolations({ "components/ui/D.tsx": "<dialog />" }, {})).toEqual([]);
    expect(dialogViolations({ "components/ModalBackdrop.tsx": "fixed inset-0" }, {})).toEqual([]);
    expect(dialogViolations({ "components/X.tsx": "<dialog />" }, { "components/X.tsx": "reason" })).toEqual([]);
  });
});
