-- Spec 0101, A9: Die Secrets ziehen aus dem Schlüsselbund des
-- Betriebssystems in die (seit A1/A2 vollständig verschlüsselte) Datenbank.
-- Im Schlüsselbund bleibt höchstens der Wurzelschlüssel K (E2).
--
-- Rein additiv: eine neue Tabelle, keine bestehende wird verändert. Der
-- eigentliche Umzug der vorhandenen Einträge ist ein Startschritt (A10),
-- keine SQL-Migration — er muss jeden Wert einzeln aus dem Schlüsselbund
-- lesen, zurücklesen und vergleichen, und das kann SQL nicht.

CREATE TABLE secrets (
    -- Die `CredentialRef` wörtlich, wie sie bisher der Account-Name im
    -- Schlüsselbund war (`server:{id}:{slot}`, `ai-provider:{id}`) — kein
    -- neues Schlüsselschema. Damit bleibt der Umzug (A10) eine reine
    -- Wertkopie, und eine Referenz, die irgendwo in der Datenbank steht
    -- (`ai_provider_configs.credential_ref`, `auth_method`-JSON), zeigt vor
    -- und nach dem Umzug auf denselben Eintrag.
    ref        TEXT PRIMARY KEY,
    -- Der Secret-Wert im Klartext **innerhalb der verschlüsselten Datei**.
    --
    -- Bewusst **keine** zusätzliche feldweise Verschlüsselung (Spec 0101,
    -- §5, letzter Spiegelstrich): Der Schlüssel dafür wäre K — derselbe
    -- Schlüssel, aus dem schon der Datenbankschlüssel abgeleitet ist (A2).
    -- Eine zweite Hülle unter demselben Schlüssel schützt gegen nichts, was
    -- die erste nicht schon abdeckt, kostet aber bei jedem Verbindungsaufbau
    -- eine Entschlüsselung und schafft einen zweiten Weg, auf dem ein
    -- Schlüssel in einen Fehlertext geraten kann.
    --
    -- `TEXT`, nicht `BLOB`: Der Trait liefert `SecretString`, also eine
    -- gültige UTF-8-Zeichenkette. Ein `BLOB` würde nur verschleiern, dass
    -- hier nie etwas anderes ankommt.
    value      TEXT NOT NULL,
    -- Nur zur Diagnose („wann wurde dieser Eintrag zuletzt geschrieben"),
    -- nie Teil einer fachlichen Entscheidung. Insbesondere entscheidet
    -- **nicht** dieser Wert über den Umzugszustand aus A10.
    updated_at TEXT NOT NULL
);
