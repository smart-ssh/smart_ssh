// Spec 0100 (BL-0232): Der Host-Key-Dialog muss allein mit der Tastatur
// sicher bedienbar sein — Anfangsfokus auf der ablehnenden Schaltfläche,
// Fokus-Fang, Escape lehnt ab, keine Entscheidung ohne Absicht — und von
// Screenreadern als modal angesagt werden. Jeder Test außer T8 scheitert am
// Stand vor dieser Spec (Beleg im Bericht, nicht hier: Fix lokal entfernt,
// Lauf beobachtet, Fix wiederhergestellt).
import { fireEvent, render, screen } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { describe, expect, it, vi } from "vitest";
import { testI18n } from "../testI18n";
import type { HostKeyInfo, HostKeyUserDecision } from "../types";
import { HostKeyDialog } from "./HostKeyDialog";

const unknownEvent: HostKeyInfo = {
  host: "example.com",
  port: 22,
  kind: "unknown",
  fingerprint: "SHA256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
  expectedFingerprint: null,
};

const mismatchEvent: HostKeyInfo = {
  host: "example.com",
  port: 22,
  kind: "mismatch",
  fingerprint: "SHA256:newnewnewnewnewnewnewnewnewnewnewnewnewn",
  expectedFingerprint: "SHA256:oldoldoldoldoldoldoldoldoldoldoldoldold",
};

function renderDialog(event: HostKeyInfo, onDecision: (decision: HostKeyUserDecision) => void = vi.fn()) {
  return render(
    <I18nextProvider i18n={testI18n}>
      <HostKeyDialog event={event} onDecision={onDecision} />
    </I18nextProvider>,
  );
}

const branches = [
  { name: "unbekannt", event: unknownEvent, role: "dialog", rejectLabel: "Ablehnen", trustLabel: "Vertrauen" },
  {
    name: "geändert",
    event: mismatchEvent,
    role: "alertdialog",
    rejectLabel: "Verbindung abbrechen",
    trustLabel: "Trotzdem vertrauen",
  },
] as const;

describe.each(branches)("HostKeyDialog ($name)", ({ event, role, rejectLabel, trustLabel }) => {
  it("T1: sagt Rolle, aria-modal, Name und Beschreibung an (A1)", () => {
    renderDialog(event);
    const dialog = screen.getByRole(role);
    expect(dialog).toHaveAttribute("aria-modal", "true");
    const heading = screen.getByRole("heading", { level: 2 });
    expect(dialog).toHaveAccessibleName(heading.textContent ?? "");
    const describedById = dialog.getAttribute("aria-describedby");
    expect(describedById).toBeTruthy();
    const description = document.getElementById(describedById as string);
    expect(description?.textContent?.length ?? 0).toBeGreaterThan(0);
  });

  it("T2: fokussiert die ablehnende Schaltfläche nach dem Rendern (A2)", () => {
    renderDialog(event);
    expect(document.activeElement).toBe(screen.getByRole("button", { name: rejectLabel }));
  });

  it("T3: ein frisches Enter ohne repeat bleibt unangetastet, Fokus bleibt auf reject (A2, A5)", () => {
    const onDecision = vi.fn();
    renderDialog(event, onDecision);
    const reject = screen.getByRole("button", { name: rejectLabel });
    const notPrevented = fireEvent.keyDown(reject, { key: "Enter", code: "Enter", repeat: false });
    expect(notPrevented).toBe(true);
    expect(document.activeElement).toBe(reject);
    expect(onDecision).not.toHaveBeenCalled();
  });

  it("T4: Tab von der letzten und Shift+Tab von der ersten Schaltfläche bleiben im Dialog (A3)", () => {
    renderDialog(event);
    const reject = screen.getByRole("button", { name: rejectLabel });
    const trust = screen.getByRole("button", { name: trustLabel });

    trust.focus();
    fireEvent.keyDown(trust, { key: "Tab" });
    expect(document.activeElement).toBe(reject);

    reject.focus();
    fireEvent.keyDown(reject, { key: "Tab", shiftKey: true });
    expect(document.activeElement).toBe(trust);
  });

  it("T5: Escape lehnt genau einmal ab und erreicht keinen dahinterliegenden Handler (A4)", () => {
    const onDecision = vi.fn();
    const windowHandler = vi.fn();
    window.addEventListener("keydown", windowHandler);
    try {
      renderDialog(event, onDecision);
      const reject = screen.getByRole("button", { name: rejectLabel });
      fireEvent.keyDown(reject, { key: "Escape" });
      expect(onDecision).toHaveBeenCalledTimes(1);
      expect(onDecision).toHaveBeenCalledWith({ decision: "reject" });
      expect(windowHandler).not.toHaveBeenCalled();

      // zweimal schnell hintereinander -> höchstens ein Aufruf
      fireEvent.keyDown(reject, { key: "Escape" });
      expect(onDecision).toHaveBeenCalledTimes(1);
    } finally {
      window.removeEventListener("keydown", windowHandler);
    }
  });

  it("T6: ein gehaltenes Enter (repeat) unmittelbar nach dem Öffnen wird verworfen (A5)", () => {
    const onDecision = vi.fn();
    renderDialog(event, onDecision);
    const reject = screen.getByRole("button", { name: rejectLabel });
    const notPrevented = fireEvent.keyDown(reject, { key: "Enter", code: "Enter", repeat: true });
    expect(notPrevented).toBe(false);
    expect(onDecision).not.toHaveBeenCalled();
  });

  it("T7: Hintergrund-Klick und Fokusverlust lösen keine Entscheidung aus, Fokus kehrt in den Dialog zurück (A3, A5)", () => {
    const onDecision = vi.fn();
    renderDialog(event, onDecision);
    const dialog = screen.getByRole(role);
    const reject = screen.getByRole("button", { name: rejectLabel });
    const overlay = dialog.parentElement as HTMLElement;

    // (a) Klick auf den Hintergrund löst nichts aus.
    fireEvent.click(overlay);
    expect(onDecision).not.toHaveBeenCalled();

    // (b) Programmatischer Fokus auf ein Element außerhalb -> zurück in den Dialog.
    const outside = document.createElement("button");
    document.body.appendChild(outside);
    outside.focus();
    expect(document.activeElement).toBe(reject);
    document.body.removeChild(outside);

    // (c) blur() auf reject (Fokus geht auf body) -> zurück in den Dialog.
    reject.focus();
    reject.blur();
    expect(document.activeElement).toBe(reject);

    expect(onDecision).not.toHaveBeenCalled();
  });

  it("T8: Klick auf die ablehnende bzw. vertrauende Schaltfläche ruft genau diese Entscheidung auf", () => {
    const onDecisionReject = vi.fn();
    const { unmount } = renderDialog(event, onDecisionReject);
    fireEvent.click(screen.getByRole("button", { name: rejectLabel }));
    expect(onDecisionReject).toHaveBeenCalledTimes(1);
    expect(onDecisionReject).toHaveBeenCalledWith({ decision: "reject" });
    unmount();

    const onDecisionTrust = vi.fn();
    renderDialog(event, onDecisionTrust);
    fireEvent.click(screen.getByRole("button", { name: trustLabel }));
    expect(onDecisionTrust).toHaveBeenCalledTimes(1);
    expect(onDecisionTrust).toHaveBeenCalledWith({ decision: "trust" });
  });
});

describe("HostKeyDialog — Zweig- und Ereigniswechsel", () => {
  it("T2 (adversarial): Anfangsfokus bleibt bei Rerender mit neuem Ereignis oder gewechseltem Zweig auf reject, nie auf trust oder body", () => {
    const onDecision = vi.fn();
    const { rerender } = render(
      <I18nextProvider i18n={testI18n}>
        <HostKeyDialog event={unknownEvent} onDecision={onDecision} />
      </I18nextProvider>,
    );
    expect(document.activeElement).toBe(screen.getByRole("button", { name: "Ablehnen" }));
    expect(document.activeElement).not.toBe(document.body);

    // Neues Ereignis, gleicher Zweig.
    const secondUnknownEvent: HostKeyInfo = { ...unknownEvent, fingerprint: "SHA256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb" };
    rerender(
      <I18nextProvider i18n={testI18n}>
        <HostKeyDialog event={secondUnknownEvent} onDecision={onDecision} />
      </I18nextProvider>,
    );
    expect(document.activeElement).toBe(screen.getByRole("button", { name: "Ablehnen" }));
    expect(document.activeElement).not.toBe(document.body);

    // Zweigwechsel unbekannt -> geändert.
    rerender(
      <I18nextProvider i18n={testI18n}>
        <HostKeyDialog event={mismatchEvent} onDecision={onDecision} />
      </I18nextProvider>,
    );
    const mismatchReject = screen.getByRole("button", { name: "Verbindung abbrechen" });
    expect(document.activeElement).toBe(mismatchReject);
    expect(document.activeElement).not.toBe(screen.queryByRole("button", { name: "Trotzdem vertrauen" }));
    expect(document.activeElement).not.toBe(document.body);
  });

  it("T10: nach der Entscheidung kehrt der Fokus auf das vorher fokussierte Element zurück (A7)", () => {
    const outside = document.createElement("button");
    outside.textContent = "außerhalb";
    document.body.appendChild(outside);
    outside.focus();

    const onDecision = vi.fn();
    const { unmount } = renderDialog(unknownEvent, onDecision);
    const reject = screen.getByRole("button", { name: "Ablehnen" });
    // Liegt nach A2 ohnehin schon hier; der Test setzt es explizit, um die
    // Reihenfolge aus der Spec (T10) nachzubilden.
    reject.focus();
    fireEvent.click(reject);
    unmount();

    expect(document.activeElement).toBe(outside);
    document.body.removeChild(outside);
  });
});
