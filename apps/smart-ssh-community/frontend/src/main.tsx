import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import './index.css'
import App from './App.tsx'
import { getStartupState } from './api.ts'
import { StartupGate } from './components/StartupGate.tsx'
import { initI18n } from './i18n.ts'
import type { StartupStateDto } from './types.ts'

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

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <StartupGate initialState={startupState}>
      <App />
    </StartupGate>
  </StrictMode>,
)
