# Spec: Server-/Gruppen-Verwaltung (UI)

Status: Entwurf
Modul: `crates/app-tauri` (neue Commands) + `frontend/`
Abhängigkeiten: `ssh-manager-core`/`persistence-sqlite` (Spec 0003/0004),
baut auf dem MVP-Screen aus Spec 0007 auf (das dort bewusst fehlende
Server-Anlegen wird hier nachgezogen)

## 1. Ziel

Server und Gruppen lassen sich vollständig im UI anlegen, bearbeiten und
löschen, statt wie bisher über `profiles_demo`/CLI. Zusätzlich: Notizen
(Spec 0003, Abschnitt 5) bekommen einen echten Editor samt Änderungshistorie,
und der Nutzer kann sich anzeigen lassen, was die KI als Kontext tatsächlich
zu sehen bekommt.

## 2. Scope-Abgrenzung

Bewusst **nicht** Teil dieser Spec, um sie überschaubar zu halten:
- ~~Drag-and-Drop-Reorganisation der Gruppenhierarchie~~ — nicht mehr
  ausgeschlossen: Spec 0103 (Issue #48) ergänzt Drag-and-drop für Server
  und Gruppen in beiden Bäumen. Die Gruppen-Auswahl per Dropdown im
  Bearbeiten-Formular bleibt als tastaturbedienbarer Weg bestehen.
- Mehrere parallele Server-Tabs/Sessions (eigene spätere Spec)
- Tag-basierte Filter-Engine-UI (Regeln verwalten) — eigene spätere Spec,
  auch wenn Tags hier schon am Server gesetzt werden können (nur als
  Freitext-Chips, keine Regel-Zuordnung)

## 3. Tauri-Commands: Gruppen

```
list_groups() -> Vec<GroupDto>              // flach, parent_id im DTO, Baum wird im Frontend gebaut
create_group(name: String, parent_id: Option<GroupId>) -> GroupId
update_group(id: GroupId, name: String, parent_id: Option<GroupId>)
delete_group(id: GroupId, confirm_cascade: bool) -> DeleteGroupResult
```

`delete_group` gibt vorab (bei `confirm_cascade: false`) eine Vorschau
zurück, was die Löschung tatsächlich bedeutet — passend zum
`CASCADE`/`SET NULL`-Verhalten aus Spec 0004, Abschnitt 4:

```rust
pub struct DeleteGroupResult {
    pub child_groups_to_delete: Vec<GroupDto>,   // werden mitgelöscht (CASCADE)
    pub servers_to_unassign: Vec<ServerDto>,     // verlieren nur die Gruppenzuordnung (SET NULL)
    pub executed: bool, // false = nur Vorschau, true = tatsächlich gelöscht
}
```

Erst ein zweiter Aufruf mit `confirm_cascade: true` löscht tatsächlich. Das
verhindert, dass ein Nutzer versehentlich mehrere Untergruppen mitlöscht,
ohne das vorher explizit gesehen zu haben.

## 4. Tauri-Commands: Server

```
list_servers(group_id: Option<GroupId>) -> Vec<ServerDto>
get_server(id: ServerId) -> ServerDto
create_server(input: ServerInput) -> ServerId
update_server(id: ServerId, input: ServerInput)
delete_server(id: ServerId)
test_connection(input: ServerInput) -> TestConnectionResult
```

```rust
pub struct ServerInput {
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub group_id: Option<GroupId>,
    pub tags: Vec<String>,
    pub auth: AuthMethodInput,
    pub jump_host: Option<ServerId>,
}

pub enum AuthMethodInput {
    Password { value: Option<String> },               // None bei update = unverändert lassen
    PrivateKey { key_content: Option<String>, passphrase: Option<String> },
    Agent,
    Certificate { cert_content: Option<String>, key_content: Option<String> },
}
```

Gleiche Konvention wie bei der AI-Provider-Verwaltung (Spec 0007, Abschnitt
8.2): Secret-Felder werden nur bei `create_server` zwingend benötigt; bei
`update_server` bedeutet `None`/leer "unverändert lassen", nicht "löschen".
Backend schreibt Secret-Inhalte über `CredentialStore::set()` **vor** dem
Schreiben der restlichen Felder in die DB — dieselbe Reihenfolge wie beim
Provider, aus demselben Grund (kein Zustand, in dem ein `CredentialRef` in
der DB steht, aber kein zugehöriges Secret existiert).

`ServerDto` (Rückgabe) enthält **keine** Secret-Felder, nur `AuthMethodKind`
(welche Methode aktiv ist) ohne Inhalt — analog zum Muster, dass der
API-Key nie ans Frontend zurückgeht.

## 5. Notiz-Verwaltung

```
update_group_notes(id: GroupId, content: String)
update_server_notes(id: ServerId, content: String)
list_note_revisions(target: NoteTarget) -> Vec<NoteRevisionDto>
rollback_note(target: NoteTarget, revision_id: Uuid)
preview_effective_notes(server_id: ServerId) -> String
```

- `update_*_notes` ruft intern `record_revision(target, content, NoteEditor::User)`
  auf (Spec 0003, Abschnitt 5.3) — jede manuelle Änderung landet in der
  Historie, genau wie KI-Änderungen.
- `rollback_note` überschreibt den aktuellen Stand **nicht still**, sondern
  erzeugt selbst eine neue Revision mit dem alten Inhalt (append-only
  Historie, kein Löschen/Umschreiben vergangener Einträge) — so bleibt
  nachvollziehbar, dass ein Rollback stattgefunden hat, statt dass es aussieht
  als wäre die Änderung nie passiert.
- `preview_effective_notes` ruft direkt `effective_notes()` (Spec 0003,
  Abschnitt 5.1) auf und gibt das Ergebnis unverändert zurück — der Nutzer
  sieht exakt den Text, den die KI beim nächsten Verbindungsaufbau zu diesem
  Server als Kontext bekommt, keine gekürzte oder umformatierte Version.

## 6. UI-Screens

- **Sidebar**: Baumdarstellung der Gruppen (rekursiv aus `list_groups()`
  clientseitig aufgebaut) mit Servern als Blätter. Klick auf Gruppe/Server
  öffnet das jeweilige Bearbeiten-Formular im Hauptbereich, nicht als
  separates Modal — passt besser zu häufigem Hin- und Herwechseln beim
  Einrichten mehrerer Server.
- **Gruppen-Formular**: Name, übergeordnete Gruppe (Dropdown, verhindert
  clientseitig die Auswahl der Gruppe selbst oder eigener Nachfahren als
  Parent — Zyklenvermeidung schon vor dem Absenden, nicht erst serverseitig
  entdeckt), Notiz-Editor (Abschnitt 5), Löschen-Button mit der
  Cascade-Vorschau aus Abschnitt 3.
- **Server-Formular**: alle Felder aus `ServerInput`, Auth-Methode als
  Auswahl mit dynamisch wechselnden Eingabefeldern (Passwort-Feld,
  Key-Datei-Auswahl via Tauri-Datei-Dialog + optionale Passphrase,
  "Agent verwenden" ohne weitere Eingabe, Zertifikat + Key als zwei
  Datei-Auswahlen), Tag-Eingabe als Chip-Input (komma-/Enter-getrennt),
  Notiz-Editor, "Kontext-Vorschau"-Button, der `preview_effective_notes`
  aufruft und das Ergebnis in einem Read-only-Textblock anzeigt.
- **Gespeicherte Host-Keys (Issue #248)**: Das Formular eines bestehenden,
  gespeicherten Servers zeigt einen schreibgeschützten Bereich "Host-Keys"
  mit einer Zeile je gespeichertem Schlüssel: Algorithmus (z. B.
  `ssh-ed25519`) und Fingerprint (`SHA256:…`, markier- und kopierbar), damit
  der Nutzer ihn mit einer zweiten Quelle abgleichen kann. Nachgeschlagen
  wird mit Host und Port des **gespeicherten** Servers, nicht mit
  ungespeicherten Formularwerten. Ist noch kein Schlüssel gespeichert, sagt
  der Bereich das ("Noch kein Host-Key gespeichert. Er wird bei der ersten
  Verbindung gespeichert."). Bei einem neuen, ungespeicherten Server und beim
  lokalen Pseudo-Server wird der Bereich nicht angezeigt. Ein Schlüssel, dessen
  Algorithmus sich nicht bestimmen lässt, erscheint mit der neutralen
  Bezeichnung `unknown`. Das Anzeigen verändert den Vertrauensstand nicht;
  Löschen oder Ersetzen eines Schlüssels gibt es hier nicht. Der rohe
  Schlüssel verlässt das Backend nie. Server mit gleichem Host und Port
  teilen sich die gespeicherten Schlüssel und zeigen dieselben Einträge.
- **Notiz-Historie**: chronologische Liste (`list_note_revisions`), jeder
  Eintrag zeigt Zeitpunkt, Editor (Nutzer, oder KI mit Provider/Modell-Name),
  und einen "Wiederherstellen"-Button pro vergangenem Eintrag (→
  `rollback_note`).

## 6a. Nicht nutzbare Server (Issue #100)

Ein gespeicherter Server, dessen Anmeldeart diese Version der App nicht
lesen kann, ist **nicht nutzbar**. Das passiert, wenn eine neuere Version
eine Anmeldeart gespeichert hat, die diese Version nicht kennt (z. B. nach
dem Zurückgehen auf eine ältere Version), oder wenn die gespeicherte
Anmeldeart beschädigt ist.

- **Die Serverliste lädt trotzdem.** Alle übrigen Server erscheinen und
  funktionieren wie gewohnt; ein einzelner nicht nutzbarer Server lässt
  weder die Liste noch den Start der App scheitern.
- **Anzeige:** In beiden Bäumen (Verbinden und Verwalten) steht der Server
  an seinem Platz in der Gruppe, deutlich als „nicht nutzbar" markiert, mit
  Name, Host und Grund:
  - unbekannte Anmeldeart: „Mit einer neueren Version der App gespeichert;
    diese Version kennt die Anmeldeart nicht."
  - beschädigte Anmeldeart: „Die gespeicherte Anmeldeart ist beschädigt
    und nicht lesbar."
- **Was nicht geht:** Verbinden, Bearbeiten, Verschieben per
  Drag-and-drop, Zugriff über MCP, Export in eine SSH-Konfiguration. Die
  Anmeldeart wird nie durch eine Ersatz-Anmeldeart (etwa „Agent")
  ersetzt — ein nicht nutzbarer Server wird nie wie ein nutzbarer
  behandelt. Wege, die einen nutzbaren Server brauchen (Verbinden, MCP,
  SSH-Konfigurations-Export, Diagnose-Export), lassen ihn aus und
  scheitern nicht an ihm.
- **Löschen:** Wählt man ihn in der Verwalten-Ansicht aus, zeigt der
  Hauptbereich den Grund und nur einen Löschen-Button, zweistufig wie beim
  normalen Server (Vorschau, dann „Endgültig löschen"). Gelöscht werden der
  Server samt Chat-Verlauf, Notizen und Tags sowie jedes Secret, das diese
  Version der App unter seiner ID speichern kann — welche er tatsächlich
  hat, ist mit unlesbarer Anmeldeart nicht bekannt. Secrets, die nur eine
  neuere Version für ihn angelegt hat, kennt diese Version nicht: Sie
  bleiben im Schlüsselbund bzw. in der Datenbank zurück und werden nicht
  als zurückgeblieben gemeldet; die Vorschau weist darauf hin. Secrets
  anderer Server werden dabei nie berührt. Lässt sich ein Secret nicht entfernen, läuft das
  Löschen trotzdem durch und nennt jeden zurückgebliebenen Eintrag, wie
  beim normalen Löschen. Andere Server, die ihn als Jump-Host nutzen,
  werden in der Vorschau genannt und verlieren diesen Verweis.
- **Zählt für Bestandsabgleiche, nie als Verbindungsziel.** Ein nicht
  nutzbarer Server zählt beim Import einer SSH-Konfiguration (Spec 0075,
  3.1.8) wie jeder vorhandene Server für die Konflikterkennung: gleicher
  Name oder gleiche Kombination aus Host, Port und Benutzer ergibt einen
  Konflikt in der Vorschau, standardmäßig wird übersprungen. Er wird dabei
  nie als `ProxyJump`-Ziel aufgelöst. Die Löschvorschau einer Gruppe nennt
  ihn — auch aus einer Untergruppe — als Server, der seine
  Gruppenzuordnung verliert (als „nicht nutzbar" gekennzeichnet); Server
  außerhalb der gelöschten Gruppe und ihrer Untergruppen stehen nicht dort.
  Weder Vorschau noch Import noch das Löschen der Gruppe ändern seine
  gespeicherte Anmeldeart.
- **Gespeicherte Daten bleiben unverändert.** Anzeigen, Auflisten und der
  einmalige Umzug der Secrets in die verschlüsselte Datenbank (Spec 0101)
  ändern die gespeicherte Anmeldeart nicht. Mit der neueren Version ist der
  Server danach wieder normal nutzbar. Der Secret-Umzug übernimmt jedes
  seiner Secrets, das diese Version kennt, statt es im Schlüsselbund
  zurückzulassen. Secrets, die nur eine neuere Version für ihn angelegt
  hat, übernimmt er nicht; sie bleiben im Schlüsselbund, auch nachdem der
  Umzug als abgeschlossen gilt.
- Andere beschädigte Felder eines Servers (ID, Zeitstempel, Port) sind
  davon nicht erfasst und lassen das Laden weiterhin scheitern.

## 7. Verbindungstest ("Verbindung testen"-Button)

`test_connection(input: ServerInput) -> TestConnectionResult` erlaubt, eine
Verbindung zu prüfen, **bevor** ein Server gespeichert wird — z. B. direkt
aus dem noch offenen Anlege-/Bearbeiten-Formular heraus.

```rust
pub enum TestConnectionResult {
    Success,
    AuthenticationFailed,
    HostKeyUnknown { fingerprint: String, key_type: Option<String> },
    HostKeyMismatch { expected_fingerprint: String, actual_fingerprint: String, key_type: Option<String> },
    NetworkError(String),
    Timeout,
}
```

Verhalten:
- Nimmt die Rohdaten aus dem Formular direkt entgegen (inkl. noch nicht
  gespeicherter Secrets) — **nichts wird dabei persistiert**, weder in der
  DB noch im `CredentialStore`. Bei `update_server`-artigen Aufrufen mit
  leerem Secret-Feld ("unverändert lassen") wird für den Test das bereits
  gespeicherte Credential des existierenden Servers herangezogen, sonst
  ließe sich ein bestehender Server gar nicht testen, ohne das Passwort
  erneut einzutippen.
- Führt **nur den SSH-Auth-Handshake** durch (`ssh-transport` verbindet und
  authentifiziert sich), aber **kein Kommando wird ausgeführt** — kein
  `execute()`-Aufruf, keine Filter-Engine-Beteiligung, da hier nichts auf
  dem Server passieren soll, nur die Erreichbarkeit/Zugangsdaten geprüft
  werden.
- Verbindung wird direkt danach wieder geschlossen, es entsteht kein Eintrag
  in `AppState.sessions`.
- Timeout von 10 Sekunden, danach `TestConnectionResult::Timeout` statt
  unbegrenztem Warten.
- Jump-Host-Kette: bereits gespeicherte Zwischen-Hops werden regulär über
  `ProfileStore` aufgelöst (Spec 0005, Abschnitt 5); nur der letzte Hop (der
  gerade bearbeitete Server) nutzt die frischen Formulardaten statt
  gespeicherter Credentials.
- **Host-Key-Prüfung wird bewusst wiederverwendet**: `HostKeyStore` ist nach
  Host/Port indiziert (Spec 0005, Abschnitt 6), nicht nach Server-ID. Ergibt
  der Test `HostKeyUnknown`, kann die UI direkt hier den
  Bestätigungsdialog aus Spec 0007 zeigen und bei Zustimmung `trust()`
  aufrufen — die spätere "echte" Verbindung über `connect()` muss dann
  nicht erneut bestätigt werden, da derselbe `HostKeyStore`-Eintrag greift.

UI-seitig: Ergebnis erscheint als kompakte Inline-Anzeige direkt im
Formular (grüner Haken bei `Success`, sonst kurze Fehlermeldung), kein
eigenes großes Modal — bei `HostKeyUnknown`/`HostKeyMismatch` erscheint der
bereits aus Spec 0005/0007 bekannte Bestätigungs- bzw. Warnungs-Dialog. Der Dialog zeigt
dabei wie bei `connect()` den Schlüsseltyp des angebotenen Schlüssels
(`key_type`; `null`, wenn er sich nicht bestimmen lässt, dann ohne diese
Zeile).

## 8. Offene Punkte

- Key-Datei-Auswahl liest den Dateiinhalt und speichert ihn im
  `CredentialStore` — der ursprüngliche Dateipfad wird **nicht**
  gespeichert. Falls der Nutzer den Key später extern rotiert, muss er ihn
  im UI erneut hochladen; es gibt keine "beobachtet Datei auf Änderungen"-
  Funktion. Das ist eine bewusste MVP-Vereinfachung, aber wert, es explizit
  festzuhalten, falls es später überrascht.
