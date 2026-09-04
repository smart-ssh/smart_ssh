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

**Korrektur nach dem verbindlichen `spec-reviewer`-Review dieses Schritts:**
eine erste Fassung dieses ADR begründete Option 2 (s. u.) fälschlich damit,
ein generiertes Dokument werde gar nicht persistiert ("kein sichtbarer
Chat-Eintrag im persistierten Sinn"). Das ist **sachlich falsch** —
`orchestration::handle_document_generated` ruft `push_history` mit dem
vollständigen `content_markdown` als `Role::Assistant`/
`MessageContent::Text` auf:

```rust
// Spec 0012, Abschnitt 5: "wird als Teil der Assistant-Nachricht in
// context.history übernommen (wie ein normaler Chat-Text)" — kein
// Sonderfall gegenüber `flush_text_buffer` oben, derselbe
// `Role::Assistant`/`MessageContent::Text`.
push_history(
    session,
    ChatMessage {
        role: Role::Assistant,
        content: MessageContent::Text(content_markdown),
    },
)
```

Ein generiertes Dokument landet also sehr wohl in `chat_messages` — nur
eben als ganz gewöhnlicher `content_type = 'text'`-Eintrag, nicht als
eigener `'document'`-Typ. Was tatsächlich (laut Spec 0012, Abschnitt 2/3,
und dem Kommentar an der `GenerateDocument`-Verzweigung in
`run_one_round`) NICHT passiert, ist: kein Filter-Engine-Durchlauf, kein
`handle_action_proposed`-Bestätigungsdialog — das war die eigentlich
gemeinte, korrekte Aussage, nur an der falschen Stelle (Persistenz statt
Bestätigung) angewendet. Die Kernaussage dieses ADR ändert sich dadurch
nicht: `content_type_for` deckt weiterhin nur drei der vier `CHECK`-Werte
ab, `'document'` bleibt unerreichbar — nur die Begründung für Option 2
unten ist entsprechend korrigiert.

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
   Variante erweitern, und `handle_document_generated` diese statt
   `MessageContent::Text` verwenden lassen, damit ein generiertes Dokument
   als eigener `content_type = 'document'`-Eintrag statt als gewöhnlicher
   `'text'`-Eintrag persistiert wird (persistiert wird es — s. Korrektur
   oben — bereits heute, nur untypisiert). Der Nutzen wäre rein
   struktureller Natur: eine wiederaufgenommene Sitzung könnte dann
   erkennen "dies war ein generiertes Dokument" und es entsprechend anders
   darstellen (z. B. mit erneuten Export-Buttons statt als reinen
   Fließtext) — genau das leistet die aktuelle `Text`-Behandlung nicht,
   `resume_chat_session`/`get_chat_history` liefern ein früher generiertes
   Dokument nach dem Wiederaufnehmen ununterscheidbar von einer normalen
   KI-Textantwort zurück. Spec 0012 selbst verlangt diese Unterscheidung
   aber nirgends (die dort getroffene, weiterhin gültige Entscheidung
   betrifft nur Filter-Engine/Bestätigungsdialog, s. o.) — eine eigene
   `Document`-Variante wäre eine über den bestehenden Spec-Text
   hinausgehende **neue** Funktion (bessere Wiederaufnahme-Darstellung),
   keine bloße Aufräumarbeit an einem toten Enum-Wert, und damit nicht
   Teil dieses "kleiner Fix"-Schritts.
3. **Unverändert lassen, dokumentiert als bewusste Reserve.** Der Wert
   kostet nichts (eine `CHECK`-Klausel ist keine Laufzeit-Belastung,
   keine zusätzliche Spalte) und hält die Tür offen für eine *künftige*
   Spec-Entscheidung, generierte Dokumente doch in die Historie
   aufzunehmen (z. B. für eine "Dokument war Teil dieser wiederaufgenommenen
   Sitzung"-Anzeige) — ohne dann zusätzlich noch eine Schema-Migration zu
   brauchen, nur um den erlaubten Wertebereich wieder zu erweitern.

Gewählt: **Option 3.** Der Aufwand für Option 1 (Tabellen-Rebuild) steht
in keinem Verhältnis zum Nutzen (ein nie erreichter Enum-Wert stört
weder Korrektheit noch Sicherheit), und Option 2 wäre eine eigenständige,
über Spec 0012 hinausgehende neue Funktion (bessere Darstellung
generierter Dokumente nach dem Wiederaufnehmen) — eine vom Nutzer zu
treffende Produktentscheidung, kein stiller Nebeneffekt dieses
"kleiner Fix"-Schritts.

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
