import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getAppInfo } from "../api";
import type { AppInfoDto } from "../types";
import { TITLEBAR_DRAG_LAYER_Z_INDEX, useIsAnyModalOpen } from "../modalLayer";
import {
  TITLEBAR_HEIGHT_CLASS,
  titlebarDragInsetStyle,
  titlebarPaddingStyle,
  usePlatform,
  type DecorationMode,
} from "../titlebarLayout";

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
  const platform = usePlatform();
  const [decorationMode, setDecorationMode] = useState<DecorationMode>("pending");
  const [appInfo, setAppInfo] = useState<AppInfoDto | null>(null);
  const anyModalOpen = useIsAnyModalOpen();

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

  // Plattformspezifisches Padding — nur solange die Overlay-Titelleiste
  // tatsächlich aktiv ist (oder die Aktivierung noch aussteht, s.
  // `DecorationMode` in `titlebarLayout.ts`).
  const paddingStyle = titlebarPaddingStyle(platform, decorationMode);

  // Issue #160 / Spec 0014, Abschnitt 5: Freiraum der Drag-Schicht für die
  // Fenster-Controls — dieselben Werte wie das Padding oben, nur ohne den
  // 16px-Innenabstand auf der Seite ohne Controls.
  const dragLayerInsetStyle = titlebarDragInsetStyle(platform, decorationMode);

  return (
    <>
      <header
        data-tauri-drag-region
        style={paddingStyle}
        className={`flex ${TITLEBAR_HEIGHT_CLASS} select-none items-center justify-between border-b border-slate-800/80 bg-slate-950/90 text-slate-300 text-xs backdrop-blur-sm transition-all`}
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
                {appInfo.edition && ` · ${appInfo.edition}`}
                {/* Dev- und Release-Build nutzen getrennte Datenverzeichnisse —
                    nur Dev wird markiert, Release bleibt der Normalfall. */}
                {appInfo.buildType === "Dev" && " · Dev"} — Early Access
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
      {/* Issue #160 / Spec 0014, Abschnitt 5: Titelleisten-Drag-Schicht.
          Jeder modale Backdrop (`ModalBackdrop`, `fixed inset-0 z-50`) liegt
          über dem ganzen Fenster inklusive dieses Headers — ohne diese
          Schicht träfe ein Klick auf die Titelleiste den Backdrop statt der
          Drag-Region, und das Fenster ließe sich nicht mehr bewegen.
          Die Schicht erscheint nur, solange ein Modal offen ist (ohne Modal
          bleibt alles wie bisher, Tabs im Header klickbar). Sie ist
          transparent (der Header bleibt wie gewohnt abgedunkelt), trägt
          keinerlei App-Inhalt, nur `data-tauri-drag-region` (Ziehen und
          Doppelklick-Zoom/-Maximieren über Tauris Drag-Skript), und deckt
          dadurch zugleich die interaktiven Header-Inhalte ab. Sie lässt den
          Bereich der Fenster-Controls frei; die Windows/Linux-Controls des
          Plugins liegen ohnehin darüber (z-Index 100, s. `index.css`).
          Stapelung: `TITLEBAR_DRAG_LAYER_Z_INDEX` (60) > `z-50` der
          Backdrops — ein Dialog mit höherem z-Index würde die Titelleiste
          wieder verdecken (s. `modalLayer.ts`). Bewusst Geschwister statt
          Kind des `<header>`: dessen `backdrop-blur` macht ihn zum
          Bezugsrahmen für `fixed`-Nachfahren und zu einem eigenen
          Stapelkontext, die Schicht käme dann nicht über die Backdrops. */}
      {anyModalOpen && (
        <div
          data-tauri-drag-region
          data-testid="titlebar-drag-layer"
          aria-hidden="true"
          className={`fixed top-0 ${TITLEBAR_HEIGHT_CLASS} select-none`}
          style={{ ...dragLayerInsetStyle, zIndex: TITLEBAR_DRAG_LAYER_Z_INDEX }}
        />
      )}
    </>
  );
}
