# Spec: Lokaler Pseudo-Server ("Localhost")

Status: Entwurf
Modul: neue Implementierung in `crates/ssh-transport` (oder eigene Crate
`crates/local-transport`, siehe Abschnitt 2), Erweiterung `crates/app-tauri`,
`frontend/`
Abhängigkeiten: `SshTransport`-Trait (Spec 0005), SFTP-Trait (Spec 0020),
Filter-Engine (Spec 0002), Kernschleife (Spec 0007)

## 1. Ziel

Ein immer vorhandener, nicht löschbarer Eintrag "Localhost" in der
Server-Liste, über den die KI-Chat-Funktionalität (Vorschlag → Filter-Engine
→ Bestätigung → Ausführung, exakt wie bei einem echten Server) auf der
**eigenen Maschine** genutzt werden kann — ohne dass dafür ein lokaler
SSH-Server (`sshd`) laufen oder eingerichtet werden muss. Viele Nutzer haben
gerade auf macOS/Windows standardmäßig gar keinen aktiven SSH-Server —
das würde diese Funktion sonst für die meisten unbenutzbar machen.

## 2. Architektur-Entscheidung: lokale Prozessausführung statt SSH-zu-sich-selbst

`LocalTransport` implementiert `SshTransport`/`InteractiveShell` (Spec
0005, Abschnitt 4) **nicht** über das SSH-Protokoll, sondern über direkte
lokale Prozessausführung:

```rust
pub struct LocalTransport { /* kein Verbindungszustand nötig */ }

#[async_trait]
impl SshTransport for LocalTransport {
    async fn execute(&mut self, command: &str) -> Result<CommandOutput, SshError> {
        // Plattform-Shell aufrufen: `sh -c <command>` (Unix), `cmd /C <command>` (Windows)
    }
    async fn open_shell(&mut self, size: PtySize) -> Result<Box<dyn InteractiveShell>, SshError> {
        // lokales PTY über die `portable-pty`-Crate, Start der Standard-Shell
        // des Nutzers ($SHELL auf Unix, powershell/cmd auf Windows)
    }
    async fn disconnect(&mut self) -> Result<(), SshError> { Ok(()) }
}
```

Analog `LocalFileSession` als lokale Implementierung des SFTP-Traits (Spec
0020, Abschnitt 3), die direkt auf das lokale Dateisystem zugreift
(`std::fs`/`tokio::fs`) statt über ein SFTP-Subsystem.

**Der entscheidende architektonische Vorteil**: Weil beide Traits bereits
seit Spec 0005/0020 sauber von ihrer konkreten Implementierung getrennt
sind, braucht **keine** der umgebenden Komponenten (Filter-Engine,
KI-Anbindung, Kernschleife, Bestätigungsdialoge, Risiko-Indikatoren,
MCP-Server) irgendeine Änderung — sie funktionieren automatisch, weil sie
nur gegen den Trait programmiert sind, nicht gegen `russh` konkret.

Keine Credential-Store-Nutzung, kein Host-Key-Handling — beides ist für
lokale Ausführung bedeutungslos, `connect()` für den lokalen Pseudo-Server
ist praktisch sofort erfolgreich, kein Handshake nötig.

## 3. Identität und Persistenz

Kein neuer Eintrag in der `servers`-Tabelle (Spec 0004) — der lokale
Pseudo-Server wird zur Laufzeit **synthetisiert**, nicht aus der DB
geladen, mit einer fest reservierten, konstanten `ServerId` (z. B. die
Nil-UUID `00000000-0000-0000-0000-000000000000`). `list_servers()` fügt
diesen Eintrag immer als erstes Element hinzu, unabhängig von
Gruppen-/Filterparametern. Vorteile dieses Ansatzes: nicht löschbar (er
existiert schlicht nicht als DB-Zeile, die man löschen könnte), taucht
nicht versehentlich in einer Gruppe auf, keine Migration nötig.

`ServerDto` bekommt ein Feld `is_local: bool`. Für `is_local: true`
sind Host/Port/Username/Auth-Methode/Jump-Host im Bearbeiten-Formular
ausgeblendet oder deaktiviert (bedeutungslos) — nur Notizen und Tags bleiben
editierbar, da `effective_notes()` (Spec 0003) auch für den lokalen
Pseudo-Server sinnvoll ist (z. B. "Homebrew-Pakete unter `/opt/homebrew`").

## 4. Verhalten in der Kernschleife

Keine Sonderbehandlung — der lokale Pseudo-Server durchläuft dieselbe
`Session`-Struktur (Spec 0007, Abschnitt 3), dieselbe Filter-Engine-Prüfung
(Spec 0002), dieselben Bestätigungsdialoge, dieselben Risiko-Indikatoren
(Spec 0026). Bewusst **keine** automatische Lockerung der Filter-Engine für
lokale Kommandos — die eigene Maschine verdient nicht weniger Kontrolle als
ein entfernter Server, eher im Gegenteil (Zugriff auf eigene Dateien,
eigene Zugangsdaten in `~/.ssh`, `~/.aws` usw.).

## 5. UI

- Fest angepinnt **oberhalb** der Gruppenhierarchie (Spec 0033), niemals
  innerhalb eines Ordners einsortierbar — visuell klar getrennt (eigenes
  Icon, z. B. ein Computer- statt Server-Symbol, Label "Localhost").
- Öffnet wie jeder andere Server einen Tab (Spec 0017) — keine
  Sonderbehandlung im Multi-Tab-System nötig, da es sich für die restliche
  App wie eine ganz normale Session verhält.
- Kein Verbindungstest-Button (Spec 0008, Abschnitt 7) nötig/sinnvoll —
  "Verbindung" ist hier immer sofort erfolgreich.

## 6. Offene Punkte

- Der lokale Pseudo-Server kann **nicht** als Jump-Host für andere Server
  fungieren (ergibt konzeptionell keinen Sinn — er ist der Ausgangspunkt,
  keine Zwischenstation im SSH-Verbindungsgraphen). Explizit ausgeschlossen,
  nicht nur "vergessen".
- Plattform-Verhalten des lokalen PTY unter Windows (ConPTY) hängt vom
  aktuellen Support-Stand der `portable-pty`-Crate ab — zum
  Implementierungszeitpunkt prüfen, da sich das zwischen Crate-Versionen
  unterscheiden kann.
- Soll der Anzeigename ("Localhost") anpassbar sein? Aktuell fest, da es
  ohnehin nur einen solchen Eintrag gibt — falls gewünscht, leicht später
  nachrüstbar über das ohnehin editierbare Notiz-/Tag-Feld hinaus.
