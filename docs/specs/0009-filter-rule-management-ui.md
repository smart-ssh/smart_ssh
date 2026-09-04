# Spec: Filter-Regel-Verwaltung (UI)

Status: Entwurf
Modul: Schema-Erweiterung `persistence-sqlite`, neue Commands in
`crates/app-tauri`, neuer Screen in `frontend/`
Abhängigkeiten: `core::filter` (Spec 0002), `persistence-sqlite` (Spec 0004),
`app-tauri` Kernschleife (Spec 0007, Abschnitt 6)

## 1. Ziel

Bisher existiert die Filter-Engine (Spec 0002) nur mit einer
In-Memory-`PolicyStore`-Implementierung für Tests — es gibt noch **keine**
Möglichkeit, echte Regeln ("`ls *` immer erlauben", "`systemctl *` auf
Produktionsservern immer bestätigen") dauerhaft anzulegen und zu verwalten.
Diese Spec schließt genau diese Lücke: persistente Regeln + ein UI dafür.

Das ist der Moment, in dem die Filter-Engine erstmals **echt in die laufende
KI-Kommandoschleife eingebunden wird** (Spec 0007, Abschnitt 6) — bisher lief
dort mangels gespeicherter Regeln praktisch jeder Vorschlag auf den
Default-Fall `Confirm` hinaus.

## 2. Schema-Erweiterung (`persistence-sqlite`)

```sql
-- migrations/0003_filter_rules.sql

CREATE TABLE filter_rules (
    id            TEXT PRIMARY KEY,
    pattern_type  TEXT NOT NULL CHECK (pattern_type IN ('glob', 'regex', 'exact')),
    pattern_value TEXT NOT NULL,
    action        TEXT NOT NULL CHECK (action IN ('allow', 'confirm', 'deny')),
    scope_type    TEXT NOT NULL CHECK (scope_type IN ('global', 'server', 'tag')),
    scope_value   TEXT,   -- server_id oder Tag-Name, NULL bei 'global'
    priority      INTEGER NOT NULL DEFAULT 0,
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL
);

CREATE INDEX idx_filter_rules_scope ON filter_rules(scope_type, scope_value);
```

`SqlitePolicyStore` implementiert den `PolicyStore`-Trait aus Spec 0002.
Analog zu Spec 0004 (`ProfileStore`) muss `PolicyStore` dafür von
synchronen auf `async fn`-Methoden umgestellt werden (`async-trait`,
Trait-Objekt bleibt nutzbar) — gleiches Vorgehen, gleiche Begründung wie
bereits einmal durchgeführt.

## 3. Tauri-Commands

```
list_rules(scope_filter: Option<ScopeFilter>) -> Vec<RuleDto>
create_rule(input: RuleInput) -> RuleId
update_rule(id: RuleId, input: RuleInput)
delete_rule(id: RuleId)
list_hard_blacklist() -> Vec<PatternDto>   // nur Anzeige, read-only
list_known_tags() -> Vec<String>            // für die Scope-Auswahl im Formular
evaluate_explained(command: String, ctx: EvalContextInput) -> EvaluationTraceDto
```

```rust
pub enum ScopeFilter { Global, Server(ServerId), Tag(String), All }

pub struct RuleInput {
    pub pattern_type: PatternType,
    pub pattern_value: String,
    pub action: RuleAction,
    pub scope: ScopeInput,   // Global | Server(ServerId) | Tag(String)
    pub priority: i32,
}
```

## 4. Erklärbare Auswertung (`evaluate_explained`)

Löst den offenen Punkt aus Spec 0002, Abschnitt 7 ("Simulationsansicht").
Erweitert `FilterEngine` um eine zweite Methode neben `evaluate()`, die
zusätzlich zur `Decision` eine nachvollziehbare Spur zurückgibt:

```rust
pub struct EvaluationTrace {
    pub decision: Decision,
    pub matched_rule: Option<RuleId>,        // None, falls Hard-Blacklist oder Default gegriffen hat
    pub matched_hard_blacklist_entry: Option<String>,
    pub sub_command_traces: Vec<EvaluationTrace>, // bei Chaining: ein Trace pro Teilkommando (Spec 0002 Abschnitt 4)
}
```

> **Erweitert durch Spec 0037**: `EvaluationTrace` bekommt zusätzlich ein
> `matched_rule_origin: Option<RuleOrigin>`-Feld, damit im Testen-Panel
> (Abschnitt 6) erkennbar ist, ob eine Organisations- oder eine
> Nutzer-Regel gegriffen hat.

Diese Methode wird **nicht** im eigentlichen KI-Kommando-Loop verwendet
(dort reicht `evaluate()`), sondern ausschließlich für die
Testen-Funktion im UI (Abschnitt 6) — Transparenz für den Nutzer, warum eine
Entscheidung so gefallen ist, ohne die Kernschleife selbst zu verkomplizieren.

## 5. Anbindung an die Kernschleife

`Session.filter_engine` (Spec 0007, Abschnitt 3) wird ab jetzt mit einer
echten `SqlitePolicyStore`-Instanz aufgebaut statt einem leeren/In-Memory-Store.
Beim `evaluate()`-Aufruf in Schritt 4 der Kernschleife (Spec 0007, Abschnitt
6) greifen damit erstmals tatsächlich gespeicherte Nutzerregeln. Keine
Verhaltensänderung am Ablauf selbst — nur der bisher faktisch leere
`PolicyStore` wird durch einen echten ersetzt.

## 6. UI: Regel-Manager

- **Regel-Liste**, gruppiert nach Scope (Global-Regeln zuerst, dann pro
  Server, dann pro Tag) — innerhalb einer Gruppe sortiert nach `priority`.
  Jede Zeile zeigt Pattern, Aktion (farblich unterschieden: Allow grün,
  Confirm gelb, Deny rot), Priorität mit einfachen Auf/Ab-Buttons statt
  Drag-and-Drop (konsistent mit der bereits in Spec 0008 getroffenen
  Entscheidung gegen Drag-and-Drop-UI).
- **Hard-Blacklist-Sektion**, deutlich als "read-only, nicht bearbeitbar"
  gekennzeichnet — zeigt die fest im Core codierten Muster aus Spec 0002,
  Abschnitt 3.1, damit der Nutzer weiß, dass diese existieren, auch wenn er
  sie nicht ändern kann.
- **Regel-Formular**: Pattern-Typ (Glob/Regex/Exact) mit Eingabefeld,
  Aktion (Allow/Confirm/Deny), Scope-Auswahl (Global, oder Server aus
  `list_servers`, oder Tag aus `list_known_tags` — mit der Möglichkeit,
  auch ein neues, noch nicht verwendetes Tag einzutippen), Priorität als
  Zahlenfeld.
- **Testen-Panel**: Eingabefeld für ein Beispielkommando + optionale
  Scope-Auswahl (welcher Server/welche Tags simuliert werden sollen), Klick
  auf "Testen" ruft `evaluate_explained` auf und zeigt das Ergebnis
  nachvollziehbar an — bei mehreren Teilkommandos (Chaining) wird jeder Teil
  einzeln mit seiner eigenen Entscheidung aufgelistet, plus die
  Gesamt-Entscheidung. Das macht die in Spec 0002 beschriebene
  "strengstes Teilergebnis gewinnt"-Logik für den Nutzer sichtbar statt
  abstrakt.

## 7. Offene Punkte

- Sollen Regeln testweise **deaktivierbar** sein (An/Aus-Schalter), ohne sie
  zu löschen? Praktisch für "ich will die Regel behalten, aber gerade nicht
  aktiv haben" — aktuell nicht vorgesehen, ließe sich als kleines Feld
  (`enabled: bool`) leicht nachrüsten, falls gewünscht.
- Import/Export von Regelsätzen (z. B. um eine bewährte Konfiguration mit
  anderen zu teilen oder zwischen Rechnern zu übertragen) — nicht Teil
  dieser Spec, verwandtes Thema zum bereits offenen Export/Import-Punkt aus
  Spec 0004.
