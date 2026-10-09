import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

/** Spec 0014, Abschnitte 4 und 5: gemeinsame Platzierung der Titelleiste
 * relativ zu den nativen Fenster-Controls. Genutzt von `AppHeader` (die
 * entsperrte App) und von `StartupTitleBarDragStrip` (die Startmasken vor
 * der Entsperrung, Issue #163) — damit die Freiräume für Ampel bzw.
 * Minimieren/Maximieren/Schließen an genau einer Stelle stehen. */

export type Platform = "macos" | "windows" | "linux" | "unknown";

/** Spec 0049, Fund 3/4: `create_overlay_titlebar` liefert zurück, ob die
 * plattformspezifische Overlay-Titelleiste des Plugins tatsächlich aktiv
 * ist ("custom" — macOS-Ampel bzw. die HTML-Controls des Plugins auf
 * Windows/Linux) oder ob die Aktivierung fehlgeschlagen ist und auf die
 * native Titelleiste zurückgefallen wurde ("native" — dann rendert das
 * Betriebssystem seine eigene Titelzeile inkl. eigener Minimieren-/
 * Maximieren-/Schließen-Controls **oberhalb** des Headers, der dann
 * keinerlei reservierten Platz mehr braucht). "pending" ist der Zustand,
 * solange keine Antwort vorliegt — hält bewusst dieselbe reservierte
 * Platzierung wie "custom", damit kein sichtbarer Sprung im Layout
 * entsteht, falls die Aktivierung (der Normalfall) erfolgreich ist. */
export type DecorationMode = "pending" | "custom" | "native";

/** Höhe der Titelleiste (Tailwind `h-9`, 36px) — für Header und
 * Drag-Schichten gleichermaßen. */
export const TITLEBAR_HEIGHT_CLASS = "h-9";

/** Freiraum für die macOS-Ampel (links). */
const MAC_CONTROLS_CLEARANCE = "max(78px, var(--tauri-plugin-decoration-left-clearance, 78px))";
/** Freiraum für die Controls des Plugins unter Windows/Linux (rechts). */
const OTHER_CONTROLS_CLEARANCE = "max(140px, var(--tauri-plugin-decoration-right-clearance, 140px))";
/** Innenabstand auf der Seite ohne Controls. */
const EDGE_PADDING = "16px";

export function detectFallbackPlatform(): Platform {
  if (typeof navigator === "undefined") return "unknown";
  const ua = navigator.userAgent.toLowerCase();
  if (ua.includes("mac")) return "macos";
  if (ua.includes("win")) return "windows";
  if (ua.includes("linux")) return "linux";
  return "unknown";
}

/** Plattform über `get_platform` (auch vor der Entsperrung erlaubt,
 * Spec 0101 A16), bis zur Antwort bzw. bei einem Fehler aus dem
 * User-Agent geschätzt. */
export function usePlatform(): Platform {
  const [platform, setPlatform] = useState<Platform>(detectFallbackPlatform);
  useEffect(() => {
    let cancelled = false;
    invoke<string>("get_platform")
      .then((p) => {
        if (cancelled) return;
        if (p === "macos" || p === "windows" || p === "linux") {
          setPlatform(p);
        }
      })
      .catch((err) => {
        console.warn("Konnte Plattform nicht über Tauri-Command ermitteln, nutze Fallback:", err);
      });
    return () => {
      cancelled = true;
    };
  }, []);
  return platform;
}

/** Innenabstand des Headers: Platz für die nativen Controls nur, solange
 * die Overlay-Titelleiste aktiv ist (oder die Aktivierung aussteht). */
export function titlebarPaddingStyle(
  platform: Platform,
  decorationMode: DecorationMode,
): { paddingLeft: string; paddingRight: string } {
  if (decorationMode === "native") {
    return { paddingLeft: EDGE_PADDING, paddingRight: EDGE_PADDING };
  }
  return platform === "macos"
    ? { paddingLeft: MAC_CONTROLS_CLEARANCE, paddingRight: EDGE_PADDING }
    : { paddingLeft: EDGE_PADDING, paddingRight: OTHER_CONTROLS_CLEARANCE };
}

/** Seitliche Grenzen einer festen Drag-Schicht über der Titelleiste:
 * dieselben Freiräume wie das Padding, aber ohne den Innenabstand auf der
 * Seite ohne Controls — die Schicht reicht dort bis an den Fensterrand. */
export function titlebarDragInsetStyle(
  platform: Platform,
  decorationMode: DecorationMode,
): { left: string; right: string } {
  if (decorationMode === "native") return { left: "0px", right: "0px" };
  return platform === "macos"
    ? { left: MAC_CONTROLS_CLEARANCE, right: "0px" }
    : { left: "0px", right: OTHER_CONTROLS_CLEARANCE };
}
