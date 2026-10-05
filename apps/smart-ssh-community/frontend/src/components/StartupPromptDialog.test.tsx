// Spec 0101, A3/A13, Klarstellung 10e: der Dialog für Startfragen —
// welche Knöpfe eine Frageart zeigt, und dass ein reiner Hinweis **keine**
// Antwort erzeugt.
//
// Eigene Prüfungen neben `StartupGate.test.tsx`: Die Zusage aus 10e hängt
// an zwei Stellen (diese Komponente schickt für einen Hinweis nichts, und
// das Tor gibt ihr dafür auch keinen sendenden Rückruf mit). Ein Test, der
// nur das Zusammenspiel prüft, bliebe grün, solange eine der beiden hält —
// und sagte nichts darüber, welche.
import { fireEvent, render, screen } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { describe, expect, it, vi } from "vitest";
import type { StartupPromptKind } from "../types";
import { testI18n } from "../testI18n";
import { StartupPromptDialog } from "./StartupPromptDialog";

function renderDialog(kind: StartupPromptKind) {
  const onAnswer = vi.fn();
  const onDismiss = vi.fn();
  render(
    <I18nextProvider i18n={testI18n}>
      <StartupPromptDialog
        request={{ kind, title: "Titel-0101", message: "Text-0101" }}
        onAnswer={onAnswer}
        onDismiss={onDismiss}
      />
    </I18nextProvider>,
  );
  return { onAnswer, onDismiss };
}

/** Die Knöpfe, die eine Frageart zeigen **darf** — dieselbe Positivliste
 * wie im Backend (`window_prompt::choice_for`). */
const EXPECTED: Record<StartupPromptKind, string[]> = {
  retryOrQuit: ["Erneut versuchen", "Beenden"],
  retrySkipOrQuit: ["Erneut versuchen", "Ohne Übernahme fortfahren", "Beenden"],
  retrySetUpOrQuit: ["Erneut versuchen", "Master-Passwort einrichten", "Beenden"],
  startOverOrQuit: ["Neu anfangen", "Beenden"],
  newKeyOrQuit: ["Neuen Schlüssel erzeugen", "Beenden"],
  confirmStartOver: ["Ja, fortfahren", "Abbrechen"],
  confirmNewKey: ["Ja, fortfahren", "Abbrechen"],
  newMasterPassword: ["Ja, fortfahren", "Abbrechen"],
  notice: ["OK"],
};

const ALL_LABELS = [
  ...new Set(Object.values(EXPECTED).flat()),
];

describe("StartupPromptDialog", () => {
  it("zeigt je Frageart genau die Knöpfe, die sie anbietet (A3)", () => {
    for (const [kind, expected] of Object.entries(EXPECTED) as [StartupPromptKind, string[]][]) {
      const view = render(
        <I18nextProvider i18n={testI18n}>
          <StartupPromptDialog
            request={{ kind, title: "Titel-0101", message: "Text-0101" }}
            onAnswer={vi.fn()}
            onDismiss={vi.fn()}
          />
        </I18nextProvider>,
      );
      for (const label of ALL_LABELS) {
        const present = screen.queryByRole("button", { name: label }) !== null;
        expect(present, `${kind}: Knopf „${label}“`).toBe(expected.includes(label));
      }
      view.unmount();
    }
  });

  it("schickt für einen Hinweis keine Antwort, sondern klickt ihn nur weg (Klarstellung 10e)", () => {
    const { onAnswer, onDismiss } = renderDialog("notice");

    fireEvent.click(screen.getByRole("button", { name: "OK" }));

    // Das Backend wartet bei einem Hinweis auf keine Antwort. Eine Antwort
    // wäre eine Antwort ohne Frage — und könnte die nächste, echte Frage
    // vorab beantworten.
    expect(onAnswer).not.toHaveBeenCalled();
    expect(onDismiss).toHaveBeenCalledTimes(1);
  });

  it("gibt das neue Master-Passwort nur mit der Antwort weiter (A13, §6)", () => {
    const { onAnswer } = renderDialog("newMasterPassword");

    fireEvent.change(screen.getByLabelText("Neues Master-Passwort"), {
      target: { value: "Passwort-0101-lang" },
    });
    fireEvent.change(screen.getByLabelText("Master-Passwort wiederholen"), {
      target: { value: "Passwort-0101-lang" },
    });
    fireEvent.click(screen.getByRole("checkbox"));
    fireEvent.click(screen.getByRole("button", { name: "Ja, fortfahren" }));

    // Klarstellung 12: Die Bestätigung der Warnung reist mit — sie wird im
    // Backend geprüft, der ausgegraute Knopf ist nur die freundliche
    // Hälfte. Bliebe sie hier liegen, lehnte das Backend jede Einrichtung
    // aus dem Startdialog ab.
    expect(onAnswer).toHaveBeenCalledWith(
      "confirm",
      "Passwort-0101-lang",
      "Passwort-0101-lang",
      true,
    );
  });

  it("gibt beim Abbrechen kein Passwort mit (A5/A13: nichts verändert)", () => {
    const { onAnswer } = renderDialog("newMasterPassword");

    fireEvent.change(screen.getByLabelText("Neues Master-Passwort"), {
      target: { value: "Passwort-0101-lang" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Abbrechen" }));

    expect(onAnswer).toHaveBeenCalledWith("cancel");
  });
});
