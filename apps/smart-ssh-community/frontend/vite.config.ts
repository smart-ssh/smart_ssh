import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'

// https://vite.dev/config/
// Server-Einstellungen folgen der Standard-Tauri-Empfehlung (fester Port,
// den tauri.conf.json als `devUrl` referenziert; `strictPort`, damit ein
// belegter Port sofort auffällt statt dass Tauri gegen den falschen
// Dev-Server verbindet).
//
// Bewusst kein `defineConfig(...)`-Wrapper: `vite`s eigenes `defineConfig`
// kennt kein `test`-Feld, `vitest/config`s `defineConfig` bringt dafür eine
// eigene, mit dieser `vite`-Version leider inkompatible vendorisierte
// Vite-Kopie mit (Typkonflikt bei den `Plugin`-Typen, s. History). Vite und
// Vitest lesen zur Laufzeit ohnehin nur die für sie jeweils relevanten
// Felder direkt aus diesem Objekt — ein reines Objektliteral vermeidet den
// Typkonflikt komplett, auf Kosten etwas Editor-Autovervollständigung.
export default {
  plugins: [react(), tailwindcss()],
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
  },
  // Spec 0029/0031: erste Tests, die tatsächlich Komponenten rendern
  // (React Testing Library), statt reiner Logik wie die bisherigen
  // `*.test.ts`-Dateien — dafür ein DOM nötig (jsdom), das Node selbst
  // nicht mitbringt.
  test: {
    environment: 'jsdom',
    setupFiles: ['./src/test-setup.ts'],
  },
}
