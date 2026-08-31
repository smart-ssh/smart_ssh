# Smart SSH

Cross-platform SSH-Client mit Server-Manager und KI-gestützter
Kommandoausführung. Verwaltet mehrere SSH-Server/Zugänge, erlaubt gefiltertes
Auffinden von Servern und übersetzt natürlichsprachige Anfragen mithilfe eines
LLM in ausführbare Shell-Befehle – inklusive Audit-Trail.

> Status: Frühes Grundgerüst. Es existiert noch keine Geschäftslogik, nur die
> Projektstruktur.

## Architektur-Übersicht

Das Projekt ist als Cargo-Workspace mit zwei Crates organisiert:

```
smart_ssh/
├── crates/
│   ├── core/         # ssh-manager-core – reine Geschäftslogik, UI-unabhängig
│   └── app-tauri/     # ssh-manager-app-tauri – App-/UI-Schicht (Tauri)
├── docs/
│   ├── adr/           # Architecture Decision Records
│   └── specs/         # Feature-Spezifikationen
└── .github/workflows/  # CI
```

### `crates/core`

Enthält die gesamte Geschäftslogik und ist bewusst **frei von jeder
UI-Abhängigkeit** (insbesondere keine Tauri-Dependency), damit sie
unabhängig von der gewählten UI-Technologie test- und wiederverwendbar
bleibt (z. B. auch für eine CLI oder TUI). Geplante Module:

- `ssh` – SSH-Verbindungsmanagement, Kommandoausführung
- `credentials` – Verwaltung von Zugangsdaten/Keys
- `filter` – Suche/Filterung der Serverliste
- `ai` – KI-gestützte Übersetzung natürliche Sprache → Kommando
- `audit` – Protokollierung ausgeführter Kommandos

### `crates/app-tauri`

App-/UI-Schicht auf Basis von [Tauri](https://tauri.app/) (noch nicht
eingerichtet). Hängt von `core` ab und bindet dessen Funktionalität an die
UI an – enthält selbst keine Geschäftslogik.

## Bauen & Testen

Voraussetzung: [Rust](https://www.rust-lang.org/tools/install) (stable).

```bash
# Gesamten Workspace bauen
cargo build --workspace

# Tests ausführen
cargo test --workspace

# Formatierung prüfen
cargo fmt --all --check

# Linting
cargo clippy --workspace --all-targets -- -D warnings
```

CI (`.github/workflows/ci.yml`) führt diese Schritte bei jedem Push/PR auf
Linux, Windows und macOS aus.

## Dokumentation

- [`docs/adr/`](docs/adr/README.md) – Architekturentscheidungen
- [`docs/specs/`](docs/specs/README.md) – Feature-Spezifikationen
