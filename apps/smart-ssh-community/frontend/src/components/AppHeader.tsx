import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { Button, Icon, Logo } from "./ui";
import { MAIN_TAB_ENTRIES, type MainTab } from "../lib/navEntries";
import { ActivityCenter } from "./activities/ActivityCenter";
import { useIsNarrow } from "../hooks/useIsNarrow";
import { getAppInfo } from "../api/api";
import type { AppInfoDto } from "../api/types";
import { TITLEBAR_DRAG_LAYER_Z_INDEX, useIsAnyModalOpen } from "../lib/modalLayer";
import {
  TITLEBAR_HEIGHT_CLASS,
  titlebarDragInsetStyle,
  titlebarPaddingStyle,
  usePlatform,
  type DecorationMode,
} from "../lib/titlebarLayout";

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
  /** Wide layout: the main navigation (Connect / Manage / Filter Rules) lives in the bar. */
  mainNav?: {
    /** `null` while a session is shown: no entry is active then. */
    active: MainTab | null;
    onSelect: (tab: MainTab) => void;
  };
  /** Wide layout: opens the settings (icon button on the right). */
  onOpenSettings?: (opener: HTMLElement) => void;
}

/**
 * Individuelle Titelleiste (Spec 0014).
 *
 * - macOS: Native Ampel-Buttons sitzen oben links -> linker Abstand
 * - Windows / Linux: Native Controls sitzen oben rechts -> rechter Abstand
 * - Drag-Region via `data-tauri-drag-region`
 */
export function AppHeader({ children, mainNav, onOpenSettings }: AppHeaderProps) {
  const { t } = useTranslation();
  const platform = usePlatform();
  const [decorationMode, setDecorationMode] = useState<DecorationMode>("pending");
  const [appInfo, setAppInfo] = useState<AppInfoDto | null>(null);
  const anyModalOpen = useIsAnyModalOpen();
  // Issue #579, Spec 0113 §3: narrow, the bar holds only the children (burger + title).
  const narrow = useIsNarrow();

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
        className={`relative z-30 flex ${TITLEBAR_HEIGHT_CLASS} select-none items-center justify-between border-b border-surface-raised/80 bg-surface-sunken/90 text-fg-muted text-xs backdrop-blur-sm transition-all`}
      >
        {/* Left: Trace logo lockup, then version / edition / early-access / data-dir / dev
            markers in small mono style (spec 0052). */}
        {!narrow && (
        <div data-tauri-drag-region className="flex shrink-0 items-center gap-3">
          <Logo variant="lockup" size={20} />
          {SHOW_EARLY_ACCESS_TITLEBAR_INFO && appInfo && (
            <span data-tauri-drag-region className="flex items-center gap-2 font-mono text-[11px] text-fg-subtle">
              {/* Issue #421: the full text only from 1024 px window width; below it
                  only the dev marker stays (version and edition are in Settings > About). */}
              <span data-tauri-drag-region data-testid="titlebar-version-full" className="hidden lg:inline">
                {appInfo.versionDisplay}
                {appInfo.edition && ` · ${appInfo.edition}`}
                {/* Dev and release builds use separate data directories — only Dev is marked. */}
                {appInfo.buildType === "Dev" && " · Dev"} — Early Access
              </span>
              {appInfo.dataDirDisplay && (
                <span
                  data-tauri-drag-region
                  data-testid="titlebar-data-dir-marker"
                  title={appInfo.dataDir ?? undefined}
                  className="inline-block max-w-[24ch] truncate text-accent"
                >
                  {t("about.dataDirMarker", { path: appInfo.dataDirDisplay })}
                </span>
              )}
              {appInfo.buildType === "Dev" && (
                <span data-tauri-drag-region data-testid="titlebar-dev-marker" className="lg:hidden">
                  · Dev
                </span>
              )}
            </span>
          )}
        </div>
        )}

        {!narrow && mainNav && (
          <nav aria-label={t("nav.label")} className="ml-4 flex h-full shrink-0 items-stretch">
            {MAIN_TAB_ENTRIES.map((entry) => {
              const active = mainNav.active === entry.id;
              return (
                <Button
                  variant="bare"
                  key={entry.id}
                  type="button"
                  onClick={() => mainNav.onSelect(entry.id)}
                  aria-current={active ? "page" : undefined}
                  className={`focus-ring label-condensed flex h-full items-center gap-1.5 border-l border-t-2 border-l-border px-3 lg:px-4 text-[13px] tracking-[0.08em] last:border-r last:border-r-border ${
                    active
                      ? "border-t-accent bg-surface text-fg-strong"
                      : "border-t-transparent text-fg-subtle hover:bg-surface-raised hover:text-fg"
                  }`}
                >
                  {entry.id === "rules" && <Icon name="shield" size={14} />}
                  {t(entry.labelKey)}
                </Button>
              );
            })}
          </nav>
        )}

        {/* Mittlerer Bereich: Platz für künftige Session-Tabs (Spec 0014, Abschnitt 4) */}
        <div
          data-tauri-drag-region
          className={`flex h-full flex-1 items-stretch ${narrow ? "min-w-0 justify-center px-1" : "min-w-0 justify-start"}`}
        >
          {children}
        </div>

        {/* Rechter Bereich: Platzhalter für optionale interaktive Header-Aktionen */}
        <div data-tauri-drag-region className="flex items-center gap-2">
          {/* Interaktive Elemente hier müssen ohne `data-tauri-drag-region` eingebunden werden */}
          {/* Issue #1325: activity center, hidden without activities. */}
          <ActivityCenter />
          {!narrow && onOpenSettings && (
            <Button
              type="button"
              variant="bare"
              aria-label={t("settings.title")}
              title={t("settings.title")}
              onClick={(e) => onOpenSettings(e.currentTarget)}
              className="focus-ring inline-flex h-7 w-7 items-center justify-center border border-transparent text-fg-subtle hover:border-border-strong hover:text-fg"
            >
              <Icon name="sliders" size={16} />
            </Button>
          )}
        </div>
      </header>
      {/* Issue #160 / Spec 0014, Abschnitt 5: Titelleisten-Drag-Schicht.
          Jeder modale Backdrop (`ModalBackdrop`, fixed over the whole window at `z-50`) liegt
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
      {/* Android has no window to drag; there the narrow navigation sheet reaches the top edge. */}
      {anyModalOpen && platform !== "android" && (
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
