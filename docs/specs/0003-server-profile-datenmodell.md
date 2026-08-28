# Spec: Server-Profile & Credentials Datenmodell

Status: Entwurf
Modul: `crates/core/profiles` (Name vorläufig, ggf. Umbenennung zu `servers`)
Abhängigkeiten: keine direkte Code-Abhängigkeit, konzeptionell verzahnt mit
Spec 0002 (Filter-Engine nutzt `Scope`/Tags aus diesem Modell) und der
künftigen KI-Provider-Spec (nutzt `effective_notes()`, siehe Abschnitt 5)

## 1. Ziel

Datenmodell für Server-Verbindungsprofile, deren Organisation in Gruppen, die
zugehörigen Credentials (sicher referenziert, nie Klartext in der DB) sowie
ein neues Feature: **LLM-Kontextnotizen** pro Server und pro Gruppe, die sowohl
vom Nutzer als auch von der KI gepflegt werden können.

## 2. Gruppen

Gruppen sind hierarchisch (Baum), damit z. B. "Kunde A / Produktion" als
verschachtelte Struktur möglich ist. Fürs MVP ist die Tiefe nicht technisch
begrenzt, UI-seitig aber ggf. auf 2–3 Ebenen beschränkt, um die Übersicht
nicht zu verlieren.

```rust
pub struct GroupId(Uuid);

pub struct Group {
    pub id: GroupId,
    pub name: String,
    pub parent_id: Option<GroupId>,
    pub notes: String,           // aktueller LLM-Kontext, s. Abschnitt 5
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
```

Wichtig: **Gruppen sind ein eigenes Konzept, getrennt von den `Tag`-Scopes der
Filter-Engine** (Spec 0002). Gruppen dienen der Organisation und dem
Notizen-Kontext; Tags (z. B. `"production"`) dienen der Policy-Steuerung. Ein
Server kann in Gruppe "Kunde A" liegen und gleichzeitig den Tag `production`
tragen — beides unabhängig voneinander, aber ein Server erbt sinnvollerweise
optional die Tags seiner Gruppe als Default (siehe Abschnitt 6, offener Punkt).

## 3. Server-Profil

```rust
pub struct ServerId(Uuid);

pub struct Server {
    pub id: ServerId,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub group_id: Option<GroupId>,
    pub tags: Vec<String>,
    pub auth: AuthMethod,
    pub notes: String,                // aktueller LLM-Kontext, s. Abschnitt 5
    pub jump_host: Option<ServerId>,   // Bastion/Jump-Host-Verkettung
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub enum AuthMethod {
    Password { credential_ref: CredentialRef },
    PrivateKey { credential_ref: CredentialRef, passphrase_ref: Option<CredentialRef> },
    Agent,
    Certificate { cert_ref: CredentialRef, key_ref: CredentialRef },
}
```

## 4. Credential-Referenzen (keine Secrets in der DB)

```rust
pub struct CredentialRef(String); // opaker Schlüssel ins OS-Keychain

pub trait CredentialStore {
    fn get(&self, r: &CredentialRef) -> Result<SecretString>;
    fn set(&self, r: &CredentialRef, value: SecretString) -> Result<()>;
    fn delete(&self, r: &CredentialRef) -> Result<()>;
}
```

Die lokale DB (SQLite) enthält **ausschließlich** `CredentialRef`-Strings,
niemals Passwörter, private Keys oder Zertifikats-Inhalte. Die eigentlichen
Secrets liegen im OS-Keychain über die `keyring`-Crate (siehe Spec 0001,
Abschnitt 2). `SecretString` (z. B. via `secrecy`-Crate) verhindert
versehentliches Loggen/Debug-Printen von Secrets im Code.

## 5. LLM-Kontextnotizen — Konzept

Das ist die Ergänzung, um die es dir ging: Ein Freitextfeld pro Server und pro
Gruppe, das der KI als zusätzlicher Kontext mitgegeben wird — z. B. "PHP 8.2
und MySQL 8 sind unter `/opt/lamp` installiert, Config liegt in
`/opt/lamp/conf`". Zwei Anforderungen:

1. **Vom Nutzer editierbar** — trivial, normales Textfeld in den
   Server-/Gruppen-Einstellungen.
2. **Von der KI editierbar** — die KI kann nach einer Aktion (z. B. LAMP-
   Stack-Installation) selbstständig vorschlagen, die Notiz zu aktualisieren.

### 5.1 Vererbung: effektiver Kontext für eine Session

Wenn eine SSH-Session zu einem Server startet, wird der an die KI übergebene
Kontext aus der Gruppen-Kette **von der Wurzel bis zum Server** zusammengesetzt,
vom Allgemeinen zum Spezifischen — spätere (spezifischere) Einträge haben mehr
Gewicht/Aktualität:

```rust
pub fn effective_notes(
    server: &Server,
    groups: &dyn ProfileStore,
) -> String {
    // Kette: Root-Gruppe ... unmittelbare Gruppe -> Server
    // Formatierung z.B.:
    // ## Kontext: Kunde A
    // <notes der Gruppe "Kunde A">
    // ## Kontext: Produktion
    // <notes der Gruppe "Produktion">
    // ## Kontext: Server "web-01"
    // <notes des Servers>
}
```

Diese Funktion lebt in `core/profiles`, ist reine Logik und ohne DB-Zugriff
über den `ProfileStore`-Trait testbar (gleiches Muster wie `PolicyStore` in
Spec 0002).

### 5.2 KI-Schreibzugriff — eigener Aktionstyp, kein Shell-Kommando

Eine Notiz-Änderung durch die KI ist **kein** Shell-Kommando und läuft
deshalb **nicht** durch die Filter-Engine aus Spec 0002. Stattdessen ein
eigener Aktionstyp, der aber demselben Transparenzprinzip folgt: die KI
schlägt vor, der Nutzer sieht einen Diff, bestätigt oder verwirft.

```rust
pub enum AiAction {
    SuggestCommand { command: String },
    ProposeNoteUpdate {
        target: NoteTarget,       // Server(ServerId) oder Group(GroupId)
        new_content: String,       // vollständiger neuer Text, nicht nur Diff
    },
}
```

UI-Verhalten: `ProposeNoteUpdate` wird immer als Diff-Ansicht (alt/neu)
angezeigt, nie automatisch übernommen — unabhängig von Filter-Engine-Regeln,
da diese nur für Shell-Kommandos gelten. Das ist bewusst strenger als
`AutoExec`-fähige Kommandos, weil eine Notiz dauerhaft den Kontext künftiger
Sessions beeinflusst und stille Fehlinformationen sich sonst unbemerkt
festsetzen könnten.

### 5.3 Änderungs-Historie (Audit)

Da Notizen sowohl von Menschen als auch von der KI verändert werden, braucht
es Nachvollziehbarkeit — separat von den eigentlichen `notes`-Feldern, die
immer nur den aktuellen Stand halten:

```rust
pub struct NoteRevision {
    pub id: Uuid,
    pub target: NoteTarget,
    pub content: String,
    pub edited_by: NoteEditor,
    pub created_at: DateTime<Utc>,
}

pub enum NoteEditor {
    User,
    Ai { provider: String, model: String },
}
```

Jede Änderung (ob Nutzer oder KI, nach Bestätigung) erzeugt einen neuen
`NoteRevision`-Eintrag. Damit ist im UI jederzeit nachvollziehbar, wann und
durch wen sich der KI-Kontext für einen Server verändert hat — und ein
Rollback auf eine vorherige Revision ist möglich.

## 6. Offene Punkte

- Sollen Server automatisch die Tags ihrer Gruppe erben (für die
  Filter-Engine-Scopes aus Spec 0002), oder bleibt das komplett getrennt?
  Tendenz: opt-in Vererbung, aber nicht erzwungen, um keine überraschenden
  Policy-Effekte durch Gruppenzugehörigkeit zu erzeugen.
- Maximale Länge/Token-Budget für `effective_notes()` — bei tiefen
  Gruppenhierarchien mit langen Notizen muss ggf. gekürzt oder priorisiert
  werden, bevor es an die KI-API geht (Kosten- und Kontextlimit-Frage,
  gehört eng mit der KI-Provider-Spec zusammen).
- Sollen `ProposeNoteUpdate`-Vorschläge ebenfalls eine Art "Auto-Accept"-Option
  bekommen (analog zur Filter-Engine), oder bleibt das für immer manuell
  bestätigungspflichtig? Aktuelle Empfehlung: immer manuell, siehe 5.2.
