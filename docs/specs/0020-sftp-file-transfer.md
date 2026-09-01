# Spec: Dateitransfer (SFTP) — Dateibrowser und KI-Zugriff

Status: Entwurf
Modul: Trait-Definitionen in `crates/core/src/ssh/`, Implementierung in
`crates/ssh-transport`, Commands in `crates/app-tauri`, Dateibrowser in
`frontend/`
Abhängigkeiten: SSH-Modul (Spec 0005, offener Punkt "SFTP" wird hiermit
geschlossen), Filter-Engine (Spec 0002), KI-Aktionen (Spec 0003/0006),
Kernschleife (Spec 0007)

## 1. Ziel und Abgrenzung

Zwei Anwendungsfälle, die sich denselben Transport teilen, aber
unterschiedlich reguliert sind:

1. **Manueller Dateibrowser** — der Nutzer navigiert selbst durch das
   Remote-Dateisystem, lädt Dateien hoch/herunter. Das ist eine direkte
   Nutzeraktion, vergleichbar mit dem interaktiven PTY-Terminal (Spec 0005):
   **keine Filter-Engine-Prüfung**, der Nutzer tut es ja selbst und sieht,
   was er tut.
2. **KI-initiierter Dateizugriff** — die KI schlägt vor, eine Datei zu lesen
   oder zu schreiben. Das ist ein Vorschlag wie jeder andere und **muss**
   derselben Kontrolle unterliegen wie ein Shell-Kommando (Abschnitt 4).

## 2. Warum SFTP für die KI überhaupt sinnvoll ist

Die KI kann heute bereits Dateien über Shell-Kommandos schreiben
(`echo`/`cat <<EOF`), die durch die Filter-Engine laufen. SFTP ersetzt das
nicht aus Notwendigkeit, sondern weil es **besser kontrollierbar** ist:

- **Echter Diff statt Kommando-Text**: Bei einer Änderung an einer
  bestehenden Datei kann die App den alten Inhalt lesen und dem Nutzer
  einen Zeilen-Diff zeigen, statt nur ein `cat <<EOF`-Kommando mit dem neuen
  Volltext. Der Nutzer sieht damit *was sich ändert*, nicht *was geschrieben
  wird* — deutlich besser prüfbar, gerade bei Config-Dateien.
- **Keine Shell-Quoting-Fallstricke**: Sonderzeichen, Anführungszeichen,
  Backslashes und Variablen-artige Inhalte (`$foo`) in Konfigurationsdateien
  brauchen bei Heredocs sorgfältiges Escaping. Fehler dabei erzeugen still
  falsche Dateiinhalte — ein Risiko, das bei SFTP-Transfer strukturell
  entfällt.
- **Keine Längenlimits** durch Kommandozeilen-Beschränkungen.

**Wichtig**: SFTP darf die bestehende Kontrolle nicht schwächen. Ohne die
Regelung aus Abschnitt 4 wäre ein SFTP-Write eine stille Umgehung der
Filter-Engine — genau das, was die App verhindern soll.

## 3. Transport

**`russh-sftp`** als SFTP-Client-Implementierung über einen
Subsystem-Channel der bestehenden SSH-Verbindung (Spec 0005). Kein zweiter
Verbindungsaufbau, kein separater Auth-Vorgang, keine erneute
Host-Key-Prüfung — die SFTP-Session läuft als weiterer Channel über
dieselbe `SshTransport`-Verbindung wie Exec- und PTY-Modus, inklusive
bestehender Jump-Host-Kette.

Kein SCP-Protokoll: OpenSSH selbst nutzt seit Version 9 intern SFTP für
`scp`, und SCP kann keine Verzeichnisse auflisten — für einen Dateibrowser
also ungeeignet. Die *Benutzeroberfläche* verhält sich wie ein klassischer
Datei-Transfer-Client, der *Transport* ist durchgehend SFTP.

```rust
#[async_trait]
pub trait SftpSession: Send {
    async fn list_dir(&mut self, path: &str) -> Result<Vec<RemoteEntry>, SshError>;
    async fn read_file(&mut self, path: &str) -> Result<Vec<u8>, SshError>;
    async fn write_file(&mut self, path: &str, content: &[u8]) -> Result<(), SshError>;
    async fn stat(&mut self, path: &str) -> Result<RemoteEntry, SshError>;
    async fn remove(&mut self, path: &str) -> Result<(), SshError>;
    async fn rename(&mut self, from: &str, to: &str) -> Result<(), SshError>;
    async fn create_dir(&mut self, path: &str) -> Result<(), SshError>;
}

pub struct RemoteEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub permissions: u32,
    pub modified: Option<DateTime<Utc>>,
}
```

Erweiterung von `SshTransport` (Spec 0005, Abschnitt 4) um:

```rust
async fn open_sftp(&mut self) -> Result<Box<dyn SftpSession>, SshError>;
```

Die SFTP-Session wird pro Server-Session **lazy** geöffnet (erst beim ersten
Dateizugriff) und dann für die Dauer der Session offengehalten, statt für
jede Operation neu aufgebaut zu werden.

## 4. KI-Zugriff: neue Aktionen und deren Kontrolle

Ergänzung zu `AiAction` (Spec 0003, Abschnitt 5.2 / Spec 0012):

```rust
pub enum AiAction {
    SuggestCommand { command: String },
    ProposeNoteUpdate { target: NoteTargetSelector, new_content: String },
    GenerateDocument { title: String, content_markdown: String },
    ReadRemoteFile { path: String },
    WriteRemoteFile { path: String, content: String },
}
```

### 4.1 `ReadRemoteFile`

Wird wie ein lesendes Kommando behandelt und läuft durch die
**Filter-Engine**. Damit vorhandene Regelsysteme greifen, ohne ein zweites
paralleles Regelkonzept einzuführen, wird die Aktion für die Auswertung auf
ein äquivalentes Kommando abgebildet: `sftp-read <pfad>`. Nutzer können
damit ganz normale Regeln schreiben (z. B. Allow für
`sftp-read /etc/nginx/*`, Deny für `sftp-read /etc/shadow`), mit derselben
Syntax und derselben Präzedenz-Kette wie für Shell-Kommandos.

Der gelesene Inhalt läuft vor der Rückgabe an die KI durch den
`OutputRedactor` (Spec 0006, Abschnitt 5) — identisch zu
Kommando-Ausgaben. Dateien über einer konfigurierbaren Größe (Default:
256 KB) werden abgelehnt statt vollständig in den KI-Kontext geladen, mit
klarer Meldung an Nutzer und KI.

### 4.2 `WriteRemoteFile`

Sicherheitskritischster Teil dieser Spec. Ablauf:

1. Abbildung auf `sftp-write <pfad>` für die Filter-Engine-Auswertung
   (gleiche Logik wie oben). Eine `Deny`-Regel blockiert wie gewohnt.
2. **Auch bei einer `Allow`-Regel wird nie ohne Anzeige geschrieben.**
   Anders als bei Shell-Kommandos gibt es hier kein `AutoExec`: Ein
   Dateischreibvorgang wird immer angezeigt, mindestens als kompakte
   Zusammenfassung. Begründung: Ein Config-Overwrite ist schwerer
   rückgängig zu machen und schwerer zu überblicken als ein einzelnes
   Kommando; die Wildcard-Natur typischer Allow-Regeln (`sftp-write
   /etc/nginx/*`) würde sonst sehr weitreichende, unsichtbare Änderungen
   erlauben.
3. **Änderungs-Vorschau**: Vor der Bestätigung liest die App die Zieldatei
   (sofern vorhanden) selbst per SFTP und liefert deren aktuellen Inhalt als
   `previousFileContent`-Feld mit dem `chat-action-proposed`-Event ans
   Frontend — analog zu `previousNoteContent` aus Spec 0019, Abschnitt 3.
   Der Diff wird **nicht** im Backend berechnet, sondern von derselben
   zeilenbasierten Diff-Komponente im Frontend dargestellt, die schon für
   Notiz-Vorschläge existiert (Spec 0019, Abschnitt 4) — eine zweite
   Diff-Implementierung für denselben UI-Zweck wäre unnötige Doppelung.
   `previousFileContent` ist `null`, falls die Datei noch nicht existiert;
   das Frontend zeigt dann den vollen Inhalt als neue Datei ohne
   Diff-Hervorhebung.
   Ausnahme: Ist die Zieldatei nicht als Text dekodierbar (Binärdatei),
   wird `previousFileContent` ebenfalls `null` gesetzt und im Dialog
   stattdessen ein Hinweis samt alter/neuer Dateigröße angezeigt — ein
   Zeilen-Diff wäre dort sinnlos.
4. **Automatisches Backup**: Vor jedem Überschreiben einer existierenden
   Datei legt die App serverseitig eine Sicherungskopie an
   (`<pfad>.smartssh-backup-<zeitstempel>`), sodass eine versehentliche
   Änderung rückgängig gemacht werden kann. Der Backup-Pfad wird dem Nutzer
   im Bestätigungsdialog und im Chat-Ergebnis genannt.
5. Nach Bestätigung: Schreiben per SFTP, Ergebnis (Erfolg/Fehler,
   Backup-Pfad) geht als `chat-action-result` zurück in den KI-Kontext.

### 4.3 Privilegierte Schreibzugriffe (Zusammenspiel mit Spec 0018)

SFTP kennt kein `sudo` — die Session läuft mit den Rechten des
SSH-Login-Users. Ein `WriteRemoteFile` auf eine root-eigene Datei
(`/etc/nginx/nginx.conf`, `/etc/systemd/system/*.service`) scheitert daher
mit "permission denied", sofern man sich nicht ohnehin als root verbindet.
Das betrifft ausgerechnet den Hauptanwendungsfall, für den die
Diff-Anzeige aus Abschnitt 4.2 den größten Nutzen hätte.

Lösung, sofern für den Server ein Sudo-Passwort hinterlegt ist (Spec 0018,
Abschnitt 4) **oder** die Verbindung ohnehin als root läuft:

1. Der reguläre SFTP-Schreibversuch wird zuerst unternommen. Gelingt er,
   ist nichts weiter zu tun (Normalfall bei Dateien im Home-Verzeichnis
   oder bei root-Verbindungen).
2. Scheitert er an fehlenden Rechten, wird **nicht** stillschweigend
   eskaliert. Stattdessen erscheint im Bestätigungsdialog explizit, dass
   der Schreibvorgang erhöhte Rechte benötigt und mit dem hinterlegten
   Sudo-Passwort ausgeführt würde — dieselbe Transparenzregel wie in Spec
   0018, Abschnitt 7 für Shell-Kommandos.
3. Nach Bestätigung: Der Inhalt wird per SFTP in eine temporäre Datei im
   Home-Verzeichnis des Login-Users geschrieben, anschließend per
   `execute_with_stdin` (Spec 0018, Abschnitt 5) mit
   `sudo -S install -m <mode> <temp> <ziel>` an den Zielort verschoben und
   die temporäre Datei entfernt. `install` statt `mv`, weil es Rechte und
   Eigentümer des Ziels in einem Schritt korrekt setzt, statt sie vom
   Temp-File zu erben.
4. Das Backup aus Abschnitt 4.2, Punkt 4 wird in diesem Fall ebenfalls über
   den privilegierten Pfad angelegt (`sudo -S cp -p`), da die Zieldatei
   sonst nicht lesbar/kopierbar wäre.
5. Ist **kein** Sudo-Passwort hinterlegt, wird der Fehler unverändert als
   Fehlschlag an Nutzer und KI zurückgemeldet, mit dem Hinweis, dass für
   diesen Pfad erhöhte Rechte nötig sind — kein stiller Fallback auf einen
   anderen Mechanismus.

Auch der Lesevorgang (`ReadRemoteFile`, Abschnitt 4.1) kann an Rechten
scheitern; dort wird der Fehler schlicht zurückgemeldet, ohne
Sudo-Eskalation — ein Lesevorgang, der erhöhte Rechte braucht, kann von der
KI weiterhin als regulär geprüftes `sudo cat <pfad>`-Kommando vorgeschlagen
werden, das durch die normale Filter-Engine läuft.

### 4.4 Was die KI nicht darf

`remove`, `rename` und `create_dir` werden der KI **nicht** als Aktionen
angeboten. Wenn die KI etwas löschen oder verschieben will, muss sie es als
normales Shell-Kommando vorschlagen, das durch die reguläre Filter-Engine
inklusive Hard-Blacklist läuft (Spec 0002, Abschnitt 3.1). Begründung: Für
diese Operationen bietet SFTP keinen der Vorteile aus Abschnitt 2 (kein
Diff, kein Quoting-Problem), aber ein zusätzliches Umgehungsrisiko — es gibt
also keinen Grund, dafür einen zweiten Weg zu schaffen.

## 5. Manueller Dateibrowser

Tauri-Commands:

```
sftp_list(session_id, path) -> Vec<RemoteEntryDto>
sftp_download(session_id, remote_path) -> ()   // nativer Speichern-Dialog
sftp_upload(session_id, local_path, remote_path) -> ()
sftp_delete(session_id, path)
sftp_rename(session_id, from, to)
sftp_mkdir(session_id, path)
```

Verhalten:
- **Keine Filter-Engine-Prüfung** — direkte Nutzeraktionen, analog zum
  interaktiven Terminal (Spec 0005, Abschnitt 1).
- Down- und Uploads laufen über den **nativen Datei-Dialog** (Tauri
  Dialog-Plugin), konsistent mit Spec 0012: Es wird nie ohne expliziten
  Dialog auf die lokale Festplatte geschrieben oder von ihr gelesen.
- Löschen erfordert eine Bestätigungsrückfrage im UI (auch wenn es eine
  Nutzeraktion ist — versehentliches Löschen per Fehlklick soll nicht
  passieren).
- Fortschrittsanzeige bei Transfers größerer Dateien; Transfers laufen
  asynchron und blockieren die Session nicht.

### 5.1 UI

Der Dateibrowser wird als **umschaltbare Ansicht im rechten Panel**
platziert (dort, wo im Layout aus Spec 0007 das Terminal sitzt), mit einem
Umschalter "Terminal | Dateien". Begründung: Der rechte Bereich ist bereits
der "direkte Zugriff auf den Server"-Bereich, während links der KI-Chat
liegt — thematisch passend, und es entsteht kein zusätzliches drittes Panel,
das den ohnehin knappen Platz weiter aufteilt.

Inhalt: Pfadleiste mit Navigation, Dateiliste (Name, Größe, Rechte,
Änderungsdatum), Kontextmenü (Herunterladen, Umbenennen, Löschen), Upload
per Button oder Drag-and-Drop aus dem Betriebssystem.

## 6. Testbarkeit

- Unit-Tests gegen einen `MockSftpSession` für die Kontroll-Logik: Mapping
  von `ReadRemoteFile`/`WriteRemoteFile` auf `sftp-read`/`sftp-write` für die
  Filter-Engine, Redaction gelesener Inhalte, Größenlimit-Ablehnung,
  Backup-Pfad-Erzeugung, Diff-Berechnung.
- Integrationstests gegen den bereits vorhandenen in-process
  `russh`-Testserver (Spec 0005, Abschnitt 8), erweitert um ein
  SFTP-Subsystem: echter Upload/Download-Roundtrip, Verzeichnisauflistung,
  Rename/Delete.

## 7. Offene Punkte

- Symlinks: aktuell nicht gesondert behandelt (werden wie normale Einträge
  gelistet). Ob ein Schreibvorgang auf einen Symlink das Ziel oder den Link
  ersetzen soll, ist bewusst noch nicht entschieden — relevant, falls die KI
  Config-Dateien schreibt, die auf verlinkte Pfade zeigen
  (`/etc/nginx/sites-enabled/*` ist ein typischer Fall).
- Aufräumen alter `.smartssh-backup-*`-Dateien: Diese sammeln sich
  serverseitig an. Denkbar wäre eine Übersicht im UI ("von Smart SSH
  angelegte Backups auf diesem Server") mit Lösch-Funktion — nicht Teil
  dieser Spec, aber sollte nicht dauerhaft vergessen werden.
- Große Verzeichnisse (mehrere tausend Einträge): aktuell keine
  Paginierung/Virtualisierung vorgesehen; falls das in der Praxis stört,
  nachrüsten.
