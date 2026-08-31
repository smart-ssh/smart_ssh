# 0007-connect-outcome-and-arc-host-keys

## Status
Accepted

## Kontext

`docs/specs/0005-ssh-module.md`, Abschnitt 4, skizziert die Signatur der
Verbindungsfunktion so:

```rust
pub async fn connect(
    target: &ConnectionTarget,
    credentials: &dyn CredentialStore,
    host_keys: &dyn HostKeyStore,
) -> Result<Box<dyn SshTransport>, SshError>;
```

Zwei Stellen dieser Skizze mussten beim Implementieren in
`crates/ssh-transport/src/connect.rs` angepasst werden:

**1. Rückgabetyp.** Abschnitt 6 der Spec verlangt ausdrücklich, dass ein
`Unknown`- oder `Mismatch`-Host-Key **nicht** automatisch fortgesetzt,
sondern "eine Rückmeldung an den Aufrufer geliefert" wird, "die die
UI-Schicht später für den Bestätigungsdialog nutzen kann" — explizit
**nicht** stillschweigend im Fehlerfall versteckt. Mit der Skizzen-Signatur
(`Result<Box<dyn SshTransport>, SshError>`) gäbe es dafür nur zwei
Möglichkeiten: entweder man erfindet eine `SshError`-Variante für
"wartet auf Bestätigung" (kein echter Fehler, würde aber wie einer
behandelt), oder man verwirft den Host-Key-Kontext (Fingerprint,
Rohschlüssel) komplett und zwingt den Aufrufer, den Verbindungsversuch ohne
diese Information zu wiederholen.

**2. Typ von `host_keys`.** `russh::client::Handler` (der Trait, dessen
`check_server_key`-Callback die Host-Key-Prüfung überhaupt erst ermöglicht)
verlangt `Self: Send + 'static` sowie — indirekt, weil `client::connect`/
`client::connect_stream` den Handler in einen `tokio::spawn`-Task
verschieben — dass der Handler keine geliehenen Referenzen mit
aufrufer-gebundener Lebensdauer halten kann. Der `ClientHandler`
(`crates/ssh-transport/src/handler.rs`) muss aber genau während dieses
Callbacks auf den `HostKeyStore` zugreifen können.

## Entscheidung

**Rückgabetyp:** `connect()` liefert `Result<ConnectOutcome, SshError>` mit

```rust
pub enum ConnectOutcome {
    Connected(Box<dyn SshTransport>),
    PendingHostKeyConfirmation {
        host: String,
        port: u16,
        raw_key: Vec<u8>,
        decision: HostKeyDecision,
    },
}
```

`SshError` bleibt für echte Fehler reserviert (Verbindung fehlgeschlagen,
Auth fehlgeschlagen, ...); "wartet auf Bestätigung" ist kein Fehler, sondern
ein regulärer, erwarteter Zwischenzustand und bekommt deshalb eine eigene
`Ok`-Variante mit allem, was die UI für den Bestätigungsdialog braucht
(Host, Port, Rohschlüssel für einen erneuten `trust()`-Aufruf, die
`HostKeyDecision` für die Unterscheidung Unknown/Mismatch aus Abschnitt 6).

**`host_keys`-Typ:** `Arc<dyn HostKeyStore>` statt `&dyn HostKeyStore`. Der
`ClientHandler` hält seinen eigenen `Arc`-Klon, erfüllt damit `'static`
ohne Umweg. Intern wird bei jedem Hop (auch bei Jump-Host-Ketten, wo pro
Hop ein neuer `ClientHandler` entsteht) `host_keys.clone()` (eine reine
Arc-Referenzzählung, kein Deep-Copy) weitergereicht.

`credentials: &dyn CredentialStore` bleibt dagegen unverändert eine reine
Referenz: Credential-Auflösung (`crate::auth::authenticate`) passiert
synchron direkt im `connect()`-Aufruf-Stack, nie innerhalb eines
`russh`-Callbacks, das dem `'static`-Zwang unterläge.

## Konsequenzen

**Positiv:**
- Die UI-Schicht kann `Connected`/`PendingHostKeyConfirmation` sauber
  unterscheiden (z. B. via `match`), ohne `SshError`-Varianten auf
  Nicht-Fehler-Zustände zu missbrauchen.
- Alle für den Bestätigungsdialog nötigen Daten (Fingerprint via
  `decision`, Rohschlüssel für `trust()`) sind in einer Nachricht
  gebündelt, kein zweiter Roundtrip nötig, um sie zu beschaffen.
- `Arc<dyn HostKeyStore>` ist für Aufrufer meist ohnehin die natürliche
  Form (ein `HostKeyStore` ist typischerweise eine Singleton-artige
  Abstraktion über die gesamte App-Laufzeit, kein Wert mit kurzer
  Lebensdauer).

**Negativ / Trade-off:**
- Weicht von der Spec-Signatur (Abschnitt 4) ab — wer nur die Spec liest,
  erwartet einen einfachen `Result<Box<dyn SshTransport>, SshError>` und
  `&dyn HostKeyStore`.
- "Fortsetzen nach `trust()`" bedeutet einen **erneuten** `connect()`-Aufruf
  (frischer TCP-/Tunnel-Aufbau), keine buchstäbliche Fortsetzung einer
  pausierten Verbindung — s. auch die Diskussion dazu in
  `docs/adr/0008-russh-nested-tunnel-limitation.md`. `russh`s
  `check_server_key`-Callback kennt nur `Result<bool, Self::Error>`; es gibt
  keinen Mechanismus, einen bereits begonnenen Handshake zu "pausieren" und
  später mit demselben Zustand fortzusetzen. Für die UI ist das
  transparent (ein Klick auf "Vertrauen" löst intern einfach einen neuen
  Verbindungsversuch aus), aber bei sehr langsamen/instabilen Netzwerken
  könnte der erneute vollständige Handshake spürbar sein.
