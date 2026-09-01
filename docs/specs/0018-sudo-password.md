# Spec: Sudo-Passwort für privilegierte Kommandos

Status: Entwurf
Modul: Erweiterung `crates/ssh-transport`, `crates/core` (`SshTransport`-Trait),
`crates/app-tauri` (Credential-Handling, Kernschleife), `frontend/`
(Server-Formular, Bestätigungsdialog)
Abhängigkeiten: SSH-Transport-Modul (Spec 0005), Server-Datenmodell/
Credential-Store (Spec 0003), Kernschleife (Spec 0007, Abschnitt 6),
Sicherheits-Härtung (Spec 0013)

## 1. Problem

`SshTransport::execute()` öffnet einen reinen, nicht-interaktiven Exec-
Channel ohne Pseudo-Terminal und ohne Stdin-Zufuhr (s.
`crates/ssh-transport/src/transport.rs`). Ein von der KI vorgeschlagenes
`sudo <kommando>` scheitert dort zuverlässig: `sudo` braucht entweder ein
TTY für seinen Passwort-Prompt oder einen `askpass`-Helfer — beides fehlt.
Das interaktive Terminal (rechtes Panel) hat dagegen einen echten PTY-
Channel, über den der Nutzer selbst `sudo` interaktiv nutzen kann — aber KI
und Terminal teilen sich zwar dieselbe authentifizierte SSH-Verbindung
(dasselbe `SshTransport`), nicht aber denselben Channel/dieselbe Shell-
Zustand (kein gemeinsames "eingeloggtes" Sudo-Timestamp-Caching).

## 2. Ziel

Ein **optionales** Sudo-Passwort pro Server, das ausschließlich lokal im
`CredentialStore` (Spec 0003, wie das SSH-Login-Passwort) abgelegt wird.
Erkennt die Kernschleife vor der Ausführung ein führendes `sudo`/`doas` im
freigegebenen Kommando, wird das Passwort **einmalig, flüchtig** über den
Exec-Channel eingespeist (`sudo -S`, Passwort über Stdin) — nie auf dem
Zielserver abgelegt, nie in einer Umgebungsvariable, nie in einer Datei.

## 3. Nicht-Ziele / Abgrenzung

- **Kein** Zusammenlegen von KI-Exec-Channel und interaktivem Terminal-PTY
  (das wäre der größere, in Erwägung gezogene Umbau — verworfen: die
  Terminal-Ausgabe müsste dafür per Prompt-Erkennung aus einem rohen
  Bytestrom herausgeschnitten werden, deutlich fragiler als die heutige
  saubere stdout/stderr/exit-code-Erfassung des Exec-Channels).
- Erkannt wird nur ein Kommando, das **als Ganzes** mit `sudo `/`doas `
  beginnt (`^\s*(sudo|doas)(\s|$)`) — kein `sudo` mitten in einer
  Kommandokette (`foo && sudo bar`). Diese Fälle laufen weiterhin wie bisher
  (scheitern ohne Passwort-Zufuhr) — eine Erkennung an beliebiger Stelle
  einer Kette bräuchte dieselbe Segmentierungslogik wie die Filter-Engine
  (Spec 0002) und ist nicht Teil dieses Schritts.
- Kein Speichern des Sudo-Passworts für die Terminal-Session — der Nutzer
  tippt dort weiterhin selbst, falls gewünscht.

## 4. Speicherung

Wie das SSH-Login-Passwort ein deterministischer `CredentialRef` pro
Server, eigener Slot (`server:{id}:sudo_password`), analog zu
`crate::server_credentials::credential_ref` (Spec 0008, Abschnitt 4). Kein
neues Feld auf `Server`/keine Schema-Migration nötig: "ist ein Sudo-
Passwort hinterlegt" wird pro Aufruf per `CredentialStore::get(...).is_ok()`
ermittelt (ein lokaler, synchroner Keychain-Zugriff, kein Netzwerk — keine
spürbare Mehrkosten beim Laden der Serverliste).

Verhalten beim Speichern (Server-Formular, Spec 0008 Abschnitt 4-Konvention
"leer = unverändert"):
- Neues, nicht-leeres Feld → wird gesetzt/überschrieben.
- Leeres Feld → bestehender Wert (falls vorhanden) bleibt unverändert.
- Explizites Entfernen: eigener "Entfernen"-Button/Befehl
  (`clear_server_sudo_password(server_id)`), da "leer lassen" bereits
  "unverändert" bedeutet — sonst gäbe es keinen Weg, ein einmal gesetztes
  Sudo-Passwort wieder zu löschen, ohne das ganze Feld semantisch
  umzudeuten.
- `delete_server` löscht den Sudo-Passwort-Slot mit (best-effort, analog zu
  den Login-Auth-Secrets).

## 5. Ausführung

`SshTransport` (Spec 0005) bekommt eine neue Methode mit Default-
Implementierung (bestehende Implementierungen/Mocks in Tests bleiben
unverändert lauffähig):

```rust
async fn execute_with_stdin(
    &mut self,
    command: &str,
    stdin: &[u8],
) -> Result<CommandOutput, SshError> {
    self.execute(command).await  // Default: Stdin ignorieren
}
```

`RusshTransport` implementiert sie echt: öffnet den Exec-Channel wie
`execute()`, schreibt danach `stdin` über den Channel (`channel.data_bytes`)
und signalisiert EOF, bevor auf die Antwort gewartet wird.

In der Kernschleife (`crate::orchestration::execute_suggested_command`,
Spec 0007 Abschnitt 6) wird das **bereits durch Filter-Engine/Bestätigung
freigegebene** Kommando vor der Ausführung geprüft:

1. Beginnt es mit `sudo`/`doas` (Abschnitt 3) **und** ist für die Session
   ein Sudo-Passwort hinterlegt (bei `connect()` einmalig aus dem
   `CredentialStore` geladen, s. Abschnitt 6) → das Kommando wird auf
   `sudo -S ...`/`doas -S ...` umgeschrieben (nur falls nicht bereits ein
   `-S`/`-A`-Flag vorhanden — dann unverändert lassen, KI/Nutzer hat es
   selbst schon vorgesehen) und über `execute_with_stdin` mit dem Passwort
   (gefolgt von einem Zeilenumbruch) als Stdin ausgeführt.
2. Sonst: unverändertes Verhalten, `execute()` wie bisher.

Das tatsächlich ausgeführte, umgeschriebene Kommando (mit `-S`, ohne
Passwort) landet wie gewohnt in `chat-action-result`/im strukturierten Log
(Spec 0016) — das Passwort selbst erscheint an keiner dieser Stellen, da es
nie Teil eines Kommando-**Texts** ist, sondern ausschließlich über den
separaten Stdin-Kanal fließt.

## 6. Session-Zustand

`Session` (Spec 0007, Abschnitt 3) bekommt ein neues Feld
`sudo_password: Option<SecretString>`, einmalig bei `connect()` aus dem
`CredentialStore` gelesen (wie `ai_provider_label`/`ai_model`) — ein
fehlender Eintrag (`CredentialError::NotFound`) wird zu `None`, kein
harter Verbindungsfehler.

## 7. Transparenz im Bestätigungsdialog

Der bestehende Bestätigungsdialog zeigt weiterhin exakt das von der KI
vorgeschlagene Kommando (ohne `-S`, ohne Passwort-Andeutung im Kommandotext
selbst) — aber ergänzt um einen kurzen, deutlich sichtbaren Hinweis, wenn
Abschnitt 5, Punkt 1 zutreffen würde ("wird mit hinterlegtem Sudo-Passwort
ausgeführt"). Das gilt konsistent mit dem sonstigen Transparenzprinzip
dieses Projekts (Spec 0002/0007: auch automatisch ablaufende Details werden
angezeigt, nie stillschweigend gemacht) — ohne diesen Hinweis würde der
Nutzer nicht erkennen, dass im Hintergrund ein gespeichertes Secret
verwendet wird, auch wenn er dem Kommando selbst zustimmt.

Backend berechnet dieses Flag (`usesStoredSudoPassword: bool`) serverseitig
bei jedem `chat-action-proposed`/`chat-action-result` für
`SuggestCommand`-Aktionen (dieselbe Erkennung wie Abschnitt 5, Punkt 1) und
sendet es als Teil des Event-Payloads mit — das Frontend rät nicht selbst,
ob ein Passwort hinterlegt ist (das weiß nur das Backend).

## 8. Sicherheitsüberlegungen

- Das Passwort verlässt den Exec-Channel **nie** als Umgebungsvariable oder
  Datei auf dem Zielserver — beides wäre für ein späteres, von der KI
  vorgeschlagenes Kommando (`env`, `cat ...`) lesbar und würde direkt in den
  KI-Kontext zurückfließen (Spec 0013, Prompt-Injection-Überlegungen gelten
  hier analog). Ausschließlich Stdin-Zufuhr für genau einen `sudo -S`-
  Aufruf, danach ist der Wert aus dem Prozessspeicher des Zielsystems wieder
  verschwunden.
- `sudo -S` echot das Passwort nicht auf stdout/stderr — es taucht in
  `CommandOutput` nicht auf, muss also nicht zusätzlich vom
  `OutputRedactor` (Spec 0006, Abschnitt 5) behandelt werden.
- Das Passwort bleibt wie das SSH-Login-Passwort ausschließlich im lokalen
  `CredentialStore` (OS-Schlüsselbund) — keine neue Speicherklasse, keine
  Abweichung vom bestehenden Sicherheitsmodell (Spec 0013).

## 9. Offene Punkte

- Elevation mitten in einer Kommandokette (`cd /var/log && sudo tail -f
  app.log`) wird bewusst nicht unterstützt (Abschnitt 3) — falls das später
  gebraucht wird, bräuchte es dieselbe Segment-Erkennung wie die
  Filter-Engine, keine eigenständige Zweitimplementierung.
- Kein Ablaufdatum/keine erneute Bestätigung für ein einmal gespeichertes
  Sudo-Passwort — folgt damit demselben Modell wie das SSH-Login-Passwort
  selbst.
