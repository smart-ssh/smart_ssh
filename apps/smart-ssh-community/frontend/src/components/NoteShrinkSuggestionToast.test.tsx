// Spec 0079: Schließen-Knopf (A1) und "Später" (A2/A3) für die
// Notiz-ist-groß-Karte, dazu die Nicht-Vormerkung durch "Mache ich selbst"
// (A5).
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { onNoteShrinkSuggested } from "../events";
import { publishRequestServerNoteEdit } from "../navigationBus";
import type { NoteShrinkSuggestedEvent } from "../types";
import {
  NoteShrinkSuggestionToast,
  resetSnoozedNoteShrinkServersForTests,
} from "./NoteShrinkSuggestionToast";

vi.mock("../api", () => ({
  commandErrorMessage: (err: unknown) => String(err),
  requestNoteShrink: vi.fn(() => Promise.resolve()),
}));

vi.mock("../events", () => ({
  onNoteShrinkSuggested: vi.fn(() => Promise.resolve(() => {})),
  onNoteShrinkFailed: vi.fn(() => Promise.resolve(() => {})),
}));

vi.mock("../navigationBus", () => ({
  publishRequestServerNoteEdit: vi.fn(),
}));

/** Fängt den Handler ab, den `NoteShrinkSuggestionToast` beim Mounten bei
 * `onNoteShrinkSuggested` registriert, und liefert eine Funktion, mit der
 * ein Test ein Event so simuliert, als käme es vom Backend. */
async function renderToastAndCaptureEmit() {
  let handler: ((event: NoteShrinkSuggestedEvent) => void) | null = null;
  vi.mocked(onNoteShrinkSuggested).mockImplementation((h) => {
    handler = h;
    return Promise.resolve(() => {});
  });

  const view = render(<NoteShrinkSuggestionToast />);
  await waitFor(() => expect(handler).not.toBeNull());

  // `act()`, nicht der bloße direkte Aufruf: der simulierte Handler löst
  // ein `setState` außerhalb jeder von Testing-Library verwalteten
  // Interaktion aus (kein `fireEvent`) — ohne `act()` ist der Re-Render
  // nicht garantiert abgeschlossen, bevor der Test direkt danach das DOM
  // abfragt, und eine `queryByText(...)`-Abwesenheitsprüfung könnte grün
  // erscheinen, obwohl sie nur "noch nicht gerendert" statt "unterdrückt"
  // misst.
  const emit = (event: NoteShrinkSuggestedEvent) => act(() => handler!(event));
  return { ...view, emit };
}

const serverA: NoteShrinkSuggestedEvent = { serverId: "server-a", serverName: "Server A" };
const serverB: NoteShrinkSuggestedEvent = { serverId: "server-b", serverName: "Server B" };

describe("NoteShrinkSuggestionToast (Spec 0079)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    // A3: die Merkmenge lebt auf Modulebene und überlebt sonst über Tests
    // hinweg — ohne diesen Reset würde ein "Später" aus einem früheren Test
    // in den nächsten durchsickern.
    resetSnoozedNoteShrinkServersForTests();
  });

  it("T1: 'Hinweis schließen' entfernt nur die aktuelle Karte, ein erneutes Event zeigt sie wieder", async () => {
    const { emit } = await renderToastAndCaptureEmit();

    emit(serverA);
    expect(await screen.findByText("Notiz für Server „Server A“ ist sehr groß")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Hinweis schließen" }));
    expect(screen.queryByText("Notiz für Server „Server A“ ist sehr groß")).toBeNull();

    emit(serverA);
    expect(await screen.findByText("Notiz für Server „Server A“ ist sehr groß")).toBeInTheDocument();
  });

  it("T2: 'Später' unterdrückt weitere Events für denselben Server, andere Server bleiben unberührt", async () => {
    const { emit } = await renderToastAndCaptureEmit();

    emit(serverA);
    await screen.findByText("Notiz für Server „Server A“ ist sehr groß");
    fireEvent.click(screen.getByRole("button", { name: "Später" }));
    expect(screen.queryByText("Notiz für Server „Server A“ ist sehr groß")).toBeNull();

    emit(serverA);
    // Kein `findBy` (das würde auf ein Erscheinen warten, das nie kommt) —
    // synchron prüfen, dass NICHTS gerendert wurde.
    expect(screen.queryByText("Notiz für Server „Server A“ ist sehr groß")).toBeNull();

    emit(serverB);
    expect(await screen.findByText("Notiz für Server „Server B“ ist sehr groß")).toBeInTheDocument();
  });

  it("T3: 'Später' überlebt ein Aushängen und erneutes Einhängen der Komponente", async () => {
    const { emit, unmount } = await renderToastAndCaptureEmit();

    emit(serverA);
    await screen.findByText("Notiz für Server „Server A“ ist sehr groß");
    fireEvent.click(screen.getByRole("button", { name: "Später" }));
    unmount();

    const { emit: emitAgain } = await renderToastAndCaptureEmit();
    emitAgain(serverA);

    expect(screen.queryByText("Notiz für Server „Server A“ ist sehr groß")).toBeNull();
  });

  it("T4: 'Mache ich selbst' merkt den Server NICHT vor — ein späteres Event zeigt die Karte wieder", async () => {
    const { emit } = await renderToastAndCaptureEmit();

    emit(serverA);
    await screen.findByText("Notiz für Server „Server A“ ist sehr groß");
    fireEvent.click(screen.getByRole("button", { name: "Mache ich selbst" }));
    expect(publishRequestServerNoteEdit).toHaveBeenCalledWith("server-a");
    expect(screen.queryByText("Notiz für Server „Server A“ ist sehr groß")).toBeNull();

    emit(serverA);
    expect(await screen.findByText("Notiz für Server „Server A“ ist sehr groß")).toBeInTheDocument();
  });
});
