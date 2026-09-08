# Changelog

Alle nennenswerten Änderungen an Smart SSH werden hier dokumentiert. Das
Format folgt [Keep a Changelog](https://keepachangelog.com/de/1.1.0/),
dieses Projekt hält sich an [Semantic Versioning](https://semver.org/lang/de/).

Funktionen, die nur in einer kostenpflichtigen Edition verfügbar sind,
sind mit **(Pro)** markiert.

## [Unreleased]

### Added
- Einstellungen neu strukturiert: Navigation links, Inhalt rechts (statt
  einer langen, ungegliederten Liste) — Kategorien für KI-Provider, Anzeige
  & Sprache, Diagnose, Sitzungen & Daten und MCP-Server.
- KI-Provider-Formular: sofortiger, rein lokaler Hinweis, falls ein
  eingegebener API-Key nicht zum erwarteten Format des gewählten Providers
  passt (nur ein Hinweis, blockiert nie das Speichern).
- KI-Provider-Formular: "Zugangsdaten testen"-Button prüft die gerade
  eingegebenen, noch nicht gespeicherten Zugangsdaten mit einem echten
  Mini-Request und zeigt, ob sie gültig sind, die Authentifizierung
  fehlschlägt, oder der Provider nicht erreichbar ist.

### Fixed
- Ein eingefügter API-Key, ein Server-Passwort/Sudo-Passwort oder eine
  Key-Passphrase mit einem angehängten Zeilenumbruch/Leerzeichen (z. B. von
  einem Copy-Paste unter Windows) wird jetzt beim Speichern getrimmt, statt
  die Authentifizierung mit "Credentials ungültig" scheitern zu lassen.
- Windows: die Titelleiste fällt bei einem Aktivierungsfehler jetzt sauber
  auf die volle native Titelleiste zurück (inkl. funktionierender
  Minimieren-/Maximieren-/Schließen-Controls und Fenster-Ziehen), statt in
  einem kaputten Zwischenzustand hängen zu bleiben.
- Ein vom KI-Provider zurückgemeldetes Rate-Limit (HTTP 429) — bislang der
  häufigste Grund, warum die KI mitten in einer Sitzung ohne jede Meldung
  aufhörte zu antworten — wird jetzt automatisch mit Backoff und
  `Retry-After`-Berücksichtigung wiederholt; die mehreren KI-Anfragen pro
  Nachricht (Hauptantwort, Risiko-Zweitmeinung, Einschleusungs-Check) werden
  zeitlich entzerrt statt als Burst abgeschickt. Scheitert es trotzdem, zeigt
  der Chat jetzt eine eigene, handlungsanleitende Meldung ("bitte kurz warten
  und erneut senden") statt der generischen "Provider-Konfiguration prüfen"-
  Meldung.

### Security
- Bei einem Fehler eines KI-Providers (falscher API-Key, Rate-Limit,
  Netzwerkfehler, Modell nicht gefunden) landet die Fehlerantwort des
  Providers jetzt redigiert im Log, ergänzend zur bestehenden
  UI-Meldung — erleichtert die Diagnose, ohne je ein Secret preiszugeben.

## [0.4.0] — 2026-09-07

Erste öffentliche Testversion.

### Added
- Word-Export als erstes Pro-Modul **(Pro)**.
- Lizenz-Eingabe-UI mit Live-Aktivierung **(Pro)**.

### Security
- Ressourcen-Caps gegen feindliche/fehlerhafte Server: ein Output-Cap, der
  vorher erst nach vollständigem Puffern griff, begrenzt jetzt bereits
  während des Streamings; ein expliziter Rekursions-Cap gegen
  verschachtelte Command-Substitution.
- Integrität der Fencing-Markierungen für nicht vertrauenswürdigen Inhalt
  im KI-Kontext abgesichert.

### Fixed
- Härtungsrunde für die Testphase: verwaiste Keychain-Einträge nach einem
  fehlgeschlagenen Server-Anlegen werden jetzt zuverlässig zurückgerollt,
  Fehlermeldungen (falscher API-Key, Host nicht erreichbar, Provider nicht
  gestartet, …) nennen jetzt den nächsten Schritt statt roher Technik, und
  ein Absturz beim Start landet jetzt garantiert in der Logdatei statt
  spurlos zu verschwinden.

## [0.3.0] — Initial Early Access

Erste zusammenhängende Version: SSH-Client mit KI-Copilot und
Filter-/Policy-Engine mit Bestätigungs-Workflow für jedes vorgeschlagene
Kommando, Server-/Gruppen-/Regel-/Notizverwaltung, MCP-Server-Anbindung,
persistente Chat-Sessions, Risiko-Indikatoren, Multi-Provider-KI-Anbindung
(Anthropic, OpenAI, Ollama, generisch OpenAI-kompatibel) mit Redaction
sensibler Inhalte, macOS-Build signiert und notarisiert.
