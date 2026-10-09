import { TITLEBAR_DRAG_LAYER_Z_INDEX } from "../modalLayer";
import { TITLEBAR_HEIGHT_CLASS, titlebarDragInsetStyle, usePlatform } from "../titlebarLayout";

/**
 * Issue #163 / Spec 0014, Abschnitt 5: Drag-Region der Startmasken vor der
 * Entsperrung.
 *
 * Vor der Entsperrung zeigt `StartupGate` statt der App eine eigene Maske —
 * ohne `AppHeader`, also ohne Titelleiste. Diese Leiste ersetzt sie für die
 * Fensterfunktionen: ein transparenter Streifen in Titelleistenhöhe an der
 * Fensteroberkante, nur mit `data-tauri-drag-region` (Ziehen und
 * Doppelklick-Zoom/-Maximieren über Tauris Drag-Skript), ohne App-Inhalt.
 *
 * **Kein `AppHeader` im gesperrten Zustand:** Er ruft `get_app_info` und
 * `create_overlay_titlebar` auf, die das Starttor vor der Entsperrung
 * abweist (Spec 0101, A16). Diese Leiste fragt nur `get_platform` — das
 * steht auf der Positivliste. Die Overlay-Titelleiste aktiviert das Backend
 * beim Start ohnehin selbst; die Leiste hält deshalb dieselben Freiräume
 * wie der Header, solange dessen Aktivierung aussteht ("pending").
 *
 * **Stapelung:** derselbe z-Index wie die Drag-Schicht des Headers
 * (`TITLEBAR_DRAG_LAYER_Z_INDEX`): über dem Backdrop eines Startdialogs
 * (`z-50`), damit das Fenster auch bei offenem Dialog ziehbar bleibt, aber
 * unter den Fenster-Controls des Plugins (z-Index 100). Die Leiste bleibt
 * frei von den Controls (links die macOS-Ampel, rechts Minimieren/
 * Maximieren/Schließen unter Windows/Linux). Maske und Startdialog halten
 * oben einen Rand in Titelleistenhöhe frei, damit keine Eingabe und kein
 * Knopf unter der Leiste liegt.
 */
export function StartupTitleBarDragStrip() {
  const platform = usePlatform();
  return (
    <div
      data-tauri-drag-region
      data-testid="startup-titlebar-drag-strip"
      aria-hidden="true"
      className={`fixed top-0 ${TITLEBAR_HEIGHT_CLASS} select-none`}
      style={{
        ...titlebarDragInsetStyle(platform, "pending"),
        zIndex: TITLEBAR_DRAG_LAYER_Z_INDEX,
      }}
    />
  );
}
