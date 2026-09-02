-- Spec 0025, Abschnitt 3/4: Erweiterung von `ai_provider_configs` um
-- anbieterspezifische Zusatz-Header und einen optionalen
-- TEE-Attestierungs-Endpunkt.
--
-- `extra_headers` als JSON-kodiertes Array von [key, value]-Paaren statt
-- einer eigenen Zeilen-pro-Header-Tabelle: die Header gehören untrennbar
-- zu genau einer Provider-Config, es gibt keinen Bedarf, sie einzeln zu
-- adressieren/zu joinen — ein einzelnes TEXT-Feld spart eine Tabelle plus
-- Join für einen rein internen Konfigurationswert. `NOT NULL DEFAULT '[]'`
-- statt NULL-fähig: vermeidet ein drittes Unterscheidungsmerkmal
-- (NULL vs. leeres Array) für denselben Fall "keine Zusatz-Header".
ALTER TABLE ai_provider_configs ADD COLUMN extra_headers TEXT NOT NULL DEFAULT '[]';

-- `attestation_url` optional wie `base_url` (nur gesetzt, wenn der Nutzer
-- einen TEE-Attestierungs-Endpunkt hinterlegt hat).
ALTER TABLE ai_provider_configs ADD COLUMN attestation_url TEXT;
