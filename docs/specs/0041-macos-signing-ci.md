# Spec: macOS-Signing & Notarisierung in der Official-CI

Status: Entwurf — **Vorbereitung**, nicht sofort umsetzbar
Modul: privates Repo, `.github/workflows/official.yml` (existiert noch nicht)
Abhängigkeiten: Repository-Trennung/App-Shell (0038, definiert Community vs.
Official), lokale macOS-Signierung (bereits eingerichtet und verifiziert),
Architektur-Brief D2/Abschnitt 8

> **Nummerierung**: nächste freie Nummer in deiner Reihe.
>
> **Warum jetzt schon**: Der lokale Signing-/Notarisierungs-Weg wurde gerade
> vollständig eingerichtet und verifiziert (Developer-ID-Signatur, Hardened
> Runtime, Entitlements, App Store Connect API-Key-Notarisierung). Diese Spec
> sichert dieses frische Wissen für die spätere CI-Integration, damit es nicht
> neu erarbeitet werden muss. Umgesetzt wird sie erst, **sobald das private
> Repo existiert** — vorher gibt es keine `official.yml`.

## 1. Nicht verhandelbare Trennung: nur privates Repo

macOS-Signing/Notarisierung passiert **ausschließlich** in der
`official.yml`-Pipeline des privaten Repos — **niemals** in der öffentlichen
`community.yml` (Spec 0038). Begründung (Architektur-Brief D2, Abschnitt 8):
Die Community Edition ist bewusst unsigniert; nur die Official Edition wird
signiert/notarisiert und über die Website verteilt. Die Apple-Signing-Secrets
dürfen niemals in einem öffentlich klonbaren Repo liegen — auch nicht als
GitHub-Secrets, da das die falsche Vertrauensgrenze wäre.

Der lokale Build bleibt davon unberührt: `cargo tauri build` überspringt
Signing sauber, wenn keine Credentials gesetzt sind (verifiziert:
`tauri-bundler` skippt statt zu scheitern) — die eingecheckte Config
produziert also weiterhin unsignierte Builds ohne gesetzte Variablen.

## 2. Von lokal zu CI — was sich ändert

Lokal liegen die Credentials in `~/build/smart-ssh-signing.env` plus die
`.p8`-Datei und das Zertifikat im Login-Keychain. Ein CI-Runner hat weder
Keychain noch Dateien — **alles muss über GitHub-Secrets kommen**:

| Lokal | CI (GitHub Secret) | Inhalt |
|---|---|---|
| Zertifikat im Login-Keychain | `APPLE_CERTIFICATE` | Developer-ID-Zertifikat **inkl. privatem Schlüssel**, als `.p12` exportiert und base64-kodiert |
| — (Keychain-entsperrt) | `APPLE_CERTIFICATE_PASSWORD` | Passwort des `.p12`-Exports |
| `APPLE_SIGNING_IDENTITY` (Fingerprint) | `APPLE_SIGNING_IDENTITY` | derselbe Fingerprint wie lokal |
| `.p8`-Datei auf Platte | `APPLE_API_KEY` (o. ä. Name je nach Action) | Inhalt der `.p8`, base64-kodiert — **nicht** ein Pfad, die Datei existiert im Runner nicht |
| `APPLE_API_KEY` (Key-ID) | `APPLE_API_KEY_ID` | Key-ID |
| `APPLE_API_ISSUER` | `APPLE_API_ISSUER` | Issuer-ID |
| — | `APPLE_TEAM_ID` | `AGRWTKQZ8C` |

**Namens-Vorbehalt**: Die exakten Variablennamen, die `tauri-action` bzw.
`cargo tauri build` in der aktuellen Version erwartet, sind zum
Umsetzungszeitpunkt gegen die dann aktuelle Doku zu prüfen — sie haben sich
zwischen Tauri-Versionen verschoben (`APPLE_API_KEY` als Pfad vs. Inhalt,
`APPLE_PASSWORD` vs. API-Key-Variante). Die Tabelle oben ist das Konzept,
nicht die garantierte Schreibweise.

## 3. Umsetzung über `tauri-action`

Die offizielle `tauri-action` (GitHub Action) kennt diese Variablen und
erledigt Keychain-Import, Signieren, Notarisieren und Stapeln in einem
Schritt — dieselbe Kette wie `cargo tauri build` lokal. Es werden **keine**
manuellen `codesign`/`notarytool`/`xcrun stapler`-Aufrufe von Hand
zusammengebaut. Die Action bekommt die Secrets als `env:` übergeben.

Muss auf einem **macOS-Runner** laufen (`runs-on: macos-latest`) — Apples
Signing-Tools existieren nur auf macOS. Die bestehende Matrix aus Spec 0001
hat das ohnehin; der Windows-/Linux-Signing-Teil (eigene Verfahren, siehe
Abschnitt 6) läuft auf den jeweiligen Runnern.

## 4. Sicherheitsanforderungen an die Pipeline

- Secrets werden **nie** geloggt (GitHub maskiert sie automatisch, aber kein
  `echo`/`set -x` in Schritten, die Secrets berühren).
- Das temporäre Keychain, in das das Zertifikat importiert wird, wird am
  Ende des Jobs wieder entfernt (die Action macht das i. d. R. selbst — im
  Zweifel explizit).
- Die base64-Blobs (`.p12`, `.p8`) werden nur zur Laufzeit dekodiert, in
  Runner-lokale Temp-Dateien, nie in den Workspace/das Artefakt geschrieben.
- Kein Secret landet in einem Build-Artefakt oder Release-Asset.

## 5. Auslöser und Artefakte

- Auslöser: Tag-Push (z. B. `v*`) oder manueller `workflow_dispatch` —
  **nicht** jeder Commit (Notarisierung dauert Minuten bis ~1 Stunde und
  verbraucht Apple-Kontingent).
- Ergebnis: notarisierte, gestapelte `.dmg`, an einen GitHub-Release
  angehängt bzw. für die Website-Verteilung bereitgestellt.
- Verifikationsschritt im Job nach dem Build: `spctl -a -vv` auf das Bundle,
  erwartet `accepted` / `source=Notarized Developer ID`. Schlägt das fehl,
  schlägt der Job fehl — ein unnotarisiertes Official-Artefakt darf nicht
  stillschweigend durchgehen.

## 6. Windows- und Linux-Signing (Platzhalter, eigene Specs)

Nicht Teil dieser Spec, aber der Vollständigkeit halber vermerkt, da
`official.yml` sie am selben Ort orchestriert:
- **Windows**: Azure Artifact Signing bzw. OV/EV-Code-Signing (private
  Keys müssen seit 2023 auf Hardware/HSM liegen; Microsofts Cloud-Signing
  ist CI-tauglich). Eigene Spec, sobald relevant.
- **Linux**: GPG-signierte Releases (eigener Schlüssel, keine
  Zertifizierungsstelle). Deutlich einfacher, eigene Spec.

## 7. Offene Punkte

- Exakte `tauri-action`-Version und Variablennamen zum Umsetzungszeitpunkt
  gegen die dann aktuelle Doku prüfen (Abschnitt 2, Namens-Vorbehalt).
- Reputationsbindung bei Windows-Signing (SmartScreen-Reputation ist an das
  konkrete Zertifikat gebunden; bei Zertifikatswechsel beginnt sie neu) —
  relevant erst für die Windows-Spec.
- Ob die Notarisierung bei jedem Release-Tag oder nur bei „echten"
  Releases (nicht Pre-Releases) läuft — Kontingent-/Zeitfrage.
