# 0031-chat-messages-document-content-type-unused

## Status
Vorgeschlagen

## Kontext

`chat_messages.content_type` trägt eine `CHECK`-Einschränkung mit vier
erlaubten Werten (Migrationen `0008_chat_sessions.sql`,
`0009_chat_messages_content_blob.sql`):

```sql
content_type TEXT NOT NULL CHECK (
    content_type IN ('text', 'command_result', 'action_rejected', 'document')
)
```

`persistence-sqlite::chat_session_store::content_type_for` — die einzige
Stelle, die tatsächlich einen `content_type`-Wert erzeugt — deckt aber nur
drei der vier `MessageContent`-Varianten ab:

```rust
fn content_type_for(content: &MessageContent) -> &'static str {
    match content {
        MessageContent::Text(_) => "text",
        MessageContent::CommandResult { .. } => "command_result",
        MessageContent::ActionRejected { .. } => "action_rejected",
    }
}
```

`MessageContent` selbst hat gar keine `Document`-Variante — `'document'`
im `CHECK` ist seit Einführung der Tabelle (Spec 0034) totes Vokabular,
kein tatsächlich erreichbarer Zustand. Der Sitzungs-Abgleich (Spec 0040)
verlangt hier explizit eine getroffene und dokumentierte Entscheidung
(entfernen oder tatsächlich produzieren), statt den Leerlauf weiter
stillschweigend mitzuschleppen.

## Entscheidung

**`'document'` bleibt im `CHECK` erhalten — keine Migration, um es zu
entfernen — aber aus genau diesem Grund dokumentiert, nicht weil es
übersehen wurde.**

Abgewogene Optionen:

1. **Entfernen** (Tabelle neu aufbauen wie in Migration 0009, `CHECK`
   ohne `'document'`). Reiner Aufräum-Effekt, kein funktionaler Gewinn —
   und ein weiterer TEXT/BLOB-Tabellen-Rebuild allein für einen nie
   erreichten Enum-Wert ist Migrations-Churn ohne Gegenwert.
2. **Tatsächlich produzieren**: `MessageContent` um eine `Document`-
   Variante erweitern und generierte Dokumente (Spec 0012,
   `AiAction::GenerateDocument`) in `chat_messages` persistieren. Das
   widerspricht aber einer bereits getroffenen, in Spec 0012 Abschnitt 2
   und im `handle_document_generated`-Doc-Kommentar (`orchestration.rs`)
   festgehaltenen Design-Entscheidung: ein generiertes Dokument läuft
   *nicht* durch die Filter-Engine, *keinen* Bestätigungsdialog, "kein
   sichtbarer Chat-Eintrag" im persistierten Sinn — es ist bewusst reiner,
   lokaler, exportierbarer Inhalt, keine dauerhaft gespeicherte Chat-
   Nachricht. Diese Option würde also eine andere, bereits bewusst
   getroffene Spec-0012-Entscheidung rückgängig machen — nicht Teil
   dieses "kleiner Fix"-Schritts.
3. **Unverändert lassen, dokumentiert als bewusste Reserve.** Der Wert
   kostet nichts (eine `CHECK`-Klausel ist keine Laufzeit-Belastung,
   keine zusätzliche Spalte) und hält die Tür offen für eine *künftige*
   Spec-Entscheidung, generierte Dokumente doch in die Historie
   aufzunehmen (z. B. für eine "Dokument war Teil dieser wiederaufgenommenen
   Sitzung"-Anzeige) — ohne dann zusätzlich noch eine Schema-Migration zu
   brauchen, nur um den erlaubten Wertebereich wieder zu erweitern.

Gewählt: **Option 3.** Der Aufwand für Option 1 (Tabellen-Rebuild) steht
in keinem Verhältnis zum Nutzen (ein nie erreichter Enum-Wert stört
weder Korrektheit noch Sicherheit), und Option 2 würde eine bereits
bewusst getroffene, unabhängige Design-Entscheidung aus Spec 0012 im
Vorbeigehen revidieren.

## Konsequenzen

**Positiv:**
- Keine Migrations-Churn für einen rein kosmetischen Aufräumeffekt.
- Erweitert `MessageContent` künftig um eine `Document`-Variante (sollte
  eine spätere Spec das explizit verlangen), ist keine zusätzliche
  Schema-Migration mehr nötig — der `CHECK` erlaubt den Wert bereits.

**Negativ / Trade-off:**
- `'document'` bleibt ein für jeden Leser des Schemas verwirrender,
  scheinbar erreichbarer, tatsächlich toter Wert, bis entweder Option 1
  oder 2 künftig bewusst nachgeholt wird. Dieses ADR ist der Hinweis
  dafür — ein künftiger Leser, der sich fragt, wo `'document'` erzeugt
  wird, findet hier die Antwort ("nirgends, bewusst so"), statt danach
  suchen zu müssen.
