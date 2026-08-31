# 0016-filter-rule-management-design-gaps

## Status
Accepted

## Kontext

`docs/specs/0009-filter-rule-management-ui.md` skizziert die Filter-Regel-
Verwaltung (Schema, Commands, `RuleInput`/`ScopeFilter`/`EvaluationTrace`)
größtenteils als Pseudocode, ohne jeden referenzierten Typ konkret zu
definieren. Beim Implementieren ergaben sich vier Stellen, an denen eine
begründete Entscheidung nötig war.

## Entscheidungen

**1. `RuleId` als `String`-Newtype, nicht Uuid-basiert.** Die Spec
referenziert `RuleId` in den Tauri-Command-Signaturen (`create_rule(...) ->
RuleId`), definiert den Typ selbst aber nicht — anders als `ServerId`/
`GroupId`/`ProviderId` (alle Uuid-basiert seit ihren jeweiligen Specs).
`core::filter::Rule.id` war in Spec 0002 von Anfang an ein freier `String`
(für sprechende IDs wie `"allow-ls"` in Tests/Beispielen), und Spec 0009s
SQL-Schema legt nur `id TEXT PRIMARY KEY` fest, ohne UUID-Formatierung zu
verlangen. `RuleId(pub String)` (`crates/core/src/filter/types.rs`) erhält
diese Flexibilität; `persistence-sqlite` befüllt ihn in der Praxis mit
`Uuid::new_v4().to_string()` (analog zu den anderen IDs), aber der Typ
selbst zwingt das nicht.

**2. `ScopeFilter::All` weggelassen.** Die Spec-Skizze definiert
`pub enum ScopeFilter { Global, Server(ServerId), Tag(String), All }` und
gleichzeitig `list_rules(scope_filter: Option<ScopeFilter>)` — mit `None`
und `Some(All)` als zwei Werten für exakt dasselbe ("keine Einschränkung"),
redundant. `list_rules` nimmt stattdessen direkt `Option<Scope>` entgegen
(`crates/app-tauri/src/commands.rs`/`filter_rules.rs`): `None` bedeutet
"alle Regeln", `Some(scope)` filtert exakt auf diesen Scope. Das deckt
denselben Anwendungsfall ab, ohne eine separate `ScopeFilter`-Deklaration
und deren Konvertierung zu brauchen.

**3. `RuleInput`/`RuleDto` nutzen `core::filter::{Scope, RuleAction}`
direkt statt eigener `ScopeInput`-artiger Typen.** Die Spec-Skizze
suggeriert mit dem Kommentar `scope: ScopeInput` einen eigenen Typ, aber
`Scope`/`RuleAction` sind bereits `Serialize + Deserialize` und werden
bereits unverändert über dieselbe IPC-Grenze gereicht (z. B. `Decision` in
`chat-action-proposed`, Spec 0007). Ein strukturell identischer Zweit-Typ
hätte keinen Mehrwert gehabt — nur Konvertierungscode ohne Nutzen. Frontend
(`types.ts`) spiegelt `Scope`/`RuleAction` in derselben Standard-
Außen-Tagging-Form wie das bereits etablierte `Decision`.

**4. `PolicyStore::rules_for` bleibt ohne `Result`, Datenbankfehler werden
zu leerer Regelliste.** Das Trait aus Spec 0002 kennt kein `Result` in
dieser Methode (`fn rules_for(&self, scope: &EffectiveScope) -> Vec<Rule>`),
Spec 0009 (Async-Umstellung) ändert daran nichts. `SqlitePolicyStore`
(`crates/persistence-sqlite/src/policy_store.rs`) loggt einen DB-Fehler
und liefert `Vec::new()` statt zu propagieren oder zu panicken — konsistent
mit den "Fail-safe defaults" aus Spec 0002 Abschnitt 1: ohne ladbare Regeln
fällt `FilterEngine` auf ihre eingebauten Defaults zurück (Hard-Blacklist
bleibt aktiv, alles andere landet auf `Confirm` statt `AutoExec`). Der
worst case eines DB-Fehlers ist damit "mehr Confirm als nötig", nie "mehr
AutoExec als beabsichtigt".

## Konsequenzen

**Positiv:**
- Keine unnötigen Parallel-Typen (`ScopeInput`, `ScopeFilter`) — weniger
  Konvertierungscode, weniger Fläche für künftige Inkonsistenzen zwischen
  Typ und tatsächlicher Serialisierungsform.
- `RuleId` bleibt so flexibel wie von Spec 0002 ursprünglich vorgesehen,
  ohne bestehende, auf sprechenden String-IDs aufbauende Tests
  umschreiben zu müssen.
- Ein Datenbankproblem beim Laden von Filter-Regeln blockiert nie den
  gesamten KI-Kommando-Loop — es fällt lediglich auf sicherere Defaults
  zurück.

**Negativ / Trade-off:**
- `RuleId`s fehlende Typ-Garantie (kein UUID-Format erzwungen) bedeutet,
  dass ein zukünftiger externer Import/Export von Regelsätzen (s. Spec
  0009 Abschnitt 7, offener Punkt) auf beliebige String-IDs treffen könnte
  — bei Bedarf müsste Eindeutigkeit dann explizit geprüft werden, statt
  sich auf UUID-Kollisionsresistenz zu verlassen.
- Ein stiller Fallback auf `Vec::new()` bei einem DB-Fehler ist nur über
  ein `eprintln!`-Log sichtbar, nicht über ein Tauri-Event — ein
  dauerhafter DB-Fehler (z. B. gesperrte Datenbank) würde sich für den
  Nutzer nur als "alle KI-Kommandos brauchen plötzlich Bestätigung"
  bemerkbar machen, ohne offensichtlichen Hinweis auf die Ursache.
