-- Issue #162: serverseitige Web-Recherche des KI-Providers je
-- Provider-Konfiguration ein-/ausschaltbar. Default an — auch für bereits
-- bestehende Konfigurationen (der `DEFAULT` füllt jede vorhandene Zeile).
-- Wirkt nur bei Providern, die serverseitige Web-Werkzeuge haben (heute
-- Anthropic); für alle anderen wird der Wert ignoriert.
ALTER TABLE ai_provider_configs ADD COLUMN web_research_enabled BOOLEAN NOT NULL DEFAULT TRUE;
