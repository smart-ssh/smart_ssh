// @vitest-environment node
//
// Issue #101: `scripts/check-tauri-commands.mjs` gleicht Definition
// (`#[tauri::command]`), Registrierung (`generate_handler!`) und
// Frontend-Aufrufe (`invoke("…")`) ab. Die Fälle mit dem echten Repo laufen
// auf einer Kopie in einem temporären Verzeichnis, damit eine Änderung nie
// im Arbeitsbaum landet.
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import {
  checkRepo,
  isFrontendTestFile,
  parseAllowedWhileLocked,
  parseDefinedCommands,
  parseInvokedCommands,
  parseRegisteredCommands,
  stripJsComments,
  stripRustComments,
} from "../../../../scripts/check-tauri-commands.mjs";

const REPO_ROOT = path.resolve(__dirname, "../../../..");
const LIB_RS = "crates/app-shell/src/lib.rs";

const tmpDirs: string[] = [];
afterEach(() => {
  for (const d of tmpDirs.splice(0)) fs.rmSync(d, { recursive: true, force: true });
});

/** Kopiert genau die Verzeichnisse, die die Prüfung liest. */
function copyRepo(): string {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "tauri-cmd-check-"));
  tmpDirs.push(dir);
  for (const sub of ["crates/app-shell/src", "apps/smart-ssh-community/frontend/src"]) {
    fs.cpSync(path.join(REPO_ROOT, sub), path.join(dir, sub), { recursive: true });
  }
  return dir;
}

function edit(root: string, file: string, change: (text: string) => string) {
  const full = path.join(root, file);
  const before = fs.readFileSync(full, "utf8");
  const after = change(before);
  expect(after).not.toBe(before);
  fs.writeFileSync(full, after);
}

describe("check-tauri-commands on the real repository", () => {
  it("passes on the current tree", () => {
    const { problems, counts } = checkRepo(REPO_ROOT);
    expect(problems).toEqual([]);
    expect(counts.defined).toBeGreaterThan(100);
    expect(counts.registered).toBe(counts.defined);
  });

  it("fails and names a command whose generate_handler! entry is commented out", () => {
    const root = copyRepo();
    edit(root, LIB_RS, (t) => t.replace(/^([ \t]*)commands::get_app_info,(?=\r?$)/m, "$1// commands::get_app_info,"));
    // `api.ts` ruft das Kommando auf — beide Seiten der Lücke erscheinen.
    expect(checkRepo(root).problems).toEqual([
      expect.objectContaining({ name: "get_app_info", kind: expect.stringMatching(/nicht in generate_handler! registriert/) }),
      expect.objectContaining({ name: "get_app_info", kind: expect.stringMatching(/aufgerufen, aber nicht registriert/) }),
    ]);
  });

  it("fails and names an invoked command that is not registered", () => {
    const root = copyRepo();
    edit(root, "apps/smart-ssh-community/frontend/src/fileDialog.ts", (t) =>
      t + '\nexport const probe = () => invoke("does_not_exist");\n',
    );
    expect(checkRepo(root).problems).toEqual([
      expect.objectContaining({
        name: "does_not_exist",
        kind: expect.stringMatching(/aufgerufen, aber nicht registriert/),
        where: expect.stringMatching(/fileDialog\.ts:\d+$/),
      }),
    ]);
  });

  it("fails and names a #[tauri::command] that is not registered", () => {
    const root = copyRepo();
    edit(root, "crates/app-shell/src/commands/app_meta.rs", (t) =>
      t + "\n#[tauri::command]\npub async fn brand_new_command() -> Result<(), String> {\n    Ok(())\n}\n",
    );
    expect(checkRepo(root).problems).toEqual([
      expect.objectContaining({ name: "brand_new_command", kind: expect.stringMatching(/definiert, aber nicht/) }),
    ]);
  });

  it("fails and names a registration without definition", () => {
    const root = copyRepo();
    edit(root, LIB_RS, (t) => t.replace(/^([ \t]*)commands::get_app_info,(?=\r?$)/m, "$1commands::get_app_info,\n$1commands::ghost_command,"));
    expect(checkRepo(root).problems).toEqual([
      expect.objectContaining({ name: "ghost_command", kind: expect.stringMatching(/kein #\[tauri::command\] definiert/) }),
    ]);
  });

  it("ignores test files and comments", () => {
    const root = copyRepo();
    const src = path.join(root, "apps/smart-ssh-community/frontend/src");
    fs.writeFileSync(path.join(src, "probe.test.ts"), 'invoke("only_in_test_ts");\n');
    fs.writeFileSync(path.join(src, "probe.test.tsx"), 'invoke("only_in_test_tsx");\n');
    fs.writeFileSync(
      path.join(src, "probe.ts"),
      '// invoke("line_comment");\n/* invoke("block_comment"); */\nexport const ok = 1;\n',
    );
    edit(root, "crates/app-shell/src/commands/app_meta.rs", (t) =>
      t + "\n// #[tauri::command]\n// pub fn commented_command() {}\n/* #[tauri::command]\nfn block_commented() {} */\n",
    );
    expect(checkRepo(root).problems).toEqual([]);
  });

  it("checks that every startup-gate allow-list name is registered", () => {
    const root = copyRepo();
    edit(root, "crates/app-shell/src/startup_gate.rs", (t) =>
      t.replace('"quit_application",', '"quit_application",\n    "not_registered_gate_entry",'),
    );
    expect(checkRepo(root).problems).toEqual([
      expect.objectContaining({ name: "not_registered_gate_entry", kind: expect.stringMatching(/ALLOWED_WHILE_LOCKED/) }),
    ]);
  });

  it("exits non-zero from the command line and names each mismatch", () => {
    const script = path.join(REPO_ROOT, "scripts/check-tauri-commands.mjs");
    const root = copyRepo();
    const clean = spawnSync(process.execPath, [script, "--root", root], { encoding: "utf8" });
    expect(clean.status).toBe(0);
    edit(root, "apps/smart-ssh-community/frontend/src/fileDialog.ts", (t) =>
      t + '\nexport const probe = () => invoke("does_not_exist");\n',
    );
    edit(root, "crates/app-shell/src/commands/app_meta.rs", (t) =>
      t + "\n#[tauri::command]\npub async fn brand_new_command() {}\n",
    );
    const broken = spawnSync(process.execPath, [script, "--root", root], { encoding: "utf8" });
    expect(broken.status).toBe(1);
    expect(broken.stderr).toMatch(/does_not_exist: im Frontend aufgerufen, aber nicht registriert/);
    expect(broken.stderr).toMatch(/brand_new_command: definiert, aber nicht in generate_handler! registriert/);
  });

  it("fails loudly on a second generate_handler!", () => {
    const root = copyRepo();
    edit(root, "crates/app-shell/src/commands/app_meta.rs", (t) =>
      t + "\nfn other() { let _ = tauri::generate_handler![get_app_info]; }\n",
    );
    expect(() => checkRepo(root)).toThrow(/mehr als ein `generate_handler!`/);
  });
});

describe("parseDefinedCommands", () => {
  it("accepts arguments, pub(crate), async and extra attributes", () => {
    const src = [
      "#[tauri::command]",
      "fn plain() {}",
      '#[tauri::command(rename_all = "snake_case")]',
      "pub async fn with_args(state: State<'_, AppState>) {}",
      "#[tauri::command]",
      "#[allow(clippy::too_many_arguments)]",
      "/// doc",
      "pub(crate) fn with_attr() {}",
      "#[ tauri :: command ]",
      "pub(in crate::x) async fn spaced() {}",
      'const S: &str = "#[tauri::command] fn in_string() {}";',
      "// #[tauri::command]",
      "// fn commented() {}",
    ].join("\n");
    expect(parseDefinedCommands(src).map((d: { name: string }) => d.name)).toEqual([
      "plain",
      "with_args",
      "with_attr",
      "spaced",
    ]);
  });

  it("fails loudly when no fn follows the attribute", () => {
    expect(() => parseDefinedCommands("#[tauri::command]\nstruct NotAFn;\n", "x.rs")).toThrow(/x\.rs:1/);
  });
});

describe("parseRegisteredCommands", () => {
  it("takes the last path segment and ignores comments", () => {
    const text = [
      "fn run() {",
      "  builder.invoke_handler(tauri::generate_handler![",
      "    commands::a,",
      "    // commands::commented,",
      "    /* commands::block, */",
      "    nested::deep::b,",
      "    c",
      "  ]);",
      "}",
    ].join("\n");
    const names = parseRegisteredCommands([{ file: "lib.rs", text }], "lib.rs").map((r: { name: string }) => r.name);
    expect(names).toEqual(["a", "b", "c"]);
  });

  it("fails loudly on an unreadable entry", () => {
    const text = "tauri::generate_handler![commands::a, some_macro!(x)]";
    expect(() => parseRegisteredCommands([{ file: "lib.rs", text }], "lib.rs")).toThrow(/nicht lesbar/);
  });

  it("fails loudly when the list is in another file", () => {
    expect(() =>
      parseRegisteredCommands([{ file: "other.rs", text: "generate_handler![a]" }], "lib.rs"),
    ).toThrow(/erwartet in lib\.rs/);
  });

  it("does not count a generate_handler! that only appears in a comment", () => {
    const files = [
      { file: "lib.rs", text: "generate_handler![a]" },
      { file: "doc.rs", text: "//! vor dem von `generate_handler!` erzeugten Verteiler\n" },
    ];
    expect(parseRegisteredCommands(files, "lib.rs").map((r: { name: string }) => r.name)).toEqual(["a"]);
  });
});

describe("parseInvokedCommands", () => {
  it("finds plain, generic and multi-line calls, skipping plugins and non-literals", () => {
    const src = [
      'invoke("plain");',
      "invoke<Record<string, () => void>>('generic');",
      "await invoke<Foo>(",
      '  "multi_line",',
      "  { a: 1 },",
      ");",
      'invoke("plugin:store|get");',
      "function invoke<T>(cmd: string) { return tauriInvoke(cmd); }",
      'tauriInvoke("not_counted_here");',
      'const url = "https://example.invalid"; invoke("after_url");',
      "const re = /\\/\\//; invoke(\"after_regex\");",
      "const t = `x ${invoke(\"inside_template\")} // not a comment`;",
    ].join("\n");
    expect(parseInvokedCommands(src).map((v: { name: string }) => v.name)).toEqual([
      "plain",
      "generic",
      "multi_line",
      "after_url",
      "after_regex",
      "inside_template",
    ]);
  });
});

describe("helpers", () => {
  it("recognises frontend test files", () => {
    expect(isFrontendTestFile("src/a.test.ts")).toBe(true);
    expect(isFrontendTestFile("src/components/B.test.tsx")).toBe(true);
    expect(isFrontendTestFile("src/test-setup.ts")).toBe(true);
    expect(isFrontendTestFile("src/api.ts")).toBe(false);
    expect(isFrontendTestFile("src/testing.ts")).toBe(false);
  });

  it("keeps Rust lifetimes, char literals and strings when stripping comments", () => {
    const src = "fn f<'a>(x: &'a str) -> char { let s = \"// no\"; '/' } // gone";
    expect(stripRustComments(src)).toBe("fn f<'a>(x: &'a str) -> char { let s = \"// no\"; '/' }        ");
  });

  it("keeps strings with comment markers in JS", () => {
    expect(stripJsComments('a("/* x */"); // y').trimEnd()).toBe('a("/* x */");');
  });

  it("reads the startup-gate allow list without commented entries", () => {
    const src = 'const ALLOWED_WHILE_LOCKED: &[&str] = &[\n  "a",\n  // "b",\n  "c",\n];';
    expect(parseAllowedWhileLocked(src)).toEqual(["a", "c"]);
  });
});
