import { StrictMode, type ComponentType, type ReactNode } from 'react'
import { createRoot } from 'react-dom/client'
import App from './App.tsx'
import { getStartupState } from './api.ts'
import { AppRoot } from './components/AppRoot.tsx'
import { StartupGate } from './components/StartupGate.tsx'
import { ErrorBoundary } from './components/ui'
import { initI18n } from './i18n.ts'
import { installContextMenuSuppression } from './contextMenu.ts'
import type { StartupStateDto } from './types.ts'
import { detectFallbackPlatform } from './titlebarLayout.ts'
import { applyStoredTheme, initTheme } from './theme.ts'

export interface BootstrapOptions {
  /** Runs after initI18n and before the first render; may be async. */
  beforeRender?: () => void | Promise<void>
  /**
   * Headless components rendered once as siblings of <App/> inside the
   * StartupGate (so only after unlock), each in its own ErrorBoundary.
   */
  extensions?: ReadonlyArray<{ id: string; Component: ComponentType }>
  /** Optional wrapper around <App/> inside the StartupGate. */
  wrap?: (app: ReactNode) => ReactNode
}

/** The frontend start sequence (Spec 0038, section 4). */
export async function bootstrap(options: BootstrapOptions = {}): Promise<void> {
  // Spec 0114: CSS hook for the Android-only narrow layout (`narrow:` variant).
  document.documentElement.dataset.platform = detectFallbackPlatform()

  // Spec 0101, A16: **Der Startzustand steht vor dem ersten Render.**
  //
  // Zwei Gründe, beide aus der Spec:
  //
  // 1. Die Sprache der Startmasken kommt aus diesem Zustand (ADR 0095 §8).
  //    Im Passwort-Modus sind `store` und `os` vor der Entsperrung nicht
  //    registriert (Klarstellung 9: „Einstellungsdateien sind vor der
  //    Entsperrung nicht lesbar"), `initI18n()` käme dort also nur auf seinen
  //    Rückfall Englisch — unabhängig davon, was der Start selbst aus der
  //    Umgebung bestimmt hat.
  // 2. `App` ruft beim Mounten Daten-Kommandos auf, die das Tor im gesperrten
  //    Zustand mit `APP_LOCKED` abweist. Erst die Antwort hier sagt, ob das
  //    überhaupt gerendert werden darf.
  //
  // Scheitert das Kommando (es steht in der Positivliste des Tors, sollte
  // also nie scheitern), bleibt `null`: `StartupGate` zeigt dann eine
  // Meldung mit „Erneut versuchen" statt eine App, deren Kommandos alle
  // fehlschlagen.
  // Issue #804: cached appearance first (the pre-paint script already set it),
  // then the stored choice once the store is readable (not before the unlock in
  // password mode — `StartupGate` repeats it after unlocking).
  initTheme()

  let startupState: StartupStateDto | null = null
  try {
    startupState = await getStartupState()
  } catch (err) {
    console.error('Startzustand konnte nicht gelesen werden:', err)
  }

  // Spec 0024, Abschnitt 4: Sprache muss vor dem ersten Render feststehen
  // (kein sichtbares Umschalten kurz nach dem Start) — `main.tsx` ist ein
  // ES-Modul, Top-Level-`await` ist hier unproblematisch (Vite/moderne
  // Browser unterstützen das nativ).
  //
  // Ist die App gesperrt, gilt die Sprache aus dem Startzustand; nach dem
  // Entsperren holt `StartupGate` die gespeicherte Wahl nach
  // (`applyStoredLanguage`).
  await initI18n(
    startupState && startupState.screen !== 'unlocked' ? startupState.language : undefined,
  )

  if (startupState && startupState.screen === 'unlocked') {
    try {
      await applyStoredTheme()
    } catch (err) {
      console.error('Gespeicherte Darstellung konnte nicht gelesen werden:', err)
    }
  }

  // Issue #629: no WebView menu ("Reload") in release builds.
  if (import.meta.env.PROD) installContextMenuSuppression()

  if (options.beforeRender) {
    try {
      await options.beforeRender()
    } catch (err) {
      // Optional hook: fail open, the app still renders.
      console.error('beforeRender failed:', err)
    }
  }

  const app = <App />
  createRoot(document.getElementById('root')!).render(
    <StrictMode>
      <AppRoot>
        <StartupGate initialState={startupState}>
          {options.wrap ? options.wrap(app) : app}
          {(options.extensions ?? []).map(({ id, Component }) => (
            <ErrorBoundary key={id} id={id} label="Extension">
              <Component />
            </ErrorBoundary>
          ))}
        </StartupGate>
      </AppRoot>
    </StrictMode>,
  )
}
