# Smart SSH 🚀

[![CI](https://github.com/brokoskokoli/smart_ssh/actions/workflows/community.yml/badge.svg)](https://github.com/brokoskokoli/smart_ssh/actions/workflows/community.yml)
[![Release](https://github.com/brokoskokoli/smart_ssh/actions/workflows/release.yml/badge.svg)](https://github.com/brokoskokoli/smart_ssh/actions/workflows/release.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.75%2B-orange.svg)](https://www.rust-lang.org/)
[![Tauri](https://img.shields.io/badge/Tauri-v2-24C8D8.svg)](https://tauri.app/)

**Smart SSH** ist ein moderner, plattformübergreifender Desktop-SSH-Client mit integriertem Server-Manager und sicherem KI-Kommando-Assistenten. Entwickelt mit **Rust** und **Tauri v2** für maximale Geschwindigkeit, minimale Ressourcennutzung und kompromisslose Sicherheit.

---

## ✨ Hauptfunktionen

### 🖥️ Server- & Gruppen-Management
- **Hierarchische Gruppen & Tags:** Organisiere Server in verschachtelten Gruppen mit Vererbung von Tags und Kontextnotizen.
- **Jump Hosts / Bastions:** Nahtlose Verkettung über Zwischen-Server (SSH-Bastion-Hosts).
- **Flexible Authentifizierung:** Unterstützung für Passwörter, Private Keys (verschlüsselt mit Passphrase), SSH-Agent (`SSH_AUTH_SOCK`) und OpenSSH-Zertifikate.
- **Sichere Secret-Verwaltung:** Zugangsdaten und API-Keys werden **ausschließlich im nativen OS-Schlüsselbund** gespeichert (macOS Keychain, Windows Credential Manager, Linux Secret Service).
- **Integrierte SQLite-Datenbank:** Speicherung von Serverprofilen, Tags, Notizhistorien und Sicherheitsregeln mit automatischen Migrationen.
- **Verbindungstest vorab:** Sofortige Validierung von Host, Port, Jump-Host und Zugangsdaten vor dem Speichern.

### 🤖 Sicherer KI-Assistent & Split-Screen UI
- **Dual-Panel-Layout:** Chat-Panel als primärer Arbeitskanal links, interaktives xterm.js-Terminal rechts für Beobachtung und manuelle Eingriffe.
- **Multi-Provider-Support:** Direkte Anbindung an **OpenAI** (GPT-4o, etc.), **Anthropic** (Claude 3.5 Sonnet, etc.), **Ollama** (lokale Modelle) sowie generische OpenAI-kompatible APIs.
- **Live-Streaming:** Echtes Server-Sent Events (SSE) Streaming für sofortiges Text- und Status-Feedback.
- **Automatisierte Folgerunden:** Die KI interpretiert ausgeführte Befehlsausgaben und schlägt bei mehrstufigen Diagnose- und Wartungsaufgaben selbstständig nächste Schritte vor.

### 🛡️ Filter- & Policy-Engine (Fail-Safe Defaults)
- **3-stufige Entscheidung:** Jedes vorgeschlagene Kommando wird strikt evaluiert:
  - `AutoExec`: Automatische Ausführung (nur bei expliziter Allow-Regel)
  - `Confirm`: Manuelle Bestätigung durch den Nutzer (mit Live-Kommando-Editor)
  - `Deny`: Harte Blockierung gefährlicher Befehle
- **Präzedenz:** `Hard-Blacklist > Deny > Confirm > Allow > Default (Confirm)`.
- **Evasionsschutz:** Parser zerlegt Operatoren (`&&`, `||`, `;`, `|`, `&`, `\n`, `\r`) und Command-Substitutionen (`$(...)`, Backticks, `<(...)`), um Filterumgehungen zuverlässig zu verhindern.
- **Prompt-Isolation:** Verhindert Indirect Prompt Injections (IPI) durch strukturierte Kapselung von Server-Outputs und Validierung von System-Metadaten.
- **Automatische Secret-Redaction:** Erkennt und maskiert Passwörter, Private Keys, Bearer-Tokens, GitHub-PATs und AWS-Keys im Terminal-Output, bevor Daten an die KI-API gesendet werden.

### 📝 Notizen, Revisionshistorie & Dokumenten-Export
- **Automatische Notiz-Vorschläge:** Vorschlag relevanter Betriebsinformationen (Pfade, Versionen, Configs) beim Trennen einer SSH-Session.
- **Revisionshistorie (Audit-Trail):** Vollständige Versionshistorie für Server- und Gruppennotizen mit 1-Klick-Rollback.
- **Quick Rule Creation:** Erstelle dauerhafte Filterregeln direkt aus dem Bestätigungsdialog heraus.
- **Dokumenten-Generator & Multi-Format-Export:** Erstelle Systemberichte, Runbooks und Protokolle und exportiere sie auf Knopfdruck nach **Markdown (`.md`)**, **HTML (`.html`)**, **PDF (`.pdf`)** und **Word (`.docx`)**.

---

## 🏛️ Architektur-Übersicht

Das Projekt ist als modularer Cargo-Workspace aufgebaut:

```
smart_ssh/
├── crates/
│   ├── core/                  # ssh-manager-core: Domain-Typen, Filter-Engine, Redactor
│   ├── ssh-transport/         # russh-Implementierung, Shell-PTY, Exec, HostKey-Check
│   ├── ai-providers/          # OpenAI, Anthropic, Ollama, SSE-Parser, Fallback-Modus
│   ├── credentials-keyring/   # OS-Keychain-Anbindung (keyring-Crate)
│   ├── persistence-sqlite/    # SQLite-Speicher für Profile, Notizen, Regeln & AI-Configs
│   └── app-shell/             # App-Wiring, Orchestrierung, Tauri-Commands (Bibliothek)
├── apps/
│   └── smart-ssh-community/   # Dünnes Tauri-v2-Binary: app_shell::run(Wiring::community())
│       └── frontend/          # React 19, TypeScript, Tailwind CSS, xterm.js
├── docs/
│   ├── adr/                   # Architecture Decision Records (0001–0019)
│   └── specs/                 # Feature-Spezifikationen (0001–0014)
└── .github/workflows/         # CI/CD & Multi-Platform Release Workflows
```

---

## 📦 Installation & Download

Fertige Installer und Binaries für alle Plattformen stehen unter [Releases](https://github.com/brokoskokoli/smart_ssh/releases) bereit:

- **macOS:** `.dmg`-Installer *(Universal Binary für Apple Silicon & Intel)*
- **Windows:** `.msi` / `.exe`-Installer *(x64)*
- **Linux:** `.AppImage` *(direkt startbar)* oder `.deb`-Paket

> **Hinweis für macOS-Nutzer:** Falls Gatekeeper beim ersten Start eine Warnung anzeigt, kann das Quarantäne-Attribut im Terminal entfernt werden:
> ```bash
> xattr -cr /Applications/Smart\ SSH.app
> ```

---

## 🛠️ Entwicklung & Bauen

### Voraussetzungen
- [Rust](https://www.rust-lang.org/tools/install) (Version 1.75+)
- [Node.js](https://nodejs.org/) (Version 20+) & `npm`
- Plattformspezifische Tauri-Abhängigkeiten (siehe [Tauri Prerequisites](https://v2.tauri.app/start/prerequisites/))

### Lokaler Entwicklungsstart

```bash
# 1. Repository klonen
git clone https://github.com/brokoskokoli/smart_ssh.git
cd smart_ssh

# 2. Frontend-Abhängigkeiten installieren
cd apps/smart-ssh-community/frontend
npm install
cd ../../..

# 3. Desktop-App im Dev-Modus starten
cargo tauri dev --manifest-path apps/smart-ssh-community/Cargo.toml
# oder aus apps/smart-ssh-community/frontend:
npm run tauri dev
```

### Tests & Linting

```bash
# Alle Unit- und Integrationstests im gesamten Workspace ausführen
cargo test --workspace

# Code-Formatierung prüfen
cargo fmt --all --check

# Clippy-Lints prüfen
cargo clippy --workspace --all-targets -- -D warnings
```

---

## 📚 Dokumentation

- [Entwickler- & Release-Guide](README_DEV.md) – Versionsverwaltung, Release-Workflow und GitHub Secrets
- [Feature-Spezifikationen](docs/specs/README.md) – Detaillierte Spezifikationen (Specs 0001–0014)
- [Architecture Decision Records (ADRs)](docs/adr/README.md) – Dokumentation aller Architekturentscheidungen (ADR 0001–0019)

---

## 📄 Lizenz

Dieses Projekt ist unter der **Functional Source License, Version 1.1 (FSL-1.1-MIT)** lizenziert (mit automatischem Übergang zur MIT-Lizenz nach zwei Jahren) – siehe [LICENSE.md](LICENSE.md) für Details.
