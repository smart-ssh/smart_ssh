// Spec 0031, Abschnitt 4/6: "Weiter"-Button bleibt deaktiviert, bis die
// Checkbox aktiv ist — kein Wegklicken ohne bewusste Bestätigung.
import { fireEvent, render, screen } from "@testing-library/react";
import { I18nextProvider } from "react-i18next";
import { useEffect, useState } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  registerFirstRunNoticeExtension,
  resetRegistryForTests,
  type FirstRunNoticeExtensionContext,
} from "../extensions/registry";
import { testI18n } from "../testI18n";
import { FirstRunNoticeScreen } from "./FirstRunNoticeScreen";

function renderScreen(
  onAcknowledge: (afterStored: () => void) => void | Promise<void> = vi.fn(),
) {
  render(
    <I18nextProvider i18n={testI18n}>
      <FirstRunNoticeScreen onAcknowledge={onAcknowledge} />
    </I18nextProvider>,
  );
  return onAcknowledge;
}

describe("first-run notice screen (Spec 0031)", () => {
  it("keeps the continue button disabled until the checkbox is checked", () => {
    renderScreen();
    const continueButton = screen.getByRole("button", { name: "Weiter" });
    expect(continueButton).toBeDisabled();

    fireEvent.click(screen.getByRole("checkbox"));
    expect(continueButton).toBeEnabled();

    fireEvent.click(screen.getByRole("checkbox"));
    expect(continueButton).toBeDisabled();
  });

  it("only calls onAcknowledge after the checkbox was checked and continue was clicked", () => {
    const onAcknowledge = renderScreen();
    const continueButton = screen.getByRole("button", { name: "Weiter" });

    // Ein deaktivierter Button feuert in jsdom (wie im echten Browser)
    // keinen Klick-Handler aus — trotzdem hier explizit geprüft, dass ohne
    // vorherige Checkbox-Aktivierung nichts passiert.
    fireEvent.click(continueButton);
    expect(onAcknowledge).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("checkbox"));
    fireEvent.click(continueButton);
    expect(onAcknowledge).toHaveBeenCalledTimes(1);
  });

  it("shows the exact responsibility text from spec 0031", () => {
    renderScreen();
    expect(screen.getByText(/Verantwortung für jedes bestätigte Kommando liegt bei dir/)).toBeInTheDocument();
  });

  // Spec 0101, A21/Klarstellung 1: Seit die Datenbank verschlüsselt ist
  // (SQLCipher, Schlüsselbund oder Master-Passwort), stimmt der alte Satz
  // „nicht zusätzlich verschlüsselt“ nicht mehr — er behauptete das
  // Gegenteil des jetzigen Zustands. *Gegenbeweis:* Dieser Test scheitert
  // gegen den alten Text in `locales/de/common.json`.
  it("shows the exact wording of Klarstellung 1 (database is encrypted, key held by keychain or master password)", () => {
    renderScreen();
    expect(
      screen.getByText(
        "Die lokale Datenbank ist verschlüsselt. Den Schlüssel verwahrt der Schlüsselbund deines Betriebssystems oder – wenn du es einrichtest – dein Master-Passwort. Wer Zugriff auf dein entsperrtes Benutzerkonto hat, kann die Daten lesen.",
      ),
    ).toBeInTheDocument();
    expect(screen.queryByText(/nicht zusätzlich verschlüsselt/)).not.toBeInTheDocument();
  });
});

// Spec 0031, Abschnitt 6 (issue #157): optionale Erweiterungen.
describe("first-run notice extensions (Spec 0031, section 6)", () => {
  afterEach(() => {
    resetRegistryForTests();
    vi.restoreAllMocks();
  });

  /** Simulates the caller: stores the acknowledgement, then signals it. */
  const storingAcknowledge = () => vi.fn((afterStored: () => void) => afterStored());

  function confirm() {
    fireEvent.click(screen.getByRole("checkbox", { name: /gelesen und verstanden/ }));
    fireEvent.click(screen.getByRole("button", { name: "Weiter" }));
  }

  /** An extension with an optional checkbox (off by default, as the rule
   * for extension authors requires) and a continue handler. */
  function optionalCheckboxExtension(
    label: string,
    onContinue: () => void | Promise<void> = () => {},
  ) {
    return function Extension({ onContinue: register }: FirstRunNoticeExtensionContext) {
      const [on, setOn] = useState(false);
      useEffect(() => register(onContinue));
      return (
        <label>
          <input type="checkbox" checked={on} onChange={(e) => setOn(e.target.checked)} />
          {label}
        </label>
      );
    };
  }

  it("without registrations renders no extension area and only the mandatory checkbox", () => {
    renderScreen();
    expect(screen.queryByTestId(/^first-run-notice-extension-/)).toBeNull();
    expect(screen.getAllByRole("checkbox")).toHaveLength(1);
    expect(screen.getAllByRole("button")).toHaveLength(1);
  });

  it("renders each extension in its own area below the mandatory checkbox, in order", () => {
    registerFirstRunNoticeExtension({
      id: "second",
      order: 20,
      Component: optionalCheckboxExtension("Second option"),
    });
    registerFirstRunNoticeExtension({
      id: "first",
      order: 10,
      Component: optionalCheckboxExtension("First option"),
    });
    renderScreen();

    const first = screen.getByTestId("first-run-notice-extension-first");
    const second = screen.getByTestId("first-run-notice-extension-second");
    const mandatory = screen.getByRole("checkbox", { name: /gelesen und verstanden/ });

    expect(first).toHaveTextContent("First option");
    expect(second).toHaveTextContent("Second option");
    expect(first).not.toContainElement(mandatory);
    expect(second).not.toContainElement(mandatory);
    // DOM order: mandatory checkbox, then first, then second.
    expect(mandatory.compareDocumentPosition(first) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(first.compareDocumentPosition(second) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it("keeps continue disabled until the mandatory checkbox is checked, even when an extension's option is checked", () => {
    registerFirstRunNoticeExtension({
      id: "opt",
      order: 1,
      Component: optionalCheckboxExtension("Optional"),
    });
    renderScreen();
    const continueButton = screen.getByRole("button", { name: "Weiter" });
    const optional = screen.getByRole("checkbox", { name: "Optional" });

    expect(optional).not.toBeChecked();
    fireEvent.click(optional);
    expect(continueButton).toBeDisabled();

    fireEvent.click(screen.getByRole("checkbox", { name: /gelesen und verstanden/ }));
    expect(continueButton).toBeEnabled();
  });

  it("calls every registered handler once, only after the caller signals the stored acknowledgement", () => {
    const handlerA = vi.fn();
    const handlerB = vi.fn();
    registerFirstRunNoticeExtension({ id: "a", order: 1, Component: optionalCheckboxExtension("A", handlerA) });
    registerFirstRunNoticeExtension({ id: "b", order: 2, Component: optionalCheckboxExtension("B", handlerB) });

    let signalStored: () => void = () => {};
    const onAcknowledge = vi.fn((afterStored: () => void) => {
      signalStored = afterStored;
    });
    renderScreen(onAcknowledge);

    confirm();
    expect(onAcknowledge).toHaveBeenCalledTimes(1);
    expect(handlerA).not.toHaveBeenCalled();
    expect(handlerB).not.toHaveBeenCalled();

    signalStored();
    signalStored();
    expect(handlerA).toHaveBeenCalledTimes(1);
    expect(handlerB).toHaveBeenCalledTimes(1);
  });

  it("a throwing or rejecting handler is logged and does not stop the other handlers", async () => {
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => {});
    const after = vi.fn();
    registerFirstRunNoticeExtension({
      id: "throws",
      order: 1,
      Component: optionalCheckboxExtension("T", () => {
        throw new Error("sync boom");
      }),
    });
    registerFirstRunNoticeExtension({
      id: "rejects",
      order: 2,
      Component: optionalCheckboxExtension("R", () => Promise.reject(new Error("async boom"))),
    });
    registerFirstRunNoticeExtension({ id: "after", order: 3, Component: optionalCheckboxExtension("A", after) });
    const onAcknowledge = storingAcknowledge();
    renderScreen(onAcknowledge);

    confirm();

    expect(onAcknowledge).toHaveBeenCalledTimes(1);
    expect(after).toHaveBeenCalledTimes(1);
    await vi.waitFor(() => expect(consoleError).toHaveBeenCalledTimes(2));
  });
});

// Spec 0031, Abschnitt 6 (issue #159): Eine Erweiterung, die beim Rendern
// wirft, darf den Pflicht-Hinweis nicht mitreißen. *Gegenbeweis:* Ohne die
// Error-Boundary in `FirstRunNoticeScreen` hängt der Wurf den ganzen Baum
// aus — `render` wirft, und jeder dieser Tests scheitert.
describe("first-run notice extensions that fail to render (Spec 0031, section 6)", () => {
  afterEach(() => {
    resetRegistryForTests();
    vi.restoreAllMocks();
  });

  function confirm() {
    fireEvent.click(screen.getByRole("checkbox", { name: /gelesen und verstanden/ }));
    fireEvent.click(screen.getByRole("button", { name: "Weiter" }));
  }

  function workingExtension(label: string, onContinue: () => void = () => {}) {
    return function Working({ onContinue: register }: FirstRunNoticeExtensionContext) {
      useEffect(() => register(onContinue));
      return <p>{label}</p>;
    };
  }

  /** Registers its continue handler during render, then throws. */
  function throwingExtension(onContinue: () => void = () => {}) {
    return function Throwing({ onContinue: register }: FirstRunNoticeExtensionContext): never {
      register(onContinue);
      throw new Error("render boom");
    };
  }

  const ourLogsFor = (spy: ReturnType<typeof vi.spyOn>, id: string) =>
    spy.mock.calls.filter(
      ([message]) => typeof message === "string" && message.includes(`"${id}"`),
    );

  it("keeps the notice usable and hides only the failing extension's area", () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    registerFirstRunNoticeExtension({ id: "broken", order: 1, Component: throwingExtension() });
    registerFirstRunNoticeExtension({ id: "working", order: 2, Component: workingExtension("Working option") });
    const onAcknowledge = vi.fn();
    renderScreen(onAcknowledge);

    expect(screen.getByText(/Verantwortung für jedes bestätigte Kommando liegt bei dir/)).toBeInTheDocument();
    expect(screen.queryByTestId("first-run-notice-extension-broken")).toBeNull();
    expect(screen.getByTestId("first-run-notice-extension-working")).toHaveTextContent("Working option");

    const continueButton = screen.getByRole("button", { name: "Weiter" });
    expect(continueButton).toBeDisabled();
    fireEvent.click(screen.getByRole("checkbox", { name: /gelesen und verstanden/ }));
    expect(continueButton).toBeEnabled();
    fireEvent.click(continueButton);
    expect(onAcknowledge).toHaveBeenCalledTimes(1);
  });

  it("logs the failure once with the extension id", () => {
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => {});
    registerFirstRunNoticeExtension({ id: "broken", order: 1, Component: throwingExtension() });
    registerFirstRunNoticeExtension({ id: "working", order: 2, Component: workingExtension("Working option") });
    renderScreen();

    const logs = ourLogsFor(consoleError, "broken");
    expect(logs).toHaveLength(1);
    expect(logs[0][0]).toBe('First-run notice extension "broken" failed to render:');
    expect(logs[0][1]).toEqual(new Error("render boom"));
    expect(ourLogsFor(consoleError, "working")).toHaveLength(0);
  });

  it("with only a throwing extension, the notice can still be acknowledged", () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    registerFirstRunNoticeExtension({ id: "broken", order: 1, Component: throwingExtension() });
    const onAcknowledge = vi.fn();
    renderScreen(onAcknowledge);

    expect(screen.queryByTestId(/^first-run-notice-extension-/)).toBeNull();
    confirm();
    expect(onAcknowledge).toHaveBeenCalledTimes(1);
  });

  it("does not call the continue handler of an extension that failed to render", () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    const brokenHandler = vi.fn();
    const workingHandler = vi.fn();
    registerFirstRunNoticeExtension({ id: "broken", order: 1, Component: throwingExtension(brokenHandler) });
    registerFirstRunNoticeExtension({ id: "working", order: 2, Component: workingExtension("W", workingHandler) });
    const onAcknowledge = vi.fn((afterStored: () => void) => afterStored());
    renderScreen(onAcknowledge);

    confirm();

    expect(onAcknowledge).toHaveBeenCalledTimes(1);
    expect(brokenHandler).not.toHaveBeenCalled();
    expect(workingHandler).toHaveBeenCalledTimes(1);
  });

  it("drops the handler of an extension that registered in an effect and throws on a later render", () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    const lateHandler = vi.fn();
    const workingHandler = vi.fn();
    function ThrowsLater({ onContinue: register }: FirstRunNoticeExtensionContext) {
      const [broken, setBroken] = useState(false);
      useEffect(() => register(lateHandler));
      if (broken) throw new Error("late boom");
      return (
        <button type="button" onClick={() => setBroken(true)}>
          Break
        </button>
      );
    }
    registerFirstRunNoticeExtension({ id: "late", order: 1, Component: ThrowsLater });
    registerFirstRunNoticeExtension({ id: "working", order: 2, Component: workingExtension("W", workingHandler) });
    const onAcknowledge = vi.fn((afterStored: () => void) => afterStored());
    renderScreen(onAcknowledge);

    expect(screen.getByTestId("first-run-notice-extension-late")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Break" }));
    expect(screen.queryByTestId("first-run-notice-extension-late")).toBeNull();

    confirm();

    expect(onAcknowledge).toHaveBeenCalledTimes(1);
    expect(lateHandler).not.toHaveBeenCalled();
    expect(workingHandler).toHaveBeenCalledTimes(1);
  });
});
