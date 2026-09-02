import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import './index.css'
import App from './App.tsx'
import { initI18n } from './i18n.ts'

// Spec 0024, Abschnitt 4: Sprache muss vor dem ersten Render feststehen
// (kein sichtbares Umschalten kurz nach dem Start) — `main.tsx` ist ein
// ES-Modul, Top-Level-`await` ist hier unproblematisch (Vite/moderne
// Browser unterstützen das nativ).
await initI18n()

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>,
)
