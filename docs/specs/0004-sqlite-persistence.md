# Spec: SQLite-Persistenz für Server-Profile

Status: Entwurf
Modul: neue Crate `crates/persistence-sqlite`
Abhängigkeiten: `ssh-manager-core` (implementiert dessen `ProfileStore`-Trait
aus Spec 0003)

## 1. Architektur-Entscheidung: eigene Crate statt Teil von `core`

Die SQLite-Anbindung wird **nicht** in `ssh-manager-core` implementiert,
sondern in einer neuen Crate `crates/persistence-sqlite`, die den
`ProfileStore`-Trait aus Spec 0003 implementiert.

Begründung: `core` soll frei von I/O-Abhängigkeiten und schnell testbar
bleiben (reine Logik, In-Memory-Implementierungen für Tests). Eine konkrete
DB-Anbindung ist ein austauschbares Detail, kein Kernbestandteil der Logik.
Das folgt demselben Prinzip wie die Trennung `core`/`app-tauri` aus Spec 0001
— nur eine Ebene tiefer. Sollte später eine andere Storage-Lösung nötig
werden (z. B. für Sync zwischen Geräten), tauscht man diese Crate aus, ohne
`core` anzufassen.

## 2. Technologie

**`sqlx`** mit SQLite-Backend, `runtime-tokio` + `rustls`-Feature.
Begründung: compile-time-geprüfte Queries (`sqlx::query!`), nativer
async/await-Support passend zu Tauris async Command-Handlern, eingebettete
Migrationen über `sqlx::migrate!()` — kein manueller Migrationsschritt für
Endnutzer nötig, die Migrationen laufen automatisch beim App-Start.

Alternative `rusqlite` wurde verworfen, da synchron und ohne
compile-time-Query-Checks.

## 3. Speicherort der Datenbank

Plattformspezifischer App-Datenordner über die `directories`-Crate:

- macOS: `~/Library/Application Support/Smart SSH/smart-ssh.db`
- Windows: `%APPDATA%\Smart SSH\smart-ssh.db`
- Linux: `~/.local/share/smart-ssh/smart-ssh.db`

Der Pfad wird nicht hartcodiert, sondern über
`directories::ProjectDirs::from(...)` ermittelt.

## 4. Schema

```sql
-- migrations/0001_initial.sql

PRAGMA journal_mode = WAL;

CREATE TABLE groups (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    parent_id   TEXT REFERENCES groups(id) ON DELETE CASCADE,
    notes       TEXT NOT NULL DEFAULT '',
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);

CREATE INDEX idx_groups_parent ON groups(parent_id);

CREATE TABLE servers (
    id              TEXT PRIMARY KEY,
    name            TEXT NOT NULL,
    host            TEXT NOT NULL,
    port            INTEGER NOT NULL,
    username        TEXT NOT NULL,
    group_id        TEXT REFERENCES groups(id) ON DELETE SET NULL,
    auth_method     TEXT NOT NULL,   -- JSON-serialisiertes AuthMethod-Enum
    notes           TEXT NOT NULL DEFAULT '',
    jump_host_id    TEXT REFERENCES servers(id) ON DELETE SET NULL,
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL
);

CREATE INDEX idx_servers_group ON servers(group_id);

CREATE TABLE server_tags (
    server_id   TEXT NOT NULL REFERENCES servers(id) ON DELETE CASCADE,
    tag         TEXT NOT NULL,
    PRIMARY KEY (server_id, tag)
);

CREATE INDEX idx_server_tags_tag ON server_tags(tag);

CREATE TABLE note_revisions (
    id              TEXT PRIMARY KEY,
    target_type     TEXT NOT NULL CHECK (target_type IN ('server', 'group')),
    target_id       TEXT NOT NULL,
    content         TEXT NOT NULL,
    editor_type     TEXT NOT NULL CHECK (editor_type IN ('user', 'ai')),
    ai_provider     TEXT,
    ai_model        TEXT,
    created_at      TEXT NOT NULL
);

CREATE INDEX idx_note_revisions_target ON note_revisions(target_type, target_id);
```

Designentscheidungen dazu:
- **`auth_method` als JSON-Spalte statt normalisierter Tabellen.** Das
  `AuthMethod`-Enum aus Spec 0003 hat unterschiedliche Felder pro Variante;
  eine JSON-Serialisierung (`serde_json`) ist hier pragmatischer als drei
  nullable Spalten-Sets. Enthält ausschließlich `CredentialRef`-Strings, nie
  Secrets (siehe Spec 0003, Abschnitt 4).
- **`server_tags` als eigene Tabelle statt JSON-Array-Spalte**, damit die
  Filter-Engine (Spec 0002) später effizient nach Tag filtern/joinen kann,
  ohne JSON parsen zu müssen.
- **Timestamps als TEXT (ISO 8601)** statt SQLite-nativer Integer-Zeitstempel,
  für bessere Lesbarkeit beim manuellen Debuggen der DB und verlustfreies
  Round-tripping mit `chrono::DateTime<Utc>`.
- **`ON DELETE CASCADE` für Gruppen-Kinder und Tags**, aber
  **`ON DELETE SET NULL` für `group_id`/`jump_host_id` auf Servern** — das
  Löschen einer Gruppe soll nicht versehentlich Server mitlöschen, nur die
  Zuordnung auflösen.

## 5. Implementierung des `ProfileStore`-Traits

```rust
pub struct SqliteProfileStore {
    pool: sqlx::SqlitePool,
}

impl SqliteProfileStore {
    pub async fn connect(db_path: &Path) -> Result<Self> {
        // Pool aufbauen, sqlx::migrate!() ausführen, PRAGMA foreign_keys=ON setzen
    }
}

#[async_trait]
impl ProfileStore for SqliteProfileStore {
    // Implementiert alle Methoden aus dem Trait in Spec 0003 Abschnitt 5.1
    // (Server/Gruppe abrufen, Gruppenkette von Wurzel bis Zielgruppe)
}
```

Der `ProfileStore`-Trait aus Spec 0003 muss dafür auf `async fn` umgestellt
werden (über die `async-trait`-Crate, damit er weiterhin als Trait-Objekt
nutzbar bleibt für Tests mit `InMemoryProfileStore`). Das ist ein kleiner
nachträglicher Eingriff in `core::profiles` — wird als eigener erster Schritt
im Implementierungs-Prompt behandelt, bevor die SQLite-Crate entsteht.

## 6. Testbarkeit

Tests laufen gegen eine **In-Memory-SQLite-DB** (`sqlite::memory:`), mit den
echten Migrationen angewendet — keine separate Test-Datenbank-Logik nötig,
dieselben Migrationsdateien wie in Produktion. Jeder Test bekommt eine frische
Pool-Instanz, kein geteilter State zwischen Tests.

Testfälle (Auszug):
- Gruppe anlegen, Server anlegen, wieder abrufen → Felder identisch
- Gruppenkette über 3 Ebenen korrekt von Wurzel bis Blatt zurückgegeben
- Server-Löschung entfernt zugehörige `server_tags`, aber nicht die Gruppe
- Gruppen-Löschung setzt `group_id` betroffener Server auf `NULL`, löscht sie
  nicht
- `note_revisions` werden beim Schreiben einer neuen Notiz-Version zusätzlich
  zum aktuellen `notes`-Feld persistiert (nicht ersetzt)
- Migrationen sind idempotent: zweimaliges Ausführen von `connect()` auf
  derselben DB-Datei bricht nicht

## 7. Entscheidung: keine Datei-Verschlüsselung im MVP

Die SQLite-Datei wird **nicht** zusätzlich verschlüsselt (kein SQLCipher).
Begründung: Sie enthält keine Secrets (die liegen im Keychain, siehe Spec
0003 Abschnitt 4), sondern Hostnames, Usernames, Ports, Gruppen-/Server-Namen
und Freitext-Notizen, die operative Details verraten können, aber keine
Zugangsdaten sind. Für dieses Risiko wird die OS-Festplattenverschlüsselung
(FileVault/BitLocker/LUKS) als ausreichend vorausgesetzt.

Diese Annahme muss dem Nutzer sichtbar gemacht werden — z. B. ein Hinweis
beim ersten App-Start oder in den Einstellungen, dass die lokale Datenbank
unverschlüsselt auf der Festplatte liegt und volle Festplattenverschlüsselung
empfohlen wird, falls diese nicht bereits aktiv ist.

Ein optionales SQLCipher-Feature mit Key aus dem OS-Keychain (transparent,
ohne Passwort-Eingabe) bleibt als spätere Ausbaustufe denkbar, etwa für
Nutzer, die zusätzlichen Schutz gegen gezieltes Kopieren der `.db`-Datei bei
entsperrtem Nutzerkonto wollen (z. B. durch Malware oder versehentliches
Cloud-Backup). Kein Bestandteil dieser Spec, keine offene Frage mehr, sondern
bewusst zurückgestellt.

**Ergänzung**: Mit der Einführung persistenter Chat-Sitzungen (Spec 0034)
wurde diese Bewertung für Chat-Inhalte verfeinert, nicht revidiert — statt
Full-Database-Verschlüsselung (technisch mit `sqlx` nicht ohne Weiteres
umsetzbar, siehe Spec 0036) wird gezielt nur der Konversationsinhalt
verschlüsselt. Die hier getroffene Einschätzung zu Metadaten bleibt
unverändert gültig.

## 8. Weitere offene Punkte

- Soll es einen Export/Import-Mechanismus geben (z. B. verschlüsseltes JSON),
  um Profile zwischen Rechnern zu übertragen, ohne Cloud-Sync zu bauen?
  Nicht Teil dieser Spec, aber relevant für die Roadmap.
