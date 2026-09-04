import { describe, expect, it } from "vitest";
import { initialHistoryNavState, navigateHistory, type HistoryNavState } from "./promptHistoryNav";

// Reine Verhaltenstests für Spec 0015, Abschnitt 5 — losgelöst von DOM-
// Cursor-Abfragen/Event-Handling (das übernimmt `ChatPanel.tsx`s
// Cursor-Position-Gate, hier bewusst nicht getestet, da rein
// browserabhängig).

describe("navigateHistory", () => {
  const history = ["erstens", "zweitens", "drittens"]; // chronologisch aufsteigend, "drittens" = jüngster

  it("zeigt beim ersten Pfeil-oben den jüngsten Eintrag und speichert den Entwurf", () => {
    const result = navigateHistory("up", history, initialHistoryNavState, "mein Entwurf");

    expect(result).toEqual({
      value: "drittens",
      nextState: { historyIndex: 2, stashedDraft: "mein Entwurf" },
    });
  });

  it("zeigt bei weiterem Pfeil-oben jeweils den nächstälteren Eintrag", () => {
    const afterFirst: HistoryNavState = { historyIndex: 2, stashedDraft: "entwurf" };

    const result = navigateHistory("up", history, afterFirst, "entwurf");

    expect(result).toEqual({
      value: "zweitens",
      nextState: { historyIndex: 1, stashedDraft: "entwurf" },
    });
  });

  it("stoppt beim ältesten Eintrag ohne Fehler", () => {
    const atOldest: HistoryNavState = { historyIndex: 0, stashedDraft: "entwurf" };

    const result = navigateHistory("up", history, atOldest, "entwurf");

    expect(result).toBeNull();
  });

  it("liefert null für Pfeil-oben ohne jede Historie", () => {
    const result = navigateHistory("up", [], initialHistoryNavState, "entwurf");

    expect(result).toBeNull();
  });

  it("navigiert bei Pfeil-unten zurück Richtung jüngstem Eintrag", () => {
    const atOldest: HistoryNavState = { historyIndex: 0, stashedDraft: "entwurf" };

    const result = navigateHistory("down", history, atOldest, "entwurf");

    expect(result).toEqual({
      value: "zweitens",
      nextState: { historyIndex: 1, stashedDraft: "entwurf" },
    });
  });

  it("stellt über den jüngsten Eintrag hinaus den zwischengespeicherten Entwurf wieder her", () => {
    const atNewest: HistoryNavState = { historyIndex: 2, stashedDraft: "mein Entwurf" };

    const result = navigateHistory("down", history, atNewest, "drittens");

    expect(result).toEqual({
      value: "mein Entwurf",
      nextState: { historyIndex: null, stashedDraft: "" },
    });
  });

  it("stellt auch einen leeren zwischengespeicherten Entwurf wieder her", () => {
    const atNewest: HistoryNavState = { historyIndex: 2, stashedDraft: "" };

    const result = navigateHistory("down", history, atNewest, "drittens");

    expect(result).toEqual({
      value: "",
      nextState: { historyIndex: null, stashedDraft: "" },
    });
  });

  it("liefert null für Pfeil-unten außerhalb des Navigations-Modus (normale Cursor-Bewegung)", () => {
    const result = navigateHistory("down", history, initialHistoryNavState, "entwurf");

    expect(result).toBeNull();
  });

  it("voller Rundgang: hoch bis zum ältesten, wieder runter bis zum Entwurf", () => {
    let state = initialHistoryNavState;
    const draft = "mein Entwurf";

    const up1 = navigateHistory("up", history, state, draft)!;
    expect(up1.value).toBe("drittens");
    state = up1.nextState;

    const up2 = navigateHistory("up", history, state, draft)!;
    expect(up2.value).toBe("zweitens");
    state = up2.nextState;

    const up3 = navigateHistory("up", history, state, draft)!;
    expect(up3.value).toBe("erstens");
    state = up3.nextState;

    expect(navigateHistory("up", history, state, draft)).toBeNull();

    const down1 = navigateHistory("down", history, state, draft)!;
    expect(down1.value).toBe("zweitens");
    state = down1.nextState;

    const down2 = navigateHistory("down", history, state, draft)!;
    expect(down2.value).toBe("drittens");
    state = down2.nextState;

    const down3 = navigateHistory("down", history, state, draft)!;
    expect(down3.value).toBe("mein Entwurf");
    expect(down3.nextState).toEqual(initialHistoryNavState);
  });
});
