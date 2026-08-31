# Spec: SSH-Verbindungsmodul

Status: Entwurf
Modul: Trait-Definitionen in `crates/core/src/ssh/`, konkrete Implementierung
in neuer Crate `crates/ssh-transport`
Abhängigkeiten: `ssh-manager-core` (nutzt `AuthMethod`/`CredentialStore` aus
Spec 0003 zur Auth-Auflösung)

## 1. Ziel

Aufbau und Verwaltung von SSH-Verbindungen, inklusive Jump-Host-Verkettung,
Host-Key-Verifikation und zwei Nutzungsarten:

- **Exec-Modus**: einzelnes Kommando ausführen, stdout/stderr/exit-code
  einsammeln — das ist der Modus, den die Filter-Engine (Spec 0002) und der
  KI-Workflow nutzen, weil dort jedes Kommando einzeln geprüft/bestätigt wird.
- **Interaktiver Modus**: PTY-Shell für das Terminal-Tab (xterm.js-Anbindung
  im Frontend) — freier Tastatur-Stream, keine Filter-Engine-Prüfung, weil
  hier der Nutzer direkt selbst tippt.

Beide Modi laufen über **dieselbe** offene Verbindung (SSH-Multiplexing über
Channels), nicht über separate Neuverbindungen pro Kommando.

## 2. Architektur-Entscheidung: Trait in `core`, Implementierung separat

Wie schon bei der Persistenz (Spec 0004) gilt dasselbe Prinzip: `core`
definiert nur die Traits (`SshTransport`, `HostKeyStore` u. a.), die konkrete
`russh`-basierte Umsetzung lebt in einer eigenen Crate `crates/ssh-transport`.
Begründung identisch — `core` bleibt schnell testbar über Mock-Implementierungen,
und ein späterer Wechsel der SSH-Bibliothek (z. B. falls `russh` an Grenzen
stößt) betrifft nicht den Rest der Codebasis.

## 3. Technologie

**`russh`** (reine Rust-Implementierung, async, `tokio`-basiert).

Begründung: kein Binding gegen System-`libssh`/OpenSSH nötig — das
vereinfacht plattformübergreifendes Bauen erheblich (kein Cross-Compile-Ärger
mit C-Abhängigkeiten auf Windows/macOS/Linux). Zusätzlicher Vorteil für
Tests: `russh` implementiert sowohl Client- als auch Server-Seite, wodurch
sich für Integrationstests ein echter SSH-Server **in-process** hochfahren
lässt, ganz ohne Docker oder externe Testinfrastruktur (siehe Abschnitt 7).

## 4. Kernabstraktionen

```rust
pub struct CommandOutput {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub exit_code: Option<i32>,
}

pub struct PtySize {
    pub cols: u16,
    pub rows: u16,
}

#[async_trait]
pub trait SshTransport: Send + Sync {
    async fn execute(&mut self, command: &str) -> Result<CommandOutput, SshError>;
    async fn open_shell(&mut self, size: PtySize) -> Result<Box<dyn InteractiveShell>, SshError>;
    async fn disconnect(&mut self) -> Result<(), SshError>;
}

#[async_trait]
pub trait InteractiveShell: Send {
    async fn write(&mut self, data: &[u8]) -> Result<(), SshError>;
    async fn read(&mut self) -> Result<Vec<u8>, SshError>; // blockiert bis Daten verfügbar oder EOF
    async fn resize(&mut self, size: PtySize) -> Result<(), SshError>;
}
```

Eine Verbindung entsteht über eine freie Funktion, nicht Teil des Traits
selbst (Verbindungsaufbau braucht Auth-Auflösung, siehe Abschnitt 6):

```rust
pub async fn connect(
    target: &ConnectionTarget,
    credentials: &dyn CredentialStore,
    host_keys: &dyn HostKeyStore,
) -> Result<Box<dyn SshTransport>, SshError>;
```

## 5. Jump-Host-Verkettung

`ConnectionTarget` wird aus einem `Server`-Profil (Spec 0003) rekursiv über
dessen `jump_host`-Feld aufgelöst, vom äußersten Jump-Host zum eigentlichen
Ziel:

```rust
pub struct ConnectionTarget {
    pub hops: Vec<Hop>, // erster Eintrag = erster Sprung, letzter = eigentliches Ziel
}

pub struct Hop {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth: AuthMethod,
}
```

Verbindungsaufbau: TCP-Verbindung zum ersten Hop, SSH-Handshake. Für jeden
weiteren Hop wird **kein neuer TCP-Socket** geöffnet, sondern ein
`direct-tcpip`-Channel über die bereits bestehende SSH-Verbindung zum
nächsten Hop aufgebaut, und darüber wiederum ein SSH-Handshake geführt
(Standard-Technik für SSH-Tunneling durch Bastions). Zirkelerkennung bei der
Auflösung der `jump_host`-Kette aus dem `ProfileStore` ist Pflicht (siehe
bereits etabliertes Muster aus `effective_notes()` in Spec 0003) — eine
zyklische Jump-Host-Kette darf nicht zu einer Endlosschleife führen, sondern
muss einen Fehler liefern.

## 6. Host-Key-Verifikation (Trust-on-First-Use)

Sicherheitsrelevanter Teil, analog im Transparenz-Prinzip zur Filter-Engine:
**kein automatisches Akzeptieren unbekannter oder geänderter Host-Keys.**

```rust
pub enum HostKeyDecision {
    Trusted,
    Unknown { fingerprint: String },
    Mismatch { expected_fingerprint: String, actual_fingerprint: String },
}

pub trait HostKeyStore: Send + Sync {
    fn check(&self, host: &str, port: u16, key: &[u8]) -> HostKeyDecision;
    fn trust(&self, host: &str, port: u16, key: &[u8]) -> Result<(), SshError>;
}
```

Verhalten:
- `Trusted` → Verbindung geht normal weiter.
- `Unknown` → Verbindungsaufbau pausiert, UI zeigt den Fingerprint zur
  expliziten Bestätigung (wie bei der ersten Verbindung zu einem neuen
  Server üblich). Erst nach Nutzer-Bestätigung wird `trust()` aufgerufen und
  der Verbindungsaufbau fortgesetzt.
- `Mismatch` → **harter Stopp, keine einfache Bestätigung wie bei `Unknown`.**
  Ein geänderter Host-Key ist ein möglicher Hinweis auf einen
  Man-in-the-Middle-Angriff. Die UI muss das prominent und unmissverständlich
  als Warnung darstellen (deutlich strenger gestaltet als ein normaler
  Bestätigungsdialog), bevor ein Nutzer den neuen Key explizit als korrekt
  markieren kann (z. B. weil der Server tatsächlich neu aufgesetzt wurde).

Die konkrete Speicherung bekannter Host-Keys (eigene Tabelle in
`persistence-sqlite`, oder klassische `known_hosts`-Datei) ist nicht Teil
dieser Spec, sondern folgt in einer eigenen kleinen Ergänzung zu Spec 0004,
sobald dieses Modul steht. Für dieses Modul zählt nur: `HostKeyStore` ist ein
Trait, sodass die Verbindungslogik unabhängig von der konkreten Speicherung
entwickelt und getestet werden kann.

## 7. Fehlerbehandlung

```rust
pub enum SshError {
    ConnectionFailed(String),
    AuthenticationFailed,
    HostKeyRejected,
    ChannelError(String),
    Timeout,
    JumpHostCycle,
    CredentialResolutionFailed(String),
}
```

## 8. Testbarkeit

Zwei Ebenen, bewusst getrennt:

- **Unit-Tests (Standard, laufen immer bei `cargo test`)**: reine Logik ohne
  echtes Netzwerk — Jump-Host-Ketten-Auflösung inkl. Zirkelerkennung,
  Host-Key-Entscheidungslogik (`Trusted`/`Unknown`/`Mismatch`) gegen einen
  In-Memory-`HostKeyStore`, Fehler-Mapping. Nutzt Mock-Implementierungen von
  `SshTransport`/`HostKeyStore`.
- **Integrationstests (separat markiert, z. B. eigenes Test-Target oder
  `#[ignore]` per Default)**: echter Verbindungsaufbau, Exec- und
  PTY-Modus, Jump-Host-Verkettung end-to-end — gegen einen **in-process
  `russh`-Server**, der als Test-Fixture in der Test-Crate selbst hochgefahren
  wird (kein externer Docker-Container nötig). Das hält den normalen
  `cargo test`-Lauf schnell, ermöglicht aber trotzdem echte Protokolltests
  ohne manuelle Infrastruktur.

## 9. Offene Punkte

- SFTP-Unterstützung (Datei-Up-/Download) ist nicht Teil dieser Spec — falls
  später gewünscht, eigene Spec.
- Reconnect-Verhalten bei Verbindungsabbruch mitten in einer Session (z. B.
  Netzwerkwechsel): aktuell nicht spezifiziert, vermutlich manueller
  Reconnect-Button im MVP statt automatischer Retry-Logik.
- Wiederverwendung einer offenen Verbindung über mehrere Server-Tabs/Sessions
  hinweg (Connection-Pooling) vs. eine Verbindung pro Tab — MVP-Annahme:
  eine Verbindung pro geöffnetem Server-Tab, kein Pooling, der Sache halber
  einfacher zu debuggen.
