# Spec: Regel-Schnellvorschlag im Bestätigungsdialog

Status: Entwurf
Modul: Erweiterung `crates/app-tauri` + `frontend/`
Abhängigkeiten: `core::filter` (Spec 0002), Regel-Verwaltung (Spec 0009),
Bestätigungsdialog (Spec 0007, Abschnitt 6/7)

## 1. Ziel

Im bestehenden Bestätigungsdialog für `Confirm`-Kommandos (weder Blacklist
noch Whitelist gegriffen) bekommt der Nutzer einen zusätzlichen Button
**"Akzeptieren und Regel erstellen"** mit einem Dropdown sinnvoller
Muster-Vorschläge. Ein Klick erledigt beides gleichzeitig: das aktuelle
Kommando wird ausgeführt **und** eine passende Allow-Regel angelegt, damit
ähnliche Kommandos künftig nicht erneut bestätigt werden müssen.

## 2. Muster-Vorschläge

```
suggest_rule_patterns(command: String) -> Vec<PatternSuggestionDto>
```

```rust
pub struct PatternSuggestionDto {
    pub label: String,           // menschenlesbar, für die Dropdown-Anzeige
    pub pattern_type: PatternType,
    pub pattern_value: String,
}
```

Heuristik (bewusst einfach, kein Anspruch auf Vollständigkeit):
- **Exakt**: das Kommando selbst, unverändert (`Pattern::Exact`)
- **Basis-Wildcard**: erstes Token + `" *"`, z. B. `ls -la /var/log` →
  `ls *` (nur falls das Kommando mehr als ein Token hat)
- **Subkommando-Wildcard**: falls das zweite Token nicht mit `-`/`--`
  beginnt (sieht nach einem Subkommando aus, nicht nach einer Flag), z. B.
  `systemctl status nginx` → `systemctl status *`

Maximal drei Vorschläge, Duplikate (falls zwei Heuristiken dasselbe Muster
ergeben) werden entfernt.

## 3. Kombinierter Command

```
accept_and_create_rule(
    session_id: SessionId,
    action_id: ActionId,
    pattern: PatternInput,
    scope: ScopeInput,
    priority: Option<i32>,
) -> RuleId
```

Führt intern zwei Schritte atomar hintereinander aus: 1) legt die Regel an
(gleiche Logik wie `create_rule`, Spec 0009), 2) löst die wartende
`Confirm`-Entscheidung für `action_id` auf, exakt wie ein normaler
`respond_to_action`-Aufruf mit `Approve`. Die neue Regel wirkt sich **nicht**
rückwirkend auf das gerade laufende Kommando aus — dessen Ausführung basiert
weiterhin auf der expliziten Nutzer-Bestätigung in diesem Moment, nicht auf
der neuen Regel. Erst künftige, ähnliche Vorschläge profitieren automatisch.

## 4. UI

Im Bestätigungsdialog: zusätzlicher Button "Akzeptieren und Regel erstellen ▾"
neben den bestehenden "Ausführen"/"Ablehnen"-Buttons. Klick öffnet ein
kompaktes Dropdown mit den Vorschlägen aus `suggest_rule_patterns` (Label +
Pattern-Vorschau), daneben eine Scope-Auswahl (Default: **aktueller Server**,
nicht Global — sicherere Voreinstellung, Nutzer kann auf Global/Tag
umstellen, gleiche Scope-Auswahl-Komponente wie im Regel-Formular aus Spec
0009). Klick auf einen Vorschlag ruft `accept_and_create_rule` auf und
schließt den Dialog.

## 5. Offene Punkte

- Soll die neu erstellte Regel-Aktion (`Allow`) fest sein, oder soll der
  Nutzer im selben Dropdown auch `Confirm` als Aktion wählen können (z. B.
  "ich will das nicht automatisch, aber der Dialog soll sich das Muster
  merken")? Aktuell nur `Allow` vorgesehen, da `Confirm` als Regel gegenüber
  dem bereits bestehenden Default-Fallback keinen echten Mehrwert böte.
