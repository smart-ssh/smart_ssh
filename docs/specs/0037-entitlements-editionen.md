# Spec: Entitlements & Editionen

Status: Entwurf
Modul: Erweiterung `ssh-manager-core` (Apache-2.0-lizenziert)
Abhängigkeiten: Filter-Engine (Spec 0002 — Erweiterung um `RuleOrigin`),
KI-Dokumente (Spec 0012 — erste praktische Gating-Anwendung), Chat-Session-
Persistenz (Spec 0034 — Ziel-Datenmodell, nicht zwingende Migration)

Grundlage: Architektur-Brief "Editionen, Lizenz und Entitlements",
Abschnitte 1 (D1–D9), 2 (Feature-Matrix), 4 (Core-Erweiterungen).

## 1. Ziel

Das Vokabular für Bezahlfunktionen wird in `ssh-manager-core` eingeführt —
öffentlich, Apache-2.0-lizenziert, aber **ohne** die eigentlichen
Bezahlmodul-Implementierungen (die kommen später in ein privates Repo,
Spec 0038). `ssh-manager-core` bleibt dabei frei von Entitlement-**Logik**
im Sinne von "was darf ein bestimmter Nutzer" — es kennt nur die Typen
(D5).

## 2. Feature-Enum und Entitlements

```rust
// core/src/entitlements.rs

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Feature {
    SharedInventory,
    SharedNotes,
    CuratedRulePacks,
    OrgPolicy,
    MultiServerActions,
    ManagedAi,
    OrgAiPolicy,
    TeamAgents,
    SessionHistory,
    ActivityReport,
    SessionHandover,
    CloudSync,
    DocumentExport,
    AuditExport,
    Sso,
    SelfHosted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tier { Free, Personal, Pro, Team, Business, Enterprise }

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entitlements {
    pub tier: Tier,
    pub features: HashSet<Feature>,
    pub seats: Option<u32>,
    pub expires_at: Option<DateTime<Utc>>,
    pub non_commercial: bool,
    pub licensee: Option<String>,
}

impl Entitlements {
    pub fn free() -> Self { /* Tier::Free, features leer */ }
    pub fn has(&self, f: Feature) -> bool { self.features.contains(&f) }
    pub fn require(&self, f: Feature) -> Result<(), FeatureLocked> {
        if self.has(f) { Ok(()) } else { Err(FeatureLocked { feature: f, tier: self.tier }) }
    }
}

#[derive(Debug, thiserror::Error, Serialize)]
#[error("feature {feature:?} is not available in tier {tier:?}")]
pub struct FeatureLocked { pub feature: Feature, pub tier: Tier }

pub trait EntitlementProvider: Send + Sync {
    fn current(&self) -> Entitlements;
    fn watch(&self) -> tokio::sync::watch::Receiver<Entitlements>;
}

/// Für die (aktuell einzige existierende) Community Edition und für Tests.
pub struct FixedEntitlements(pub Entitlements);
```

`FeatureLocked` wird in den bestehenden Tauri-Command-Fehlertyp
eingebettet (nicht als String transportiert), damit das Frontend ihn
eindeutig von fachlichen Fehlern unterscheiden kann.

**Kein `CertificateAuth`-Feature** — Zertifikats-Auth ist bereits
veröffentlicht und bleibt endgültig Free (D4), kein Gating dafür.

## 3. Gating-Konvention (D5)

Jeder Tauri-Command, der ein gegatetes Feature auslöst, prüft **als erste
Anweisung**:

```rust
#[tauri::command]
pub async fn some_gated_command(state: State<'_, AppState>, ...) -> Result<T, AppError> {
    state.entitlements.current().require(Feature::XY)?;
    // ... eigentliche Logik
}
```

Gesperrte Features schlagen **geschlossen** fehl (Fehler, nie stille
Degradierung auf ein "Free-Verhalten" als Fallback).

## 4. Rückbau statt Gating: Word-Export wird aus dem öffentlichen Repo entfernt

Ursprünglich war hier eine `require(Feature::DocumentExport)`-Prüfung für
`export_document` mit `format: Word` vorgesehen. **Geänderte Entscheidung**:
Da das private Repo noch nicht angelegt ist und es aktuell keinen
Lizenzschlüssel-Mechanismus gibt, der das Feature je freischalten könnte,
wäre eine Gating-Prüfung an dieser Stelle totem Code — sie würde nie greifen
können. Stattdessen wird der Word-Export-Pfad **komplett aus dem
öffentlichen Repo entfernt** und später, sobald das private Repo existiert,
dort neu gebaut (entspricht dem im Architektur-Brief, Abschnitt 11, Punkt 4
beschriebenen Plan: das docx-Modul wandert als erstes Pro-Modul ins private
Repo, statt im öffentlichen Repo zu verbleiben und nur gegatet zu werden).

Konkret aus Spec 0012 zu entfernen:
- Der `DocumentFormat::Word`-Zweig in `export_document` (Backend-Konvertierung
  über `docx-rs`).
- Die `docx-rs`-Abhängigkeit selbst, falls sie ausschließlich für diesen
  Zweck genutzt wird.
- Der "Als Word speichern"-Button im Frontend (Spec 0012, Abschnitt 3).
- Zugehörige Word-spezifische Tests.

**Unverändert bleibt**: `DocumentFormat::Markdown` und der komplette
Markdown-Export-Pfad — vollständig Free, keine Änderung.

`Feature::DocumentExport` bleibt als Enum-Variante im Vokabular (Spec 0037,
Abschnitt 2) bestehen — es wird nur aktuell nirgends geprüft, da es nichts
gibt, das es gaten müsste. Sobald das private Repo den Word-Export neu
aufbaut, kommt die `require()`-Prüfung dort hinzu, nicht rückwirkend hier.

## 5. Policy-Ebenen in der Filter-Engine

Erweiterung von Spec 0002 um eine **Ebene** als zusätzliches
Ordnungskriterium:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RuleOrigin {
    Builtin,       // Hard-Blacklist, bestehend, unverändert
    Organization,  // von einer Organisation bereitgestellt, lokal nicht übersteuerbar
    User,          // vom Nutzer definiert (bestehender SQLite-Regelspeicher)
}
```

**Präzise Sortierreihenfolge** (löst die Mehrdeutigkeit zwischen "Ebene vor
Spezifität" und "Organization-Deny schlägt User-Allow immer" auf, indem
beides in einem konsistenten Algorithmus zusammengeführt wird):

1. Hard-Blacklist (`Builtin`) — wie bisher, unverändert zuerst geprüft.
2. **Aktions-Tier**: Deny > Confirm > Allow > Default (wie bisher, Spec
   0002 Abschnitt 3 — ändert sich durch `RuleOrigin` **nicht**. Eine
   `Organization`- oder `User`-Deny-Regel schlägt jede Allow-Regel, exakt
   wie bereits vor dieser Erweiterung.)
3. **Innerhalb desselben Aktions-Tiers**: `Organization` vor `User` —
   *das* ist die tatsächlich neue Sortierebene. Konkret: existieren im
   selben Tier (z. B. beide `Confirm`) sowohl eine `Organization`- als auch
   eine `User`-Regel, die auf dasselbe Kommando passen, gewinnt die
   `Organization`-Regel — **unabhängig davon, welche der beiden die
   spezifischere Server-/Tag-/Global-Zuordnung hat.**
4. Erst danach: Scope-Spezifität (Server > Tag > Global), dann `priority`
   — wie bisher, jetzt aber erst nach Schritt 3 angewendet.

`evaluate_explained` (Spec 0009) gibt zusätzlich die `RuleOrigin` der
greifenden Regel aus.

```rust
#[async_trait]
pub trait PolicySource: Send + Sync {
    fn origin(&self) -> RuleOrigin;
    async fn rules(&self) -> Result<Vec<Rule>>;
    async fn watch(&self) -> Result<watch::Receiver<Vec<Rule>>>;
}
```

Der bestehende SQLite-Regelspeicher (Spec 0009) ist und bleibt die einzige
`User`-Quelle. `ssh-manager-core` selbst liefert **keine**
`Organization`-Quelle — das ist Aufgabe eines künftigen `OrgPolicy`-Moduls
im privaten Repo (Spec 0038 ff.), das lediglich Regeln mit
`origin() == Organization` über diesen Trait bereitstellt. In der
Community Edition ist die Liste der `PolicySource`s also immer nur die eine
SQLite-Quelle — dieselbe Filter-Engine-Logik, keine Sonderbehandlung.

## 6. Weitere Traits (Vokabular, keine Implementierung)

```rust
#[async_trait]
pub trait SyncBackend: Send + Sync {
    async fn push(&self, bundle: EncryptedBundle) -> Result<SyncReceipt>;
    async fn pull(&self) -> Result<Option<EncryptedBundle>>;
}
```

Nur die Trait-Definition — **kein** Git-Sync/Cloud-Sync-Implementierung in
dieser Spec. `AiProvider` (Spec 0006) bleibt unverändert; Managed AI wird
später eine weitere Implementierung mit eigenem Provider-Typ, Redaction
bleibt providerunabhängig vor jedem Versand (unverändert aus Spec 0006).

## 7. Session-Modell (Zielbild, keine erzwungene Migration)

```rust
pub struct Session {
    pub id: SessionId,
    pub server_id: ServerId,
    pub origin: SessionOrigin,           // Human | McpAgent { agent_id }
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub messages: Vec<Message>,          // kanonisches Format, redigiert
    pub ledger: Vec<LedgerEntry>,        // wörtlich, nie kompaktiert
    pub summary: Option<Summary>,
    pub compaction: CompactionState,
}

pub struct LedgerEntry {
    pub at: DateTime<Utc>,
    pub command: String,
    pub decision: Decision,
    pub rule: Option<RuleRef>,
    pub exit_code: Option<i32>,
    pub output_digest: OutputDigest,
}
```

Diese Typen werden **eingeführt**, aber die bestehende Chat-Session-
Implementierung aus Spec 0034 wird **nicht** im Rahmen dieser Spec
zwangsweise darauf migriert — das ist Gegenstand eines separaten Abgleichs
(siehe begleitender Auftrag an den `spec-reviewer`-Subagenten, Abschnitt 8).

## 8. Sitzungs-Abgleich — kein Spec-C, sondern ein Subagenten-Auftrag

Statt einer eigenen nummerierten Spec: Der bereits eingerichtete
`spec-reviewer`-Subagent (`docs/review-prompt-template.md`) wird mit den
Prüfkriterien aus dem Architektur-Brief, Abschnitt 7 (Punkte 1–8) gegen die
bestehende Chat-Session-Implementierung (Spec 0034, ggf. Spec 0036)
angesetzt — als Audit, nicht als Implementierungsauftrag. Findet er
Abweichungen, werden diese als eigene, kleine Folge-Spec nachgezogen
(Nummer nach 0038), nicht stillschweigend gefixt.

## 9. Sicherheits-Invarianten (Ergänzung)

- Keine Entitlement-Prüfung darf `FilterEngine::evaluate()` umgehen oder
  ersetzen; ein gesperrtes Feature ändert nie das Ergebnis der Filter-Engine.
- `Organization`-Regeln sind durch `User`-Regeln nicht aufhebbar — Test
  für Allow(User) gegen Deny(Organization) und Allow(User) gegen
  Confirm(Organization), jeweils mit **höherer** Scope-Spezifität auf
  Nutzerseite (das ist der Fall, der ohne Schritt 3 in Abschnitt 5 falsch
  entschieden würde).
- Gesperrte Features schlagen geschlossen fehl, keine stille Degradierung.

## 10. Testbarkeit

- `FixedEntitlements` in allen bestehenden Tests weiterhin nutzbar.
- Gating-Test für `export_document`: `format: Word` ohne
  `Feature::DocumentExport` → `FeatureLocked`; `format: Markdown` läuft
  immer durch, unabhängig vom Entitlement-Zustand.
- Policy-Ebenen-Tests gemäß Abschnitt 9.
- `evaluate_explained` nennt korrekt die `RuleOrigin` der greifenden Regel.

## 11. Offene Punkte

- Konkrete `Organization`-`PolicySource`-Implementierung (`OrgPolicy`) ist
  nicht Teil dieser Spec — folgt im privaten Repo.
- Ergebnis des Sitzungs-Abgleichs (Abschnitt 8) ist zum Zeitpunkt dieser
  Spec unbekannt — mögliche Folge-Spec noch nicht nummeriert.
