# Spec: Architektur-Übersicht

Status: Entwurf
Betrifft: Gesamtprojekt
Abhängigkeiten: keine — dies ist die Grundlage für alle weiteren Specs

## 1. Ziel des Projekts

Ein plattformübergreifender SSH-Client (macOS, Windows, Linux) mit:

- Server-Manager (Profile, Passwort-/Key-/Zertifikats-Auth)
- KI-Chat-Integration: Nutzer chattet mit einer frei wählbaren KI (eigene
  API-Keys), die SSH-Kommandos vorschlägt
- Vorschläge laufen durch eine Filter-/Policy-Engine, die je nach Regel
  automatisch ausführt oder eine explizite Bestätigung verlangt
- Kernprinzip des gesamten Projekts: **volle Transparenz und Kontrolle über
  jedes Kommando, das auf einem Server landet.** Die KI ist Copilot, nie
  autonomer Akteur ohne Sichtbarkeit für den Nutzer.

## 2. Tech-Stack-Entscheidung

**Gewählt: Tauri 2.0 (Rust-Backend + Web-Frontend)**

Begründung:
- Deutlich kleinere Bundle-Size und geringerer Ressourcenverbrauch als
  Electron; kein mitgeliefertes Node.js-Backend als zusätzliche Angriffsfläche
- Ausgereifte SSH-Bibliotheken in Rust (`russh` favorisiert, `libssh2`-Bindings
  als Alternative)
- Plattformübergreifender Zugriff auf OS-Schlüsselbunde über die `keyring`-
  Crate (macOS Keychain, Windows Credential Manager, Linux Secret Service)
  ohne plattformspezifischen Sonderaufwand
- Die Kernlogik lässt sich als UI-unabhängige Rust-Library bauen und später
  1:1 in einer TUI wiederverwenden (siehe Abschnitt 3)

Alternativen wurden verworfen: Electron (größere Angriffsfläche, höherer
Ressourcenverbrauch), native Implementierung pro Plattform (dreifacher
Wartungsaufwand, keine geteilte Kernlogik).

Frontend-Framework innerhalb von Tauri: noch nicht final entschieden
(React/Svelte/Solid) — siehe offene Punkte, Abschnitt 6.

## 3. Architektur-Layering

```
crates/
├── core/          # reine Logik, KEIN UI, KEIN Tauri
│   ├── ssh/         # Verbindungs-Handling, trait-basiert
│   ├── credentials/ # Credential-Store-Abstraktion
│   ├── filter/       # Policy-Engine (siehe Spec 0002)
│   ├── ai/            # AI-Provider-Abstraktion
│   └── audit/          # Logging
├── tui/            # später, nutzt core direkt
└── app-tauri/      # dünner Wrapper um core, Tauri-Commands/Events
frontend/            # React/Svelte, spricht ausschließlich mit app-tauri
```

**Verbindliche Regel:** `core` hat keine Abhängigkeit auf Tauri oder ein
UI-Framework. Jede fachliche Logik gehört dorthin, nicht in `app-tauri`.
`app-tauri` übersetzt lediglich zwischen Tauri-Commands/Events und den
`core`-APIs. Das stellt sicher, dass die spätere TUI (`crates/tui`) dieselbe
Logik ohne Duplikation nutzen kann.

## 4. Testbarkeits-Prinzip

Für jede Außenanbindung (SSH-Verbindung, KI-API, Credential-Store) wird ein
Trait definiert, bevor die konkrete Implementierung entsteht:

```rust
trait SshTransport {
    fn execute(&mut self, cmd: &str) -> Result<CommandOutput>;
}

trait AiProvider {
    fn suggest(&self, context: &SessionContext) -> Result<AiSuggestion>;
}

trait PolicyStore {
    fn rules_for(&self, scope: &EffectiveScope) -> Vec<Rule>;
}
```

Dadurch lässt sich jede Modul-Logik (insbesondere die sicherheitskritische
Filter-Engine, siehe Spec 0002) mit In-Memory-/Mock-Implementierungen testen,
ganz ohne echte SSH-Verbindung oder API-Calls. Jedes fachliche Modul in
`core` bekommt eine eigene Testsuite; UI-Code wird nicht auf dieselbe Weise
durchgetestet, sondern bleibt bewusst dünn.

## 5. Entwicklungsprozess (projektübergreifend)

Da das Projekt KI-gestützt entwickelt wird (siehe Diskussion zu Claude Code):

- **Spec-first pro Modul**: jedes fachliche Modul bekommt eine nummerierte
  Spec in `docs/specs/`, bevor Code entsteht
- **Tests vor/parallel zur Implementierung**, besonders bei sicherheitskritischen
  Modulen wie der Filter-Engine
- **Kleine, review-bare Schritte**: ein Modul/eine Funktion pro Durchgang
- **ADRs** in `docs/adr/` für Architekturentscheidungen, die über den Scope
  einer einzelnen Spec hinausgehen (z. B. "warum russh statt libssh2")
- **Test-Gate vor jedem Commit**: `cargo test --workspace`,
  `cargo clippy --workspace -- -D warnings`, `cargo fmt --check` müssen grün
  sein, bevor committet wird (durchgesetzt sowohl lokal als auch in CI)
- **Unabhängiger Review-Pass nach größeren Implementierungsschritten**: In
  einer **frischen** Claude-Code-Session (kein `--resume`/`--continue` der
  Implementierungs-Session), damit die Bewertung nicht durch die gerade
  getroffenen Implementierungsentscheidungen voreingenommen ist — dieselbe
  Logik wie bei menschlichem Code-Review durch eine zweite Person. Die
  Review-Session liest ausschließlich die betroffene(n) Spec(s) und den
  Diff, ändert selbst keinen Code, sondern liefert einen strukturierten
  Bericht. Vorlage dafür liegt unter `docs/review-prompt-template.md`. Bei
  sicherheitskritischen Modulen (Filter-Engine, Risiko-Klassifizierer,
  Redactor, Credential-Handling) läuft der Review-Pass mit erhöhter
  Priorität inklusive adversarialem Testen (gezielte Versuche, die Logik zu
  umgehen), bei den übrigen Modulen genügt der reguläre
  Spec-Konformitäts-/Invarianten-Check.

## 6. Offene Punkte

- Frontend-Framework (React/Svelte/Solid) — noch nicht entschieden
- Genaues Datenformat für Server-Profile und Credentials (eigene Spec folgt)
- Lizenzmodell des Projekts (Open Source? Falls ja, welche Lizenz?)
- Ob/wie Team-Sharing von Server-Profilen langfristig unterstützt wird
