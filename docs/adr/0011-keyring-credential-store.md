# 0011-keyring-credential-store

## Status
Accepted

## Kontext

`AppState` (Spec 0007, Abschnitt 3) braucht ein `credential_store: Arc<dyn
CredentialStore>`. Bislang existierte im gesamten Workspace nur eine
`InMemoryCredentialStore`-Testdouble (`core::profiles::tests`) — keine
konkrete, persistente Implementierung. Ohne eine echte Implementierung
würde jeder über `add_ai_provider` gespeicherte API-Key den Prozess nicht
überleben, was das erklärte Ziel von Spec 0007 Abschnitt 1 ("ohne
hinterlegten API-Key kein echter Test möglich") unterläuft — eine
In-Memory-Attrappe an dieser Stelle wäre keine Vereinfachung, sondern ein
kaputtes Feature.

`core::credentials`s Modul-Kommentar (aus der ursprünglichen
Workspace-Anlage) kündigt bereits an, dass eine OS-Keychain-gestützte
Implementierung über die `keyring`-Crate kommen wird, sobald sie
gebraucht wird — das ist jetzt der Fall.

Zwei Design-Fragen ergaben sich beim Umsetzen:

1. Wo lebt diese Implementierung — in `app-tauri` (das laut Spec 0007
   Abschnitt 3 "keine fachliche Logik" enthalten soll) oder in einer
   eigenen Crate?
2. Wie wird sie getestet — ein echter macOS-/Windows-/Secret-Service-
   Keychain-Zugriff setzt eine interaktive, entsperrte Sitzung voraus und
   kann bei einer unsignierten Dev-Build-Binary sogar einen einmaligen
   GUI-Bestätigungsdialog pro Rebuild auslösen.

## Entscheidung

**Eigene Crate `crates/credentials-keyring`**, die `CredentialStore` über
die `keyring`-Crate (v4, `v1`-Kompatibilitätsmodus:
`Entry::new`/`set_password`/`get_password`/`delete_credential`,
plattformübergreifend macOS Keychain Services/Windows Credential
Manager/*nix Secret Service) implementiert — konsistent mit dem
bisherigen Muster im Projekt: `persistence-sqlite` für `ProfileStore`,
`ssh-transport` für `SshTransport`, `ai-providers` für `AiProvider`. Ein
OS-Keychain-Wrapper ist zwar keine *fachliche* Logik, aber genau die
Sorte austauschbarer I/O-Baustein, die im gesamten Workspace bislang
immer eine eigene Crate bekommen hat; diese eine Implementierung als
Ausnahme direkt in `app-tauri` zu bauen, hätte das Muster ohne echten
Vorteil gebrochen.

**Kein automatisierter Test gegen den echten Keychain.** Analog zum
bereits mit `#[ignore]` markierten Zwei-Hop-Jump-Host-Test in
`ssh-transport` (ADR 0008) bleibt `KeyringCredentialStore` ungetestet
durch `cargo test` — nicht aus Bequemlichkeit, sondern weil ein
Headless-/CI-artiger Testlauf beim echten Keychain-Zugriff hängen bleiben
oder aus anderen Gründen fehlschlagen kann, die nichts mit der
Korrektheit des Codes zu tun haben. Verifikation passiert stattdessen
manuell beim `cargo tauri dev`-Smoke-Test (echten Provider anlegen,
Neustart, Provider ist weiterhin da).

## Konsequenzen

**Positiv:**
- `app-tauri` bleibt wie in Spec 0007 Abschnitt 3 vorgesehen ein dünner
  Wrapper — auch der Keychain-Zugriff ist eine ausgelagerte, für sich
  austauschbare Implementierung, kein direkt in der App-Schicht
  verdrahteter Plattform-Aufruf.
- API-Keys überleben tatsächlich einen Neustart der App — Voraussetzung
  für den in Spec 0007 Abschnitt 1 explizit genannten End-to-End-Test.
- Dieselbe Implementierung deckt macOS, Windows und *nix ab, ohne
  App-seitige Fallunterscheidung.

**Negativ / Trade-off:**
- Ungetestete Implementierung: ein Bug in `KeyringCredentialStore` (z. B.
  eine falsche Fehler-Zuordnung) fiele erst beim manuellen Smoke-Test
  auf, nicht bei `cargo test`.
- Unsignierte Dev-Builds können bei jedem Rebuild eine neue macOS-
  Keychain-ACL-Identität darstellen — im schlimmsten Fall ein
  wiederkehrender, einmaliger GUI-Bestätigungsdialog während der
  Entwicklung. Für signierte Release-Builds mit stabiler Code-Signatur
  tritt das nicht auf.
