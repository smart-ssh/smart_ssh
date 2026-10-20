import { describe, expect, it } from "vitest";
import { deriveQuestionStates } from "./QuestionCard";

const options = [
  { label: "Alpha", description: null },
  { label: "Beta", description: null },
];
const q = (id: string, multiSelect = false) => ({ type: "question", id, options, multiSelect });
const u = (text: string) => ({ type: "user", id: `u-${text}`, text });

describe("deriveQuestionStates (issue #547)", () => {
  it("keeps an unanswered question active", () => {
    expect(deriveQuestionStates([q("a")]).get("a")).toEqual({ locked: false, selected: [], ownWords: false });
  });

  it("marks an exact label match", () => {
    expect(deriveQuestionStates([q("a"), u("Beta")]).get("a")).toEqual({ locked: true, selected: ["Beta"], ownWords: false });
  });

  it("marks own words when nothing matches", () => {
    expect(deriveQuestionStates([q("a"), u("beta")]).get("a")).toEqual({ locked: true, selected: [], ownWords: true });
    expect(deriveQuestionStates([q("a"), u("Alpha\nBeta")]).get("a")?.ownWords).toBe(true);
  });

  it("marks every line of a multi-select answer", () => {
    expect(deriveQuestionStates([q("a", true), u("Alpha\nBeta")]).get("a")).toEqual({
      locked: true,
      selected: ["Alpha", "Beta"],
      ownWords: false,
    });
    expect(deriveQuestionStates([q("a", true), u("Alpha\nGamma")]).get("a")?.ownWords).toBe(true);
  });

  it("locks an older unanswered question when a newer one exists", () => {
    const s = deriveQuestionStates([q("a"), q("b")]);
    expect(s.get("a")).toEqual({ locked: true, selected: [], ownWords: false });
    expect(s.get("b")?.locked).toBe(false);
  });
});
