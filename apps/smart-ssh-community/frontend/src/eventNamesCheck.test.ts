// @vitest-environment node
//
// Issue #918: `scripts/check-event-names.mjs` keeps the Rust event name
// constants and the frontend `EVENT` constants in sync. The failure cases run
// on small fixtures; the last block runs the check on the real repository.
import { spawnSync } from "node:child_process";
import path from "node:path";
import { describe, expect, it } from "vitest";
import {
  NOT_CONSUMED_BY_CORE_UI,
  checkRepo,
  findMismatches,
  parseFrontendNames,
  parsePayloadKeys,
  parseLiteralListeners,
  parseRustNames,
} from "../../../../scripts/check-event-names.mjs";

const REPO_ROOT = path.resolve(__dirname, "../../../..");

const RUST = `
// comment
pub const DATA_CHANGED: &str = "data-changed";
pub const CHAT_ERROR: &str = "chat-error";
`;
const FRONTEND = `
export const EVENT = {
  CHAT_ERROR: "chat-error",
} as const;
`;

const run = (rust: string, frontend: string, literals: { call: string; file: string; line: number }[] = []) =>
  findMismatches({ rust: parseRustNames(rust), frontend: parseFrontendNames(frontend), literals });

describe("parsers", () => {
  it("reads the Rust names module", () => {
    expect(parseRustNames(RUST).map((e) => [e.key, e.value])).toEqual([
      ["DATA_CHANGED", "data-changed"],
      ["CHAT_ERROR", "chat-error"],
    ]);
  });
  it("reads the frontend EVENT object", () => {
    expect(parseFrontendNames(FRONTEND).map((e) => [e.key, e.value])).toEqual([["CHAT_ERROR", "chat-error"]]);
  });
  it("refuses a file without name constants or with other items", () => {
    expect(() => parseRustNames("// nothing")).toThrow(/no event name/);
    expect(() => parseRustNames(RUST + "pub fn x() {}")).toThrow(/unreadable/);
  });
  it("finds listen/once/payloadListener with a literal, but not with a constant", () => {
    const src = `
      listen<Foo>("a", h);
      once("b", h);
      payloadListener<Foo>("c");
      listen(EVENT.X, h);
      // listen("in a comment")
      emitter.once("method", h);
    `;
    expect(parseLiteralListeners(src).map((l) => l.call)).toEqual(["listen", "once", "payloadListener"]);
  });
});

describe("findMismatches", () => {
  it("accepts matching lists with an allowlisted Rust-only constant", () => {
    expect(run(RUST, FRONTEND)).toEqual([]);
  });

  it("fails when a Rust constant is renamed", () => {
    const problems = run(RUST.replace("CHAT_ERROR", "CHAT_FAILURE"), FRONTEND);
    expect(problems.map((p) => p.kind)).toEqual(
      expect.arrayContaining([
        "frontend constant has no Rust counterpart",
        expect.stringContaining("Rust constant is not in the frontend list"),
      ]),
    );
  });

  it("fails when a frontend constant is renamed", () => {
    const problems = run(RUST, FRONTEND.replace("CHAT_ERROR", "CHAT_FAILURE"));
    expect(problems.map((p) => p.name)).toEqual(expect.arrayContaining(["CHAT_FAILURE", "CHAT_ERROR"]));
  });

  it("fails when the event string differs", () => {
    const problems = run(RUST, FRONTEND.replace('"chat-error"', '"chat-failed"'));
    expect(problems).toHaveLength(1);
    expect(problems[0].kind).toMatch(/value differs/);
  });

  it("fails on a listener with a string literal", () => {
    const problems = run(RUST, FRONTEND, [{ call: "listen", file: "src/x.ts", line: 3 }]);
    expect(problems).toEqual([expect.objectContaining({ where: "src/x.ts:3" })]);
  });

  it("fails when an event constant has no payload entry (issue #920)", () => {
    const keys = parsePayloadKeys('export type EventPayloads = {\n  "data-changed": DataChangedPayload,\n};');
    const problems = findMismatches({
      rust: parseRustNames(RUST),
      frontend: parseFrontendNames(FRONTEND),
      literals: [],
      payloadKeys: keys,
    });
    expect(problems).toEqual([expect.objectContaining({ name: "CHAT_ERROR", kind: expect.stringMatching(/no payload type/) })]);
    const ok = parsePayloadKeys('  "chat-error": ChatErrorPayload,\n  "data-changed": X,');
    expect(
      findMismatches({ rust: parseRustNames(RUST), frontend: parseFrontendNames(FRONTEND), literals: [], payloadKeys: ok }),
    ).toEqual([]);
  });

  it("flags listenTyped/typedListener with a literal", () => {
    expect(parseLiteralListeners('listenTyped("a", h); typedListener("b");').map((l) => l.call)).toEqual([
      "listenTyped",
      "typedListener",
    ]);
  });

  it("fails on a stale allowlist entry", () => {
    const problems = findMismatches({
      rust: parseRustNames(RUST),
      frontend: parseFrontendNames(FRONTEND),
      literals: [],
      notConsumed: { DATA_CHANGED: "x", GONE: "x" },
    });
    expect(problems.map((p) => p.name)).toEqual(["GONE"]);
  });

  it("every allowlist entry has a reason", () => {
    for (const reason of Object.values(NOT_CONSUMED_BY_CORE_UI)) expect(reason.length).toBeGreaterThan(5);
  });
});

describe("check-event-names on the real repository", () => {
  it("finds no problems", () => {
    expect(checkRepo(REPO_ROOT).problems).toEqual([]);
  });

  it("exits 0 when run as a script", () => {
    const r = spawnSync("node", [path.join(REPO_ROOT, "scripts/check-event-names.mjs")], { encoding: "utf8" });
    expect(r.status).toBe(0);
  });
});
