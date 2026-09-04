# Spec: Repository-Trennung, App-Shell & CI-Matrix

Status: Entwurf
Modul: Refactoring `app-tauri` → Library `app-shell`, neues
`apps/smart-ssh-community`, `deny.toml`, `.github/workflows/community.yml`
Abhängigkeiten: Entitlements (Spec 0037), bestehende CI-Matrix (Spec 0001),
CI-Dependency-Audit (Spec 0035 — `deny.toml` wird hier fachlich erweitert,
nicht doppelt angelegt)

Grundlage: Architektur-Brief, Abschnitte 1 (D2, D6, D7), 3, 5, 8.

## 1. Ziel

Das bisherige `app-tauri` wird zu einer **Library** `app-shell` mit einem
`Wiring`-Struct refaktoriert, das definiert, welche Implementierungen
(Entitlements, KI-Provider, Policy-Quellen, Sync-Backends, Plugins) verdrahtet
werden. Community- und (künftig) Official-Binary werden zwei dünne
`main.rs`-Dateien, die sich nur im übergebenen `Wiring` unterscheiden. Das
öffentliche Repo bekommt damit die Struktur, die D7 (private hängt von
öffentlich ab, nie umgekehrt) technisch voraussetzt.

**Kein privates Repo in dieser Spec** — nur die öffentliche Seite der
Trennung. Das private Repo folgt separat, sobald du es angelegt hast
(du hast das gerade parallel begonnen).

## 2. `app-shell` als Library

```rust
// crates/app-shell/src/lib.rs

pub struct Wiring {
    pub entitlements: Arc<dyn EntitlementProvider>,
    pub ai_providers: Vec<Arc<dyn AiProvider>>,
    pub policy_sources: Vec<Arc<dyn PolicySource>>,
    pub sync_backends: Vec<Arc<dyn SyncBackend>>,
    pub plugins: Vec<Box<dyn FnOnce(tauri::Builder<Wry>) -> tauri::Builder<Wry> + Send>>,
    pub edition: Edition,   // Community | Official — nur für Anzeige/Updater
}

impl Wiring {
    pub fn community() -> Self {
        Self {
            entitlements: Arc::new(FixedEntitlements(Entitlements::free())),
            ai_providers: ai_providers::all_byok(),
            policy_sources: vec![Arc::new(SqlitePolicySource::new(..))],
            sync_backends: vec![],   // kein Git-Sync-Backend in dieser Spec, s. Spec 0037 Abschnitt 6
            plugins: vec![],
            edition: Edition::Community,
        }
    }
}

pub fn run(wiring: Wiring) -> tauri::Result<()> { /* bisheriger app-tauri-Setup-Code, unverändert in der Substanz */ }
```

**Vorgehen beim Refactoring**: Der bestehende `app-tauri`-Code (State-Setup,
Command-Registrierung, Event-Handling — alles, was seit Spec 0007 entstanden
ist) wird **inhaltlich unverändert** in `app_shell::run()` verschoben, nicht
neu geschrieben. Diese Spec ist ein Struktur-Refactoring, keine
Verhaltensänderung — jeder bestehende Test muss danach unverändert grün
bleiben.

## 3. Zwei Binaries

```
apps/
  smart-ssh-community/
    src/main.rs   → app_shell::run(Wiring::community())
```

Das bisherige `app-tauri`-Binary wird zu diesem dünnen `main.rs`. Ein
`apps/smart-ssh/` (Official) existiert **nicht** in dieser Spec — das
entsteht erst mit dem privaten Repo.

## 4. Frontend-Registry

```
frontend/packages/app/   Registry: registerRoute, registerPanel,
                          registerSettingsSection, registerCommandPalette
```

`useEntitlements()`-Hook: liest den Entitlement-Stand per Command, abonniert
ein Tauri-Event `entitlements:changed` (Event wird ausgelöst, wenn
`EntitlementProvider::watch()` einen neuen Stand liefert — in der Community
Edition passiert das aktuell nie, da `FixedEntitlements` sich nie ändert,
aber die Infrastruktur muss stehen).

Gesperrte Funktionen: Da es aktuell **keine** gesperrten Community-Features
gibt außer dem einen Fall aus Spec 0037 (Word-Export), reicht in diesem
Schritt, dass `FeatureLocked`-Fehler zentral abgefangen und in einen
einfachen Hinweis-Dialog übersetzt werden ("Diese Funktion erfordert
Pro") — kein vollständiges Upgrade-Flow-Design in dieser Spec, das folgt,
sobald es mehr als ein Beispiel gibt.

## 5. `deny.toml`-Erweiterung

Baut auf Spec 0035 auf (dort bereits als Berichtsmodus eingeführt) — diese
Spec macht daraus, sofern Spec 0035 inzwischen bereinigt und auf blockierend
umgestellt wurde, einen **Pflichtschritt** in `community.yml` (Abschnitt 6):
`cargo deny check licenses` muss ohne Ausnahme grün sein, damit
`community.yml` durchläuft. Keine neue `deny.toml`-Konfiguration — dieselbe
Datei, dieselben Regeln wie in Spec 0035.

## 6. `community.yml`

Neuer/angepasster CI-Workflow, der explizit **aus einem frischen Klon ohne
Secrets** baut: `cargo build`, `cargo test`, `cargo clippy -D warnings`,
`cargo fmt --check`, `cargo deny check licenses`, Frontend-Build. Läuft auf
der bestehenden Matrix aus Spec 0001 (macOS/Windows/Linux).

**Zweck dieses Jobs**: beweisen, dass das öffentliche Repo eigenständig baut
— schlägt ein Schritt fehl, weil (später) etwas aus dem privaten Repo fehlt,
ist das ein Bug in der Trennung, nicht im öffentlichen Code selbst. Da es
aktuell noch kein privates Repo mit echter Abhängigkeit gibt, ist dieser
Job vorerst identisch zur bestehenden CI-Matrix aus Spec 0001 — der
eigentliche Wert entsteht erst, sobald das private Repo existiert und diese
Pipeline als Regressionsschutz gegen versehentliche private Abhängigkeiten
dient.

## 7. Sicherheits-Invariante

- Das Refactoring darf **keine** bestehende Funktionalität verändern —
  jeder Test, der vor dieser Spec grün war, muss danach unverändert grün
  sein. Das ist keine "neue Funktion", sondern eine Struktur-Verschiebung.

## 8. Testbarkeit

- Vollständiger bestehender Testlauf (`cargo test --workspace`) bleibt nach
  dem Refactoring unverändert grün — kein neuer Test nötig, der bestehende
  Testbestand ist die Abnahme.
- `community.yml` läuft erfolgreich aus einem frischen Klon durch.

## 9. Offene Punkte

- SBOM-Erzeugung (`cargo-cyclonedx`) aus Abschnitt 8 des Architektur-Briefs
  ist nicht Teil dieser Spec — sinnvolle spätere Ergänzung zu
  `community.yml`, aber kein Blocker für die Repository-Trennung selbst.
- Vollständiges Upgrade-Dialog-Design (Abschnitt 4) folgt, sobald mehr als
  ein gegatetes Feature existiert.
