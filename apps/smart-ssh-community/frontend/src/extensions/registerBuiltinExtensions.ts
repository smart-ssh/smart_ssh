/** Registriert die bestehenden Settings-Abschnitte über die neue Registry
 * (Spec 0038, Abschnitt 4) — der eine konkrete Migrationsschritt dieses
 * Teils, absichtlich begrenzt auf `AiProviderSettings`s bisher fest
 * verdrahtete `<ChatRetentionSettings />`/`<McpServerSettings />`
 * (Spec 0037/0028), ohne den Rest der App umzustrukturieren. Muss vor dem
 * ersten Render von `AiProviderSettings` importiert sein — geschieht als
 * Modul-Nebeneffekt über den Import in `App.tsx`. */

import { ChatRetentionSettings } from "../components/ChatRetentionSettings";
import { McpServerSettings } from "../components/McpServerSettings";
import { registerSettingsSection } from "./registry";

registerSettingsSection({ id: "chat-retention", component: ChatRetentionSettings });
registerSettingsSection({ id: "mcp-server", component: McpServerSettings });
