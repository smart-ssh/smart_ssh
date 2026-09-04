import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

export type Platform = "macos" | "windows" | "linux" | "unknown";

function detectFallbackPlatform(): Platform {
  if (typeof navigator === "undefined") return "unknown";
  const ua = navigator.userAgent.toLowerCase();
  if (ua.includes("mac")) return "macos";
  if (ua.includes("win")) return "windows";
  if (ua.includes("linux")) return "linux";
  return "unknown";
}

interface AppHeaderProps {
  children?: React.ReactNode;
}

/**
 * Individuelle Titelleiste (Spec 0014).
 *
 * - macOS: Native Ampel-Buttons sitzen oben links -> linker Abstand
 * - Windows / Linux: Native Controls sitzen oben rechts -> rechter Abstand
 * - Drag-Region via `data-tauri-drag-region`
 */
export function AppHeader({ children }: AppHeaderProps) {
  const [platform, setPlatform] = useState<Platform>(detectFallbackPlatform);

  useEffect(() => {
    invoke<string>("get_platform")
      .then((p) => {
        if (p === "macos" || p === "windows" || p === "linux") {
          setPlatform(p);
        }
      })
      .catch((err) => {
        console.warn("Konnte Plattform nicht über Tauri-Command ermitteln, nutze Fallback:", err);
      });

    // Initialisiere die Overlay-Titelleiste
    invoke("create_overlay_titlebar").catch((err) => {
      console.warn("create_overlay_titlebar Fehler:", err);
    });
  }, []);

  const isMac = platform === "macos";

  // Plattformspezifisches Padding:
  // - macOS: Platz für native Traffic Lights links (Startwert-Offset)
  // - Windows/Linux: Platz für native Window-Controls rechts (Min/Max/Close)
  const paddingStyle = isMac
    ? {
        paddingLeft: "max(78px, var(--tauri-plugin-decoration-left-clearance, 78px))",
        paddingRight: "16px",
      }
    : {
        paddingLeft: "16px",
        paddingRight: "max(140px, var(--tauri-plugin-decoration-right-clearance, 140px))",
      };

  return (
    <header
      data-tauri-drag-region
      style={paddingStyle}
      className="flex h-9 select-none items-center justify-between border-b border-slate-800/80 bg-slate-950/90 text-slate-300 text-xs backdrop-blur-sm transition-all"
    >
      {/* Linker Bereich: App-Icon + Schriftzug — Marke aus dem Claude-
          Design-Entwurf (Abschnitt 1a, "Terminal-Cursor mit Spark"):
          eckiger Cursor-Chevron + Balken in Akzentfarbe, optionaler
          Spark oben rechts (ab ~32px Icon-Größe entfernt, s. Entwurf —
          hier bei 16px Titelleisten-Höhe bereits ohne Spark). */}
      <div data-tauri-drag-region className="flex items-center gap-2">
        <svg
          data-tauri-drag-region
          className="h-4 w-4"
          viewBox="0 0 64 64"
          fill="none"
        >
          <path
            d="M20 20 L31 32 L20 44"
            stroke="var(--color-indigo-600)"
            strokeWidth="7"
            strokeLinecap="square"
          />
          <rect x="34" y="38" width="16" height="7" fill="var(--color-indigo-600)" />
        </svg>
        <span
          data-tauri-drag-region
          className="font-heading font-semibold tracking-wide text-slate-100"
        >
          Smart SSH
        </span>
      </div>

      {/* Mittlerer Bereich: Platz für künftige Session-Tabs (Spec 0014, Abschnitt 4) */}
      <div
        data-tauri-drag-region
        className="flex flex-1 items-center justify-center px-4"
      >
        {children}
      </div>

      {/* Rechter Bereich: Platzhalter für optionale interaktive Header-Aktionen */}
      <div data-tauri-drag-region className="flex items-center gap-2">
        {/* Interaktive Elemente hier müssen ohne `data-tauri-drag-region` eingebunden werden */}
      </div>
    </header>
  );
}
