import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
import { defineConfig } from 'vite'

// https://vite.dev/config/
// Server-Einstellungen folgen der Standard-Tauri-Empfehlung (fester Port,
// den tauri.conf.json als `devUrl` referenziert; `strictPort`, damit ein
// belegter Port sofort auffällt statt dass Tauri gegen den falschen
// Dev-Server verbindet).
export default defineConfig({
  plugins: [react(), tailwindcss()],
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
  },
})
