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

// Spec 0055, Teil 4 (0050-Review-Fund): `label` war hier ursprünglich ein
// fester deutscher String — in der englischen UI stand dadurch ein
// deutscher Nav-Eintrag zwischen englischen. Jetzt ein
// Übersetzungsschlüssel, den `SettingsScreen.tsx`s `resolveSectionLabel`
// über `i18next.exists()` erkennt und auflöst (s. dortiger Doc-Kommentar)
// — die Registry selbst bleibt weiterhin i18n-unabhängig (s. `registry.ts`s
// Doc-Kommentar zu `label`), nur der hier übergebene String-WERT ändert
// sich von einem Anzeigetext zu einem Pfad in `common.json`.
registerSettingsSection({
  id: "chat-retention",
  label: "settings.categories.chatRetention",
  component: ChatRetentionSettings,
});
registerSettingsSection({
  id: "mcp-server",
  label: "settings.categories.mcpServer",
  component: McpServerSettings,
});
