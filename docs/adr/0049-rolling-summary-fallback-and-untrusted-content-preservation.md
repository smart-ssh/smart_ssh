# 0049 — Rollierende Zusammenfassung (Spec 0057, Etappe 3): Fallback, Persistenz, 0039-Erhalt

## Status

Angenommen

## Kontext

Spec 0057 §2 verlangt, dass Schritt 1 der Kompaktierungs-Leiter (Etappe 2:
altes Runden-Abschneiden mit Platzhalter) durch eine rollierende
KI-Zusammenfassung ersetzt wird — mit einem verpflichtenden, niemals
hängenden Fallback auf das Etappe-2-Verhalten, verschlüsselter Persistenz
(wie Chat-Historie/Ledger, Spec 0036) und ohne die in Etappe 2 gefundene
Post-Ingest-Eskalations-Regression (ADR 0048, Punkt 8) zu wiederholen. Die
Spec lässt mehrere Umsetzungsdetails offen, die während der Implementierung
entschieden werden mussten.

## Entscheidungen

### 1. `compact_for_send` wird `async`, nimmt jetzt `&Session`

Vor Etappe 3 war die gesamte Kompaktierungs-Leiter rein synchron (keine
I/O). Der Zusammenfassungs-Aufruf ist ein echter KI-Request — `compact_
for_send`/`compact_rounds_with_summary` sind deshalb jetzt `async fn` und
bekommen `session: &Session` (für `ai_provider`, `redactor`,
`wait_for_ai_request_slot`, den Lese-/Schreibzugriff auf `session.
summary`). `compaction.rs` bleibt trotzdem eine reine `app-shell`-interne
Datei (keine Grenzverletzung zu `core` — die core/app-shell-Trennung aus
CLAUDE.md betrifft Crate-Grenzen, nicht Modulgrenzen innerhalb von
`app-shell`).

Nebeneffekt (spec-reviewer-würdiger Fund, direkt beim Schreiben
vorweggenommen): alle drei Aufrufstellen (`run_one_round`,
`generate_session_title_on_disconnect`, `suggest_note_update_on_
disconnect`) hielten den `MutexGuard` von `session.system_context_parts`
bisher implizit über den GESAMTEN `compact_for_send`-Aufruf hinweg (ein
Rust-Temporary in Argumentposition lebt bis zum Ende der Anweisung). Mit
einem jetzt potenziell bis zu [`SUMMARY_CALL_TIMEOUT`] (120s) langen
Aufruf dahinter hätte das jeden gleichzeitigen Zugriff auf `system_
context_parts` (z. B. eine neue Nutzer-Nachricht in einem anderen
gleichzeitig offenen Tab derselben Sitzung) für die volle Dauer blockiert.
Behoben, indem an allen drei Stellen `SystemContextParts` (das `Clone`
ableitet) VOR dem Aufruf geklont und der Guard sofort freigegeben wird.

### 2. `RollingSummary` als app-shell-interner Typ, Persistenz über primitive Werte

Wie schon bei `LedgerEntryContent` (ADR 0047) und `SystemContextParts`
(ADR 0048) wird kein neuer `core`-Typ eingeführt — `RollingSummary { text,
rounds_covered }` lebt in `compaction.rs`. Die Persistenz-Schicht
(`persistence-sqlite`, die nicht von `app-shell` abhängen darf) kennt
diesen Typ folglich nicht: `SqliteChatSessionStore::save_summary`/`load_
summary` nehmen/liefern ein primitives `(&str, i64)`-Paar, `app-shell`
konvertiert an der Aufrufstelle. `rounds_covered` bleibt Klartext
(`INTEGER`) — reine Buchhaltung, keine vertraulichen Nutzdaten; nur `text`
wird verschlüsselt (Spec 0036-Muster, derselbe `chat_content_cipher` wie
Chat-Historie/Ledger).

Additive Migration `0012_chat_sessions_summary.sql`: zwei neue, nullable
Spalten (`summary_text BLOB`, `summary_rounds_covered INTEGER`) auf der
bestehenden `chat_sessions`-Tabelle statt einer neuen Tabelle — es gibt
immer höchstens EINE aktuelle Summary pro Sitzung (kein append-only-Bedarf
wie beim Ledger), ein `UPDATE` auf dieselbe Zeile ist die einfachste
korrekte Modellierung. Alte, vor Etappe 3 angelegte Zeilen bekommen beide
Spalten automatisch `NULL` (SQLite-`ALTER TABLE ADD COLUMN`-Default) und
bleiben unverändert lesbar.

### 3. Cut-Count-Bestimmung bleibt synchron/konservativ — EIN KI-Aufruf pro Kompaktierung, nicht mehrere

Etappe 2 bestimmte `cut_count` (wie viele alte Runden entfernt werden
müssen) durch eine inkrementelle Suche, die bei jedem Kandidaten die
resultierende Größe abschätzt. Für Etappe 3 wäre eine "echte" Suche
(mehrere tatsächliche Zusammenfassungs-Aufrufe, bis das Ergebnis passt)
unverhältnismäßig teuer/langsam. Stattdessen: `determine_round_cut_count`
bestimmt `cut_count` weiterhin rein synchron, unter der KONSERVATIVEN
Annahme, dass Schritt 1 im schlimmsten Fall (Zusammenfassung schlägt fehl)
den einfachen Etappe-2-Platzhalter verwendet — der damit ermittelte
`cut_count` passt garantiert unters Budget, auch ohne Zusammenfassung. Eine
ECHTE Zusammenfassung fällt praktisch immer kleiner aus als dieser
Platzhalter-Text plus die wegfallenden Runden, das tatsächliche Ergebnis
landet dann mit zusätzlichem Sicherheitsabstand unter dem Budget. Genau
EIN KI-Aufruf pro Kompaktierungslauf (nicht mehrere in einer Such-Schleife).

### 4. "Rollierend": `rounds_covered` als Fortschritts-Marker, nur das Delta wird gefaltet

`RollingSummary.rounds_covered` zählt, wie viele der ÄLTESTEN Runden
(gezählt von Index 0 in `split_into_rounds`s Ergebnis) bereits abgedeckt
sind — dieselbe Zählweise wie `cut_count`. Reicht die vorhandene Summary
(`rounds_covered >= cut_count`) schon aus, wird sie unverändert
wiederverwendet, KEIN KI-Aufruf. Reicht sie nicht, geht NUR das Delta
(`rounds[rounds_covered..cut_count]`) zusammen mit dem bisherigen
Summary-Text in den Aufruf — nie die schon zusammengefassten alten Runden
im Rohformat noch einmal, und nie die gesamte Historie von vorne (Spec
0057, §2.1, wörtlich).

### 5. Fallback: KEIN hybrides Teil-Ergebnis bei einem Fehlschlag

Schlägt der Zusammenfassungs-Aufruf fehl (Timeout, `AiEvent::Error`,
leere/Whitespace-Antwort), wird für DIESE eine Anfrage der volle,
generische Etappe-2-Platzhalter für den GESAMTEN `cut_count` verwendet —
nicht eine Mischung aus der noch gültigen alten (Teil-)Summary plus einem
generischen Hinweis für den Rest. `session.summary` selbst bleibt beim
Fehlschlag komplett unangetastet (kein Teil-Update, kein Datenverlust des
zuletzt gültigen Stands) — der nächste Kompaktierungsversuch (nächster
`send()`) startet wieder von genau diesem unveränderten Stand und
versucht die Zusammenfassung erneut. Bewusst einfach gehalten: "exakt das
Etappe-2-Verhalten" (Aufgabenstellung, wörtlich), keine zusätzliche
Fallback-Zwischenstufe.

### 6. Äußerer Zeitrahmen zusätzlich zu den Provider-eigenen Schutzmechanismen

Der Provider-Aufruf selbst trägt bereits Schutz (SSE-Inaktivitäts-Timeout
~90s, Rate-Limit-Retry-Budget ~20s, Spec 0051/Body-Timeout-Fix). Zusätzlich
umschließt `generate_rolling_summary` den GESAMTEN Aufruf mit einem
eigenen `tokio::time::timeout` (`SUMMARY_CALL_TIMEOUT = 120s`) — die
unabhängige Rückversicherung dagegen, dass irgendein unvorhergesehener
Zustand (z. B. ein Bug in einer einzelnen Provider-Implementierung, der
deren eigene Mechanismen umgeht) die Sitzung dennoch hängen lässt.
"Fehler containen", dieselbe Invariante wie beim ursprünglichen
Body-Timeout-Fix.

### 7. Redaction: defensive Re-Redaction in BEIDE Richtungen

Die zu faltenden Runden sind in der Praxis meist schon redigiert
(`CommandResult`-Inhalt läuft bereits beim Ausführen durch den Redactor,
s. `execute_suggested_command`) — `generate_rolling_summary` wendet
trotzdem zusätzlich dieselbe additive Re-Redaction an, die auch vor jedem
normalen `send()` läuft (`reapply_redaction_for_send`), weil
`MessageContent::Text`-Inhalt (z. B. KI-eigener Chat-Text) NIE automatisch
beim Ablegen in der Historie redigiert wird. Die ZURÜCKKOMMENDE
Zusammenfassung wird ebenfalls redigiert, bevor sie verwendet/gespeichert
wird ("wie normaler KI-Inhalt behandelt", Aufgabenstellung, wörtlich) —
nicht privilegiert, obwohl sie aus bereits redigiertem Material entstand
(ein Provider könnte ein zuvor durch das Redaction-Muster nicht erfasstes
Secret dennoch wörtlich wiederholen).

### 8. Untrusted-Content-Eskalation: strukturell erhalten, kein neuer Code nötig

`session.untrusted_content_ingested` ist ein `AtomicBool`, gesetzt
GENAU beim tatsächlichen Ingest (z. B. in `execute_suggested_command`,
unmittelbar beim Ausführen/Fenced-Einbetten), niemals aus `context.
history` zur Sendezeit abgeleitet, und niemals zurückgesetzt. Weder
`compact_for_send` noch `compact_rounds_with_summary`/`generate_rolling_
summary` lesen oder schreiben dieses Feld — die Eskalationsprüfung in
`handle_action_proposed` liest es direkt, unabhängig davon, was gerade im
(ggf. kompaktierten/zusammengefassten) Kontextfenster steht. Die
Etappe-2-Regression (ADR 0048, Punkt 8) betraf ausschließlich den
RESUME-Pfad (`history_contains_untrusted_content` lief dort versehentlich
auf einer bereits vorgekürzten `initial_history`) — dieser Pfad wurde in
Etappe 2 bereits korrigiert (kein Vor-Trim mehr) und von Etappe 3 nicht
wieder angefasst. Ein expliziter Regressionstest
(`test_untrusted_content_escalation_survives_round_summarization`)
verifiziert das dennoch strukturell: eine Runde mit untrusted Content wird
absichtlich aus dem Kontext heraus zusammengefasst, und eine danach neu
vorgeschlagene, an sich `AutoExec`-fähige Aktion muss trotzdem zu
`Confirm` eskalieren.

### 9. Nacharbeiten aus dem `spec-reviewer`-Review dieses Schritts

Der pflichtgemäße `spec-reviewer`-Durchlauf (CLAUDE.md, "Verbindlicher
Review-Workflow", ERHÖHT) fand keinen Sicherheits-Bruch der harten
Invarianten (Filter-Engine, Fencing, Credentials, `untrusted_content_
ingested`), aber mehrere Vollständigkeits-/Genauigkeits-/Testqualitäts-
Lücken. Behoben, in separatem Commit:

- **Fehlender Budget-Test.** Beim Verschieben der `compact_for_send`-Tests
  von `compaction::tests` nach `orchestration::tests` (Etappe 3 macht die
  Funktion `async`, braucht eine `&Session`) ging der eigentliche
  Etappe-2-Test verloren, der die Kernzusage der Leiter prüfte: "nach der
  Kompaktierung passt der Request unters Budget" (Spec 0057 §7). Wieder
  angelegt, jetzt für BEIDE Pfade (mit und ohne erfolgreiche
  Zusammenfassung).
- **Persistenz-Test prüfte nicht, was er behauptete.** Ein Kommentar
  sprach von "direktem SQL-Zugriff", der tatsächliche Test rief aber nur
  `save_summary`/`load_summary` über denselben Store auf — gegenüber
  fehlender Verschlüsselung blind. Ergänzt: ein echter Direkt-SQL-Test
  (`test_direct_sql_access_to_summary_text_column_never_reveals_plaintext`,
  analog zum bestehenden Muster für `chat_messages.content`) sowie ein
  Falscher-Schlüssel-Test, beide in `persistence-sqlite`. Der App-Shell-
  Test wurde zu einem echten End-zu-Ende-Beweis (`compact_for_send` →
  `persist_rolling_summary` → `load_summary`) umgebaut.
- **Obergrenze für die zurückgelieferte Zusammenfassung** (`SUMMARY_MAX_
  BYTES = 8_000`): ohne Cap könnte eine geschwätzige/fehlgeleitete
  Provider-Antwort über den Reuse-Zweig (`rounds_covered`) dauerhaft
  "klebrig" werden — größer als jede spätere Kompaktierung noch verkleinern
  könnte, genau die Klasse von Dauerhänger, die diese Etappe eigentlich
  beheben soll.
- **Additive Re-Redaction auch für den BISHERIGEN Summary-Text**, nicht
  nur für die neu gefalteten Runden — ein aus der DB geladener alter
  Summary-Text (ggf. mit einem älteren Redactor-Musterstand entstanden)
  lief bisher ungeprüft in den nächsten Zusammenfassungs-Aufruf.
- **`rounds_covered` beim Resume auf die tatsächlich geladene Rundenzahl
  geklemmt** (`compaction::round_count`) — ohne das hätte eine durch
  nicht-persistierte MCP-Aktionen (Spec 0034 §10) oder einen best-effort
  fehlgeschlagenen `append_message`-Aufruf inkonsistent gewordene
  Abdeckungszahl mehr Runden als "bereits abgedeckt" behandeln können, als
  überhaupt geladen wurden.
- **Zwei Testqualitäts-Lücken geschlossen**: der Redaction-Test prüfte
  bisher nur die eingehende Richtung (die zurückkommende Zusammenfassung)
  trotz seines Namens — um die ausgehende Richtung (der tatsächlich an
  den Provider gesendete, gefaltete Aufruf) ergänzt, plus ein eigener Test
  für die Re-Redaction des bisherigen Summary-Texts. Der Ledger-/Notiz-
  Test verließ sich stillschweigend auf `MockAiProvider`s bequemen "leere
  Queue → `[Done]`"-Fallback, wodurch ein Bug, der die Kompaktierung
  komplett überspringt, unbemerkt geblieben wäre — eine zusätzliche
  Assertion beweist jetzt unabhängig, dass tatsächlich eine
  Zusammenfassung erzeugt wurde.
- **Zwei fehlende, spec-kritische Testfälle ergänzt**: ein Provider-Stream,
  der ohne `Done`/`Error` einfach endet (muss wie ein Fehlschlag
  behandelt werden), und ein Provider, dessen Stream nie antwortet (muss
  nach `SUMMARY_CALL_TIMEOUT` zuverlässig auf den Fallback zurückfallen —
  mit `#[tokio::test(start_paused = true)]`/`tokio::time::advance`
  bewiesen, nicht nur behauptet).

Bewusst NICHT behoben, dem Nutzer explizit gemeldet:

- **MCP-Inhalt wird über die Summary doch persistiert.**
  `push_history_scoped`s `persist: false` schließt MCP-verursachte
  Einträge bewusst aus der persistierten, wiederaufnehmbaren Chat-Historie
  aus (Spec 0034 §10/Spec 0040 §4). `compact_rounds_with_summary` faltet
  aber `session.context.history` — das auch MCP-Einträge enthält, da jenes
  Flag nur die DB-Persistenz von `chat_messages` betrifft, nicht den
  In-Memory-Kontext — in eine Zusammenfassung, die anschließend
  verschlüsselt persistiert UND beim Resume wieder geladen wird. Verdichteter
  MCP-Inhalt landet damit doch in der persistierten Sitzung, nur in
  zusammengefasster statt roher Form. Ob das die Zusage aus Spec 0034 §10
  tatsächlich verletzt (die sich explizit auf "Historie" bezieht, nicht
  zwangsläufig auf jede abgeleitete Repräsentation), ist eine
  Produktentscheidung, die Stefan treffen muss, keine rein technische —
  nicht in dieser Review-Fix-Runde vorweggenommen.
- **Bis zu ~20 Minuten unsichtbarer Stillstand im Extremfall.**
  `compact_for_send` läuft in jeder automatischen Folgerunde
  (`MAX_AUTO_FOLLOWUP_ROUNDS = 10`); ein durchgehend hängender/
  rate-limitierter Provider könnte theoretisch bis zu zehn `SUMMARY_
  CALL_TIMEOUT`-Zyklen (120s) ohne jedes Frontend-Event durchlaufen, bevor
  die Sitzung wieder reagiert — kein Hang (die Sitzung kommt zuverlässig
  wieder zurück), aber eine lange UI-Stille, die dem Geist von Spec 0057
  §2.2 nahekommt. Eine saubere Lösung bräuchte ein neues Chat-Event
  ("Kontext wird zusammengefasst…") — größerer Umfang als ein reiner
  Review-Fix, für eine spätere Iteration vorgemerkt statt hier
  hineingezogen.
- **Seltene, nebenläufige Kompaktierung** (zwei gleichzeitige `send()`-
  Aufrufe derselben Sitzung, z. B. Tab + MCP, könnten beide eine
  Zusammenfassung erzeugen und sich gegenseitig überschreiben) — Folge ist
  ausschließlich verschwendete Kosten/ein Abdeckungs-Rückschritt, kein
  Datenverlust/keine Sicherheitslücke; als akzeptierter Kompromiss
  belassen.

## Konsequenzen

- Genau ein zusätzlicher KI-Aufruf pro Kompaktierungslauf (nur wenn
  Schritt 1 tatsächlich greift und die vorhandene Summary nicht schon
  ausreicht) — kein Mehraufwand für den Regelfall (Anfrage unter dem
  Auslöser-Schwellwert).
- Ein dauerhaft fehlschlagender Summary-Provider degradiert die Sitzung
  spürbar (jede Kompaktierung fällt auf den knapperen Etappe-2-Platzhalter
  zurück), blockiert sie aber nie — genau die geforderte Eigenschaft.
- Etappe 4 (Sitzungsende-Notiz-Dialog) ist von dieser Etappe unberührt;
  keine der hier getroffenen Entscheidungen muss dafür revidiert werden.
