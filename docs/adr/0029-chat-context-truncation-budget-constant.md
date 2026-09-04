# 0029-chat-context-truncation-budget-constant

## Status
Vorgeschlagen

## Kontext

`crates/app-tauri/src/chat_context_truncation.rs` kürzt die an
`AiProvider::send()` gesendete Kopie der Chat-Historie auf ein festes
Zeichenbudget (Spec 0034, Abschnitt 9):

```rust
pub const DEFAULT_CHAR_BUDGET: usize = 40_000;
```

Der Wert ist ein einzelner Rust-`const`, weder in den Server-/App-
Einstellungen konfigurierbar noch pro AI-Provider/Modell unterschiedlich.
Ein unabhängiger Sitzungs-Abgleich (Spec 0040, "kleiner Fund"-Kategorie)
hat gefragt, ob das als Lücke zu behandeln ist — die Spec selbst schreibt
keinen fest verdrahteten Wert vor, nur "ein Budget".

## Entscheidung

Der feste Konstantenwert bleibt eine bewusste Vereinfachung, **keine
Nutzer-Einstellung**. Gründe:

- Ein Zeichen-Budget ist ohnehin nur eine grobe Näherung an das
  eigentlich relevante Limit (Token-Budget des jeweiligen KI-Providers/
  Modells) — verschiedene Provider/Modelle haben unterschiedliche
  Token-Fenster und unterschiedliche Zeichen-zu-Token-Verhältnisse (v. a.
  zwischen lateinischer Schrift und anderen Alphabeten). Eine einzige
  Nutzer-Einstellung würde diese Ungenauigkeit nicht beheben, nur einen
  zusätzlichen falschen Eindruck von Präzision erzeugen.
- Das Feature, das dieses Budget überhaupt braucht (sehr lange Sitzungen,
  die das Kontextfenster sprengen), ist ein Rand-, kein Kernfall — die
  meisten Sitzungen bleiben weit darunter. Eine sichtbare Einstellung
  dafür würde UI-Komplexität für einen selten wirksam werdenden Wert
  hinzufügen.
- `DEFAULT_CHAR_BUDGET` ist bereits als eigener, benannter, exportierter
  `pub const` isoliert (nicht magisch inline verstreut) und über
  `truncate_to_budget_with(history, budget)` parametrisierbar — die
  Grundlage für eine spätere Konfigurierbarkeit existiert also, ohne dass
  sie jetzt exponiert werden müsste.

Kein Code-Änderungsbedarf aus diesem ADR — die bestehende Konstante bleibt
wie sie ist, dieses Dokument hält nur fest, dass das Fehlen einer
Einstellung eine bewusste Entscheidung ist, kein übersehener Punkt.

## Konsequenzen

**Positiv:**
- Keine zusätzliche UI-/Einstellungs-Fläche für einen Wert, dessen
  Genauigkeit ohnehin begrenzt ist.
- `truncate_to_budget_with` existiert bereits parametrisiert — eine
  künftige Konfigurierbarkeit (z. B. pro AI-Provider-Konfiguration) ließe
  sich nachrüsten, ohne die Kürzungslogik selbst zu ändern.

**Negativ / Trade-off:**
- Nutzer mit einem Provider/Modell, dessen tatsächliches Kontextfenster
  deutlich kleiner oder größer als das, wofür 40.000 Zeichen kalibriert
  sind, ausfällt, können das Verhalten nicht direkt anpassen — nur über
  eine künftige Code-Änderung, nicht zur Laufzeit.
