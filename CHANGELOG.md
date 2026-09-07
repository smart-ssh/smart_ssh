# Changelog

Alle nennenswerten Änderungen an Smart SSH werden hier dokumentiert. Das
Format folgt [Keep a Changelog](https://keepachangelog.com/de/1.1.0/),
dieses Projekt hält sich an [Semantic Versioning](https://semver.org/lang/de/).

Funktionen, die nur in einer kostenpflichtigen Edition verfügbar sind,
sind mit **(Pro)** markiert.

## [Unreleased]

## [0.4.0] — 2026-09-07

Erste öffentliche Testversion.

### Added
- Word-Export als erstes Pro-Modul **(Pro)**.
- Lizenz-Eingabe-UI mit Live-Aktivierung **(Pro)**.

### Fixed
- Härtungsrunde für die Testphase: verwaiste Keychain-Einträge nach einem
  fehlgeschlagenen Server-Anlegen werden jetzt zuverlässig zurückgerollt,
  Fehlermeldungen (falscher API-Key, Host nicht erreichbar, Provider nicht
  gestartet, …) nennen jetzt den nächsten Schritt statt roher Technik, und
  ein Absturz beim Start landet jetzt garantiert in der Logdatei statt
  spurlos zu verschwinden.
- Ressourcen-Caps gegen feindliche/fehlerhafte Server: ein Output-Cap, der
  vorher erst nach vollständigem Puffern griff, begrenzt jetzt bereits
  während des Streamings; ein expliziter Rekursions-Cap gegen
  verschachtelte Command-Substitution.
- Integrität der Fencing-Markierungen für nicht vertrauenswürdigen Inhalt
  im KI-Kontext abgesichert.

## [0.3.0] — Initial Early Access

Erste zusammenhängende Version: SSH-Client mit KI-Copilot und
Filter-/Policy-Engine mit Bestätigungs-Workflow für jedes vorgeschlagene
Kommando, Server-/Gruppen-/Regel-/Notizverwaltung, MCP-Server-Anbindung,
persistente Chat-Sessions, Risiko-Indikatoren, Multi-Provider-KI-Anbindung
(Anthropic, OpenAI, Ollama, generisch OpenAI-kompatibel) mit Redaction
sensibler Inhalte, macOS-Build signiert und notarisiert.
