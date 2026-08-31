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
      {/* Linker Bereich: App-Icon + Schriftzug */}
      <div data-tauri-drag-region className="flex items-center gap-2">
        <svg
          data-tauri-drag-region
          className="h-4 w-4 text-emerald-400"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="2"
          strokeLinecap="round"
          strokeLinejoin="round"
        >
          <rect width="20" height="16" x="2" y="4" rx="3" />
          <path d="m7 10 3 2-3 2" />
          <path d="M13 14h4" />
        </svg>
        <span
          data-tauri-drag-region
          className="font-semibold tracking-wider text-slate-200"
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
