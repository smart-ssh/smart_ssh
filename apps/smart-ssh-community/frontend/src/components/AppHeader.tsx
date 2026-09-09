import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getAppInfo } from "../api";
import type { AppInfoDto } from "../types";

export type Platform = "macos" | "windows" | "linux" | "unknown";

/** Spec 0052, Abschnitt 3.3: die Titelzeile zeigt Version+Hash (+ Edition,
 * z. B. "· Official" — spec-optional "falls billig", hier billig genug für
 * die Zwei-Repo-Situation aus Abschnitt 5: ein privates Official-Binary mit
 * gespiegeltem `AppHeader.tsx` bekäme sonst gar keine Edition-Kennung in
 * der Leiste) nur, solange die App in der 0.x-Testphase ist — bewusst als
 * **ein** Schalter gebaut (statt an mehreren Stellen verstreut), gekoppelt
 * an dieselbe "Early Access"-Kennzeichnung, die im selben Textstück steht.
 * Für die spätere 1.0 hier auf `false` setzen: der gesamte Zusatz
 * verschwindet dann in einem Schritt aus der Titelzeile, ohne nach
 * mehreren Stellen suchen zu müssen. */
const SHOW_EARLY_ACCESS_TITLEBAR_INFO = true;

/** Spec 0049, Fund 3/4: `create_overlay_titlebar` liefert jetzt zurück, ob
 * die plattformspezifische Overlay-Titelleiste des Plugins tatsächlich
 * aktiv ist ("custom" — macOS-Ampel bzw. die HTML-Controls des Plugins auf
 * Windows/Linux) oder ob die Aktivierung fehlgeschlagen ist und auf die
 * native Titelleiste zurückgefallen wurde ("native" — dann rendert das
 * Betriebssystem seine eigene Titelzeile inkl. eigener Minimieren-/
 * Maximieren-/Schließen-Controls **oberhalb** dieses Headers, der dann
 * keinerlei reservierten Platz mehr braucht). "pending" ist der kurze
 * Moment zwischen Mount und der ersten Antwort des Backends — hält
 * bewusst dieselbe reservierte Platzierung wie "custom", damit während
 * dieses kurzen Fensters kein sichtbarer Sprung im Layout entsteht, falls
 * die Aktivierung (der Normalfall) erfolgreich ist. */
type DecorationMode = "pending" | "custom" | "native";

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
  const [decorationMode, setDecorationMode] = useState<DecorationMode>("pending");
  const [appInfo, setAppInfo] = useState<AppInfoDto | null>(null);

  useEffect(() => {
    if (!SHOW_EARLY_ACCESS_TITLEBAR_INFO) return;
    getAppInfo()
      .then(setAppInfo)
      .catch((err) => {
        // Rein kosmetisch — die Titelzeile zeigt dann einfach nur
        // "Smart SSH" ohne Versions-Zusatz, kein Blockieren des Headers.
        console.warn("get_app_info fehlgeschlagen:", err);
      });
  }, []);

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

    // Initialisiere die Overlay-Titelleiste — der Rückgabewert sagt, ob
    // sie tatsächlich aktiv wurde oder das Backend auf die native
    // Titelleiste zurückgefallen ist (s. `DecorationMode`-Doc-Kommentar).
    invoke<string>("create_overlay_titlebar")
      .then((mode) => {
        if (mode === "custom" || mode === "native") {
          setDecorationMode(mode);
        }
      })
      .catch((err) => {
        console.warn("create_overlay_titlebar Fehler:", err);
        // Kein Rückgabewert erhalten -> im Zweifel keinen Platz für
        // Controls reservieren, die vielleicht gar nicht da sind, statt
        // einer möglicherweise leeren Lücke im Header.
        setDecorationMode("native");
      });
  }, []);

  const isMac = platform === "macos";

  // Plattformspezifisches Padding — nur solange die Overlay-Titelleiste
  // tatsächlich aktiv ist (oder die Aktivierung noch aussteht, s. o.).
  // Ist auf "native" zurückgefallen, zeichnet das Betriebssystem seine
  // eigene Titelzeile oberhalb dieses Headers; hier ist dann kein
  // reservierter Platz mehr nötig.
  const paddingStyle =
    decorationMode === "native"
      ? { paddingLeft: "16px", paddingRight: "16px" }
      : isMac
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
          {SHOW_EARLY_ACCESS_TITLEBAR_INFO && appInfo && (
            <span
              data-tauri-drag-region
              className="ml-1.5 font-normal tracking-normal text-slate-500"
            >
              {appInfo.versionDisplay}
              {appInfo.edition && ` · ${appInfo.edition}`} — Early Access
            </span>
          )}
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
