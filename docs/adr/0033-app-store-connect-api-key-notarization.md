# 0033-app-store-connect-api-key-notarization

## Status
Vorgeschlagen

## Kontext

Apple-Notarisierung braucht Anmeldedaten, mit denen `notarytool` (bzw.
hier: `tauri-bundler`/`tauri-macos-sign`, s.
`tauri_macos_sign::AppleNotarizationCredentials`) sich gegenüber Apples
Notarisierungsdienst authentifiziert. `tauri-bundler` unterstützt zwei
Varianten (`crates/app-tauri`-Abhängigkeit `tauri-bundler` 2.9.4,
`bundle/macos/sign.rs::notarize_auth`):

- **Variante B — Apple-ID + App-spezifisches Passwort**: `APPLE_ID`,
  `APPLE_PASSWORD` (ein über appleid.apple.com erzeugtes App-spezifisches
  Passwort, nicht das reguläre Account-Passwort), `APPLE_TEAM_ID`.
- **Variante A — App Store Connect API-Key**: `APPLE_API_KEY` (Key-ID),
  `APPLE_API_ISSUER` (Issuer-ID), `APPLE_API_KEY_PATH` (Pfad zur
  `.p8`-Datei).

Für die Signierung selbst (getrennt von der Notarisierung) ist in beiden
Fällen zusätzlich `APPLE_SIGNING_IDENTITY` (die Developer-ID-Application-
Zertifikats-Identität) nötig — das betrifft diese Entscheidung nicht,
beide Notarisierungs-Varianten brauchen sie gleichermaßen.

## Entscheidung

**Variante A — App Store Connect API-Key.**

Begründung:

- **CI-tauglich.** Ein API-Key ist eine reine Maschinen-Identität ohne an
  eine natürliche Person gebundenes Passwort — lässt sich als GitHub
  Actions Secret hinterlegen und in `.github/workflows/release.yml`
  verwenden, sobald Official-Edition-Releases automatisiert gebaut werden
  sollen, ohne dass dafür ein persönliches App-spezifisches Passwort
  (das an ein bestimmtes Apple-ID-Konto gebunden ist) in einer CI-Umgebung
  landen müsste.
- **Kein Passwort im Spiel.** Ein App-spezifisches Passwort ist zwar kein
  Konto-Hauptpasswort, bleibt aber ein Geheimnis, das bei einer
  Zwei-Faktor-Authentifizierungs-Änderung, einem Konto-Sicherheitsvorfall
  oder einer 2FA-Anforderungsänderung ungültig werden oder erneut manuell
  im Apple-ID-Account-Portal erzeugt werden muss. Der API-Key ist
  unabhängig vom persönlichen Apple-ID-Konto verwaltbar (eigene
  Rollen-/Berechtigungs-Verwaltung in App Store Connect), lässt sich
  gezielt widerrufen/rotieren, ohne die Apple-ID selbst anzufassen.
- **Kein 2FA-Interaktionsbedarf beim Build.** Variante B kann je nach
  Apple-Kontokonfiguration zusätzliche interaktive Bestätigungsschritte
  auslösen — für einen unbeaufsichtigten/automatisierten Build-Lauf
  (lokal wie in CI) ungeeignet. Der API-Key authentifiziert rein
  kryptografisch (privater Schlüssel + Key-/Issuer-ID), ohne jede
  interaktive Komponente.

Die konkreten Anmeldedaten (Key-ID, Issuer-ID, `.p8`-Datei) liegen
außerhalb des Repositories in `~/build/smart-ssh-signing.env` (s.
`README_DEV.md`, Abschnitt 6) — nur die Variable **Namen**, nie deren
Werte, sind irgendwo im Repo dokumentiert.

### Nebenentscheidung: Signing-Identity über Zertifikats-Fingerprint, nicht Name

`bundle.macOS.signingIdentity` in `tauri.conf.json` bleibt bewusst
**ungesetzt** — der Wert kommt ausschließlich über die
`APPLE_SIGNING_IDENTITY`-Umgebungsvariable (Fingerprint, nicht
Anzeigename: der Anzeigename "Developer ID Application: ..." existiert
im Keychain des Entwicklers doppelt und würde zu Apples "ambiguous
identity"-Fehler führen, den `codesign`/`security find-identity` bei
mehrdeutigen Namenstreffern wirft). `tauri-cli` (`interface/rust.rs`)
liest `APPLE_SIGNING_IDENTITY` **immer bevorzugt** vor einem
`tauri.conf.json`-Wert — ein leerer/fehlender Wert in der eingecheckten
Konfiguration blockiert also keinen signierten Build, verhindert aber,
dass ein Community-/CI-Build ohne gesetzte Variable versehentlich einen
"specified identity not found"-Fehler bekommt (das Zertifikat existiert
nur auf der Maschine des Official-Edition-Erstellers). S. auch
`docs/adr/0022-stable-dev-code-signature.md` für dieselbe Grundregel
("nichts Signing-Bezogenes hartcodiert in `tauri.conf.json`, das nur auf
einer bestimmten Maschine funktioniert") — dort für die Dev-Signatur,
hier für die Release-Signatur, mit demselben Prinzip.

## Konsequenzen

**Positiv:**
- Der Notarisierungs-Schritt lässt sich später ohne Änderung an der
  gewählten Authentifizierungs-Variante nach CI verlagern.
- Zugriffsrechte des API-Keys lassen sich in App Store Connect granular
  verwalten/widerrufen, unabhängig vom persönlichen Apple-Konto.
- Kein Bedarf, ein App-spezifisches Passwort zu erzeugen/zu rotieren,
  wenn sich die 2FA-Konfiguration des Apple-Kontos ändert.

**Negativ / Trade-off:**
- Einmaliger zusätzlicher Einrichtungsschritt gegenüber Variante B: ein
  API-Key muss in App Store Connect (Nutzer & Zugriff -> Integrationen)
  explizit mit der Rolle "Developer" (oder höher) erzeugt und die
  `.p8`-Datei sicher abgelegt werden — die Erzeugung eines
  App-spezifischen Passworts ist ein kleinerer Schritt im
  Apple-ID-Portal.
- Die `.p8`-Datei ist, anders als ein Passwort, eine Datei, die sicher
  aufbewahrt/gesichert werden muss (verloren = neuer Key nötig, Apple
  gibt den privaten Schlüssel nach der ersten Erzeugung nicht erneut
  heraus) — dokumentiert in `README_DEV.md`, Abschnitt 6 ("niemals
  committen").
