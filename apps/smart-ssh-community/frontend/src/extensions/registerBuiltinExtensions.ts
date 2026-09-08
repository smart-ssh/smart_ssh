/** Registriert die bestehenden Settings-Abschnitte über die neue Registry
 * (Spec 0038, Abschnitt 4) — der eine konkrete Migrationsschritt dieses
 * Teils, absichtlich begrenzt auf `AiProviderSettings`s bisher fest
 * verdrahtete `<ChatRetentionSettings />`/`<McpServerSettings />`
 * (Spec 0037/0028), ohne den Rest der App umzustrukturieren. Muss vor dem
 * ersten Render von `AiProviderSettings` importiert sein — geschieht als
 * Modul-Nebeneffekt über den Import in `components/AiProviderSettings.tsx`
 * selbst (ES-Modul-Auswertung läuft vor jedem Funktionsaufruf innerhalb des
 * importierenden Moduls, also auch vor `AiProviderSettings`s erstem
 * Render). */

import { ChatRetentionSettings } from "../components/ChatRetentionSettings";
import { McpServerSettings } from "../components/McpServerSettings";
import { registerSettingsSection } from "./registry";

// Spec 0050, Teil 1: `label` ist der Anzeige-Text für den linken
// Navigations-Eintrag der zweispaltigen Settings-Struktur — bewusst ein
// statischer String statt eines Übersetzungs-Schlüssels (die Registry ist
// ein reines Modul-Singleton ohne i18n-Anbindung, s. `registry.ts`s
// Doc-Kommentar zu `label`); reagiert also nicht auf einen Sprachwechsel,
// anders als der Inhalt der jeweiligen Sektion selbst (der ganz normal
// `useTranslation` nutzt).
registerSettingsSection({
  id: "chat-retention",
  label: "Sitzungen & Daten",
  component: ChatRetentionSettings,
});
registerSettingsSection({ id: "mcp-server", label: "MCP-Server", component: McpServerSettings });
