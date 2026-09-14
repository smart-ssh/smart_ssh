# 0048 — Kompaktierung (Spec 0057, Etappe 2): Token-Schätzung, Kontextfenster, `SystemContextParts`

## Status

Angenommen

## Kontext

Spec 0057 §3 (Kompaktierung) + §4.1 (Notiz-Handling) verlangen, dass die
App vor jedem KI-Provider-Request die tatsächlich zu sendende Größe
schätzt (Post-Fencing), bei ~70–80 % des Modell-Kontextfensters
kompaktiert, und dabei alte Chat-Runden, Riesen-Einzelausgaben und zuletzt
die Notiz in dieser Reihenfolge kürzt — ohne die gespeicherte Notiz oder
das Ledger (Spec 0057 Etappe 1) je zu verändern. Die Spec-Abschnitte
lassen mehrere Umsetzungsdetails offen, die während der Implementierung
von Etappe 2 entschieden werden mussten.

## Entscheidungen

### 1. Token-Schätzung: ~4 Byte/Token-Heuristik statt echtem Tokenizer

`compaction::estimate_tokens` teilt die Byte-Länge eines Strings durch 4
(aufgerundet) — dieselbe grobe Heuristik-statt-Tokenizer-Abwägung wie beim
abgelösten `chat_context_truncation::DEFAULT_CHAR_BUDGET` (ADR 0029), nur
nicht mehr als feste Zeichenzahl, sondern als Prozentsatz eines
modellabhängigen Kontextfensters ausgedrückt. Ein echter, providerspezifischer
Tokenizer wurde bewusst NICHT gebaut: unterschiedliche Provider/Modelle
verwenden unterschiedliche Tokenizer, ein exakter Nachbau für jeden wäre
unverhältnismäßiger Aufwand für eine Größe, die ohnehin nur einen
PROAKTIVEN Sicherheitsabstand vor dem harten Limit braucht (der
`COMPACTION_TRIGGER_RATIO`-Puffer fängt Schätzungenauigkeit ohnehin ab).

**Post-Fencing korrekt gerechnet** (Spec 0057 §3.1, der eigentliche
Diagnose-Fund): `estimate_message_tokens` ruft für `MessageContent::
CommandResult` dieselbe `fence_untrusted`-Funktion (Spec 0039) mit
denselben Parametern auf, die `ai_providers::{anthropic,
openai_compatible}::format_command_result` beim tatsächlichen Versand
verwendet — misst also das TATSÄCHLICHE Post-Fencing-Volumen, statt die
Expansion nachzubilden/zu schätzen. `format_command_result` selbst ist
providerintern und wird nicht exportiert; der feste Tag-Rahmen darum
(`<command_execution_result>`, `<exit_code>`, `<security_notice>` etc.)
wird stattdessen über eine konservative Konstante
(`COMMAND_RESULT_WRAPPER_TOKEN_OVERHEAD`) geschätzt.

### 2. Modell-Kontextfenster: kleine Namens-Heuristik + konservativer Default

`compaction::model_context_window_tokens(provider_type, model)` — es gibt
keine Modell-Registry in dieser Codebasis (`AiProviderConfig.model` ist
ein freier `String`, s. `persistence_sqlite::AiProviderConfig`). Eine
kleine, bewusst NICHT erschöpfende Namens-Zuordnung für die aktuell
gängigsten Modelle (Anthropic: 200k pauschal; OpenAI: 16k für
`gpt-3.5*`, sonst 128k) deckt den Regelfall ab. Für
`GenericOpenAiCompatible`/`Ollama` (selbstgehostete Modelle, deren
tatsächliches Kontextfenster von der lokalen Konfiguration abhängt und
von hier aus prinzipiell nicht bekannt sein kann) sowie jeden unbekannten
Modellnamen bei einem bekannten Provider gilt ein konservativer,
bewusst KLEIN gewählter Default (`DEFAULT_CONTEXT_WINDOW_TOKENS =
32_000`) — die "sichere" Fehlrichtung ist immer "zu klein annehmen", nie
"zu groß": ein zu großzügig angenommenes Fenster ist genau der Fall, der
den in Spec 0057 diagnostizierten Hänger verursacht hat, während ein zu
klein angenommenes Fenster im schlimmsten Fall etwas zu früh kompaktiert
— harmlos gegenüber der Alternative.

### 3. `SystemContextParts`: Notiz-Sektionen getrennt statt aus dem fertigen String zurückgeparst

Spec 0057 §4.1 verlangt scope-priorisierte Notiz-Kürzung (server-
spezifisch bleibt am längsten, gruppen-/globale zuerst). Vor Etappe 2 war
`system_context` ein einziger, bereits zusammengesetzter String
(`commands::build_session_system_context`). Zwei Optionen standen offen:

- (a) die Notiz-Abschnitte aus dem fertigen `system_context`-String anhand
  fester Marker (`"## Notizen / Kontext\n"`, `"## Remote-System\n"`)
  zurückparsen, oder
- (b) die Rohbestandteile (Basis-Text, `(Label, Notiztext)`-Paare
  UNGEFENCT, Remote-OS-Info) von Anfang an getrennt halten.

(a) wurde verworfen: die Fencing-Escaping-Garantie (`escape_for_prompt_fence`,
Spec 0039) schützt nur `<`/`>`/`&`, NICHT `#`/Zeilenumbrüche — eine
absichtlich in eine Notiz eingeschleuste Zeile wie `"\n\n## Remote-
System\n"` könnte einen String-Marker-Scan täuschen (zusätzlicher
"Abschnitt", der gar keiner ist, oder ein falsch erkanntes Ende). Für die
Kompaktierungslogik selbst (nicht die KI-Sicherheit, die bleibt unberührt)
wäre das ein Robustheits-/Korrektheitsrisiko. Entscheidung: (b) —
`compaction::SystemContextParts { base, note_sections, remote_os_info }`,
mit `assemble()`/`assemble_with_notes()` als der einzigen Stelle, die den
finalen String baut (identische Logik zum vorherigen Inline-Code in
`build_session_system_context`). `Session` hält `system_context_parts`
jetzt parallel zu `context.system_context` (dem bereits zusammengesetzten
String), aktualisiert an denselben zwei Stellen (`connect_session`,
`send_chat_message_impl`).

Nebeneffekt: `send_chat_message_impl`s vorherige Ermittlung von
`remote_os_info` (per String-Suche nach `"## Remote-System\n"` im
bestehenden `system_context`) wurde durch einen direkten Lese-Zugriff auf
`session.system_context_parts.remote_os_info` ersetzt — dieselbe
Robustheits-Überlegung wie oben, hier gleich mitbehoben statt eine zweite,
noch verbliebene Marker-Parsing-Stelle stehen zu lassen.

### 4. "Runde" = ab jeder `Role::User`-Nachricht bis zur nächsten

Spec 0057 §3.2 spricht von "Runden", ohne den Begriff für die
Chat-History-Datenstruktur zu definieren. `compaction::split_into_rounds`
gruppiert `history` an `Role::User`-Grenzen — dieselbe informelle
Bedeutung, die im Code bereits an anderer Stelle verwendet wird
("automatische Folgerunde", `orchestration::MAX_AUTO_FOLLOWUP_ROUNDS`).

### 5. Platzhalter-Nachricht: `Role::ActionResult`, nicht `User`/`Assistant`

Der beim Abschneiden alter Runden eingefügte Hinweis
("[Hinweis: ältere Konversation gekürzt ...]") bekommt `Role::
ActionResult` — dieselbe Kategorie wie `CommandResult`/`ActionRejected`
(beide ebenfalls vom Backend eingefügte, nicht vom Menschen/der KI
stammende Einträge, beide auf der Provider-Wire-Rolle "user" abgebildet,
s. `ai_providers::role_str`). Vermeidet außerdem, dass eine Kompaktierung
ausgerechnet die einzige verbleibende `Role::User`-Nachricht entfernt, auf
die z. B. `generate_session_title_on_disconnect`s `has_user_message`-
Prüfung angewiesen ist.

### 6. Ehrliche Kürzungs-Hinweise (Aufgabenstellung, Spec 0057 §3.2 Punkt 2)

Der Hinweistext für eine einzeln gekürzte Riesen-Ausgabe behauptet NICHT
"vollständig im Ledger" — der Transport-Output-Cap (Spec 0043/0044, 2 MB)
greift bereits beim Ausführen, VOR dem Ledger; bei einer wirklich riesigen
Ausgabe hat auch das Ledger nur die dort bereits gekappte Fassung. Der
Text sagt stattdessen "bis zur Ausführungs-Größengrenze erhalten, nicht
notwendigerweise vollständig".

### 7. Notiz-Kürzung: erst Sektionen entfernen, dann die letzte kürzen (nie entfernen)

`compaction::compact_notes_for_budget` entfernt zuerst ganze
Notiz-Sektionen von Index 0 an (allgemeinster Gruppen-Scope), bis nur noch
die letzte (server-spezifische) übrig ist — dann wird NUR NOCH diese eine
Sektion gekürzt (Text abgeschnitten, nie ganz entfernt), mit einer
Untergrenze (`MIN_LAST_NOTE_SECTION_BYTES`), damit auch im Extremfall ein
sinnvoller Rest ankommt. `parts.note_sections` selbst (die gespeicherte,
vollständige Fassung) wird dabei nie verändert — nur eine lokale Kopie.

## Konsequenzen

- Löst zusammen mit Etappe 1 (Ledger) den in Spec 0057 diagnostizierten
  Kontext-Hänger (s. `orchestration::tests::
  test_immich_case_large_note_and_long_history_stays_under_budget`).
- Ersetzt `chat_context_truncation`/ADR 0029 vollständig (Modul gelöscht).
- Die Modell-Kontextfenster-Tabelle ist bewusst unvollständig/best-effort
  — ein neues, noch nicht gelistetes Modell bei einem bekannten Provider
  fällt auf den provider-typischen Wert zurück (z. B. 128k für ein neues
  OpenAI-Modell), kein harter Fehler, aber auch keine Garantie auf
  Genauigkeit.
- Etappe 3 (Summary) ersetzt den in Schritt 1 eingefügten
  Platzhalter-Hinweis durch eine echte KI-Zusammenfassung — die
  Rundenkürzungslogik (`split_into_rounds`/`truncate_rounds_with_placeholder`)
  bleibt dabei die Grundlage, nur der Platzhalter-Text wird durch einen
  Summary-Aufruf ersetzt.
