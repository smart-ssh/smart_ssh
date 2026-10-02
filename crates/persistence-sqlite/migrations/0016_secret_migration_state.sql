-- Spec 0101, A10: der Zustand des einmaligen Umzugs der Secrets aus dem
-- Schlüsselbund des Betriebssystems in die Datenbank (Migration 0015).
--
-- Warum eine Tabelle und keine SQL-Migration für den Umzug selbst: Jeder
-- Wert muss aus dem Schlüsselbund gelesen, hierher geschrieben,
-- zurückgelesen und verglichen werden. Das kann SQL nicht, und es kann
-- mitten drin scheitern — der Fortschritt muss also einen Programmabbruch
-- überdauern.

CREATE TABLE secret_migration_state (
    -- Genau eine Zeile, erzwungen vom CHECK. Ein zweiter Zustand wäre kein
    -- Zustand mehr.
    id           INTEGER PRIMARY KEY CHECK (id = 1),
    -- `open`      — noch nichts umgezogen (oder ein Lesefehler hat
    --               abgebrochen; dann ist auch nichts geschrieben worden,
    --               was zählt).
    -- `moved`     — **alle** Referenzen sind umgezogen und zurückgelesen
    --               gleich; die Schlüsselbund-Einträge stehen noch zum
    --               Löschen aus.
    -- `done`      — alle Einträge gelöscht oder schon nicht mehr vorhanden.
    -- `skipped`   — A11.1 (Etappe 3): der Nutzer hat „Ohne Übernahme
    --               fortfahren" gewählt. In diesem Zustand wird **nie** ein
    --               Schlüsselbund-Eintrag gelöscht. Der Wert ist hier schon
    --               erlaubt, damit Etappe 3 keine Schema-Änderung braucht;
    --               erzeugt wird er bis dahin von keinem Weg.
    state        TEXT NOT NULL CHECK (state IN ('open', 'moved', 'done', 'skipped')),
    -- A10: „Beim Übergang nach *umgezogen* wird die Liste der zu löschenden
    -- Referenzen festgehalten; gelöscht wird nach dieser Liste, nicht nach
    -- dem späteren Datenbankstand." JSON-Array von `CredentialRef`-Strings.
    --
    -- Der Unterschied ist nicht theoretisch: Löscht der Nutzer einen Server,
    -- nachdem sein Secret umgezogen ist, aber bevor der Schlüsselbund-Eintrag
    -- weg ist, dann nennt der Datenbankstand diese Referenz nicht mehr — und
    -- das Secret bliebe für immer im Schlüsselbund liegen.
    pending_refs TEXT NOT NULL
);

-- Eine frische Installation hat nichts umzuziehen; sie durchläuft `open` →
-- `moved` (leere Liste) → `done` beim ersten Start, ohne den Schlüsselbund
-- überhaupt anzufassen. Das ist bewusst derselbe Weg wie bei einer
-- bestehenden Installation: ein zweiter Startpfad „ist neu, überspringe"
-- wäre eine Verzweigung, die nie getestet würde.
INSERT INTO secret_migration_state (id, state, pending_refs) VALUES (1, 'open', '[]');
