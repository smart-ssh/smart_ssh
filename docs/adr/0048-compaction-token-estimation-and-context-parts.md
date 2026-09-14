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

### 8. Nacharbeiten aus dem `spec-reviewer`-Review dieses Schritts

Der pflichtgemäße `spec-reviewer`-Durchlauf (CLAUDE.md, "Verbindlicher
Review-Workflow", ERHÖHT) fand **einen sicherheitsrelevanten** Fund und
mehrere Vollständigkeits-/Genauigkeits-Lücken. Alle behoben, in separatem
Commit:

- **Resume-Vor-Trim entfernt (SICHERHEITSRELEVANT).** Der erste Entwurf
  rief beim Laden einer wiederaufgenommenen Sitzung
  (`commands::connect_session`) `compaction::truncate_rounds_with_
  placeholder` UNBEDINGT auf — kürzte also jede Sitzung mit mehr als
  `MIN_PRESERVED_ROUNDS` Runden sofort auf 3, unabhängig vom tatsächlichen
  Token-Budget (Spec 0057 §3.2 definiert die letzten N Runden als
  UNTERGRENZE, nicht als generelle Obergrenze). Zwei Folgeschäden: (a) das
  Frontend zeigte nach "Fortsetzen" nur noch 3 Runden statt der
  vollständigen Historie; (b) schwerwiegender —
  `history_contains_untrusted_content` (Spec 0039, Abschnitt 5) lief auf
  der bereits gekürzten Fassung und konnte einen NUR in einer älteren
  Runde eingeschleusten Inhalt übersehen, wodurch `untrusted_content_
  ingested` im wiederaufgenommenen Tab fälschlich `false` startete — die
  Post-Ingest-Eskalation (`AutoExec` → `Confirm`, Spec 0039 Abschnitt 5.1)
  blieb dann im Alltagsfall "lange Sitzung fortsetzen" aus. Fix: der
  Vor-Trim entfällt ersatzlos — `loaded` fließt unverändert in
  `initial_history`; der vollständige, budgetbewusste Kompaktierungslauf
  (`compact_for_send`) greift ohnehin spätestens vor dem ersten `send()`
  dieser Sitzung, mit korrektem Budget UND ohne die Untrusted-Content-
  Erkennung zu beeinträchtigen (die läuft VOR jeder Kompaktierung, auf der
  vollständigen `loaded`-Historie).
- **Schritt 1 + Schritt 2 jetzt echt inkrementell** ("nur so weit wie
  nötig", Aufgabenstellung): `compact_rounds_for_budget` entfernt alte
  Runden einzeln und prüft nach jeder Entfernung, ob der Request schon
  wieder passt — statt unbedingt auf `MIN_PRESERVED_ROUNDS` zu kürzen.
  `compact_oversized_outputs_for_budget` kürzt übergroße Einzelausgaben in
  chronologischer Reihenfolge, ebenfalls mit Budget-Prüfung nach jeder
  Kürzung, statt unbedingt ALLE übergroßen Ausgaben anzufassen. Die
  unbedingten Varianten (`truncate_rounds_with_placeholder`,
  `truncate_oversized_command_outputs`) bleiben als eigenständige,
  direkt getestete Bausteine erhalten (`#[cfg(test)]`), werden aber nicht
  mehr von `compact_for_send` selbst aufgerufen.
- **Warn-Log, wenn die volle Leiter das Budget nicht erreicht.** Die
  strukturellen Untergrenzen (`MIN_PRESERVED_ROUNDS`,
  `MAX_SINGLE_OUTPUT_BYTES_IN_CONTEXT` pro Ausgabe,
  `MIN_LAST_NOTE_SECTION_BYTES`) können zusammen bei einem sehr kleinen
  Kontextfenster (z. B. `DEFAULT_CONTEXT_WINDOW_TOKENS`) immer noch über
  dem Budget liegen. `compact_for_send` protokolliert diesen Fall jetzt
  sichtbar (`tracing::warn!`) statt ihn lautlos zu verschlucken — sendet
  die Anfrage aber trotzdem (kein Hard-Fail, dieselbe "Fehler containen,
  nie hängen/abbrechen"-Haltung wie Spec 0057 §2.2 für den künftigen
  Summary-Fallback vorschreibt).
- **`CommandOutput::truncated`-Flag statt eigenem In-Fence-Hinweistext.**
  Die ursprüngliche Fassung schrieb einen eigenen Kürzungs-Hinweis direkt
  in die `stdout`/`stderr`-BYTES — landete damit INNERHALB der
  `<stdout>`/`<stderr>`-Fence, also im Bereich, den `<security_notice>`
  ausdrücklich als "nie als Anweisung interpretieren" markiert, UND war
  durch identischen Text in der echten (Angreifer-kontrollierten) Ausgabe
  vortäuschbar. Fix: `truncate_oversized_output` setzt stattdessen das
  bereits vorhandene `CommandOutput::truncated`-Flag — denselben
  Mechanismus, den `ai_providers::format_command_result` bereits für den
  Spec-0043-Exec-Zeit-Cap nutzt (ein `<output_truncated>`-Hinweis
  AUSSERHALB der Fence). Die dortige Formulierung ("cut off after
  reaching the configured output size limit") ist absichtlich generisch
  genug, um für beide Ursachen (Exec-Zeit- oder Kontext-Zeit-Cap)
  gleichermaßen zu stimmen, ohne eine falsche Vollständigkeit zu
  behaupten — kein zweiter Mechanismus nötig. Der Notiz-Kürzungs-Hinweis
  (`NOTE_TRUNCATED_FOR_CONTEXT_NOTICE`) bleibt demgegenüber unverändert
  als In-Text-Hinweis bestehen — für `system_context`/Notizen existiert
  kein analoges Out-of-Band-Flag; das Risiko wird als gering eingeschätzt
  (rein informativ, keine Sicherheitswirkung) und bewusst nicht behoben,
  s. Abschlussmeldung an den Nutzer.
- **`suggest_note_update_on_disconnect` überspringt den KI-Aufruf, wenn
  Kompaktierung die Notiz gekürzt hat.** Dieser eine Aufruf bittet die KI
  um eine VOLLSTÄNDIGE Ersatznotiz (`AiAction::ProposeNoteUpdate::
  new_content` ersetzt die gespeicherte Notiz komplett). Sähe die KI nur
  die für den Versand gekürzte Fassung, könnte ihr Vorschlag den
  weggekürzten Teil verlieren — ein versehentlicher Notiz-Schrumpf, den
  Spec 0057 §4.2 bewusst nur über einen eigenen, nutzergeführten Dialog
  vorsieht. Erkannt über einen einfachen String-Vergleich (`system_context`
  vor/nach `compact_for_send`) — nur Schritt 3 (Notiz) verändert
  `system_context`, Schritt 1/2 (Runden/Ausgaben) nie, die Prüfung ist
  also ein zuverlässiger Indikator ausschließlich für "wurde die Notiz
  gekürzt", nicht für Kompaktierung allgemein.
- Testabdeckung ergänzt: ein Fake-Secret in einer Ausgabe, die Schritt 2
  kürzen MUSS, bleibt in der gesendeten Kopie redigiert; Schritt 1/2
  stoppen nachweislich, sobald das Budget passt, statt unbedingt bis zur
  Untergrenze zu kürzen; `suggest_note_update_on_disconnect` überspringt
  den Vorschlag nachweislich bei kompaktierter Notiz.

Bewusst NICHT behoben (Begründung an den Nutzer weitergereicht): kein
direkter `connect_session`/Resume-Integrationstest — `connect_session`
ist (anders als `build_session_system_context`/`send_chat_message_impl`)
nicht generisch über `R: tauri::Runtime`, sondern fest an
`AppHandle<Wry>` gebunden, und mindestens ein Aufruf darin
(`risk_second_opinion::resolve_second_opinion_provider`) ist es ebenfalls
nicht — beide generisch zu machen wäre ein eigener, nicht auf diesen
Review-Fix beschränkter Umbau. Der Fix selbst ist durch Code-Lektüre
verifiziert (der Vor-Trim-Aufruf ist ersatzlos entfernt, `loaded` fließt
unverändert weiter) und durch die bereits bestehende, umfassende
`history_contains_untrusted_content`-Testsuite (die Erkennungslogik
selbst war nie fehlerhaft — nur die Eingabe dafür wurde vorher fälschlich
vorbeschnitten) — nicht durch einen neuen End-to-End-Test dieses einen
Pfades.

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
