# ADR 0057: Entscheidungen bei der Umsetzung von Spec 0066 (Chat unterbrechbar)

Status: Angenommen
Bezug: docs/specs/0066-chat-interrupt-and-sensitive-data-prompt.md, Commits
`89d0dc9`, `51439cb`, `caa622a`, Review-Nacharbeit `b2756e0`

## Kontext

Stefan hat zwei Entscheidungen getroffen (Einreihen statt Unterbrechen;
laufendes Kommando läuft beim Stopp weiter). Weitere Punkte ließ die Spec
offen, und ein `spec-reviewer`-Review (ERHÖHT, adversarial) fand Lücken, die
vor Abschluss behoben wurden. Diese ADR hält beides fest.

## 1. Stopp-Flag wird beim Turn-Start zurückgesetzt, atomar mit `running`

**Entscheidung**: Der Reset von `auto_continue_stop` passiert in
`send_chat_message_impl` unter dem `chat_turn`-Lock, im selben Schritt wie
`running = true`, vor jedem `.await`. Auch ein Folge-Turn aus der
Warteschlange setzt das Flag an derselben Stelle zurück.

**Warum**: Früher setzte `run_chat_turn` das Flag erst nach mehreren
DB-Zugriffen zurück. Seit der Stopp-Knopf ab dem ersten Moment sichtbar ist,
hätte ein schnell gedrückter Stopp so verschluckt werden können (Review-Fund).
Der Reset für Folge-Turns ist gewollt: eingereihte Nachrichten sollen auch
nach einem Stopp beantwortet werden (Spec 0066 §1).

## 2. Eingereihte Nachricht nach Stopp vor dem Versand → Folge-Turn

**Entscheidung**: Wird eine eingereihte Nachricht an der Rundengrenze in den
Verlauf gelegt und die Runde danach vor dem Versand gestoppt
(`RoundOutcome::StoppedBeforeSend`), startet `send_chat_message_impl` einen
Folge-Turn, der sie beantwortet.

**Warum**: Sonst stünde sie im Verlauf, ohne je bei der KI angekommen zu sein,
während der „wartet"-Hinweis schon verschwunden wäre (Review-Fund).

## 3. Stopp verhindert eine noch nicht gestartete Auto-Ausführung

**Entscheidung**: Ist ein AutoExec-Vorschlag schon aus dem Stream geholt
(z. B. während der Zweitmeinung), die Ausführung aber noch nicht gestartet,
verhindert ein Stopp sie. Die Karte meldet ein abgebrochenes Kommando
(`cancelled: true`, leere Ausgabe, Spec 0027). Das gilt nur für den eigenen
Chat (`ActionOrigin::Internal`), nicht für MCP-Aktionen.

**Warum**: Entscheidung 2 („laufendes Kommando läuft zu Ende") betrifft
Kommandos, die schon laufen. Eines, das noch nicht gestartet ist, soll nach
einem Stopp auch nicht mehr starten. Das ist nur eine Verschärfung: keine
Regel wird aufgeweicht. MCP ist ausgenommen, weil der Stopp dem Chat-Turn
gilt, nicht externen Clients.

## 4. Offener Bestätigungsdialog bleibt beim Stopp stehen

**Entscheidung**: Nicht automatisch ablehnen. Der Nutzer entscheidet selbst,
danach folgt keine weitere Runde.

**Warum**: Das ist das bestehende, per Test abgesicherte Verhalten aus
Spec 0021 §5. Ohne Bestätigung wird ohnehin nichts ausgeführt, ein
automatisches Ablehnen brächte also keinen Sicherheitsgewinn.

## 5. UI-Details, die die Spec offen ließ

- „Abgebrochen" ist ein eigenes, schlichtes Chat-Element aus dem
  `chat-response-cancelled`-Event, keine Markierung an der Antwort. Das
  funktioniert auch, wenn vor dem Abbruch noch kein Text kam.
- Der „wartet"-Hinweis wird für alle eingereihten Nachrichten zugleich
  entfernt: bei `chat-queued-messages-sent` oder sobald im Frontend kein
  Sendevorgang mehr offen ist. Das Backend übergibt immer die ganze
  Warteschlange auf einmal.
- „Weiter" (Spec 0065) wird ignoriert, solange eine Antwort läuft. Sonst
  würde die Fortsetzungs-Anweisung in einem fremden Turn landen.
- Eingereihte Nachrichten tragen kein zusätzliches Label im Text an die KI.
  Sie stehen als normale Nutzer-Nachricht nach dem Kommando-Ergebnis im
  Verlauf.

## 6. Bewusst NICHT behoben (Review-Funde)

- **Nicht abbrechbare Nebenaufrufe**: die Zusammenfassung in
  `compact_for_send`, die Risiko-Zweitmeinung und der Injection-Check laufen
  auch nach einem Stopp zu Ende. Der Stopp wirkt erst danach. Das
  sicherheitsrelevante Folgeproblem (Ausführung nach Stopp) ist durch §3
  geschlossen; der Rest kostet nur Wartezeit. Die Spec-Vorgabe „< 1 s" gilt
  deshalb für den Haupt-Stream, nicht für diese Nebenaufrufe.
- **Streng alternierende OpenAI-kompatible Backends** (manche
  llama.cpp-/Mistral-Templates) können zwei aufeinanderfolgende
  User-Nachrichten ablehnen. Das kam schon vorher vor (Nutzer-Nachricht nach
  einem Kommando-Ergebnis) und wird durch das Einreihen nur häufiger. Ein
  Zusammenführen im Provider wäre ein eigener Schritt.
- **Eingereihte Nachrichten gehen bei einem Absturz verloren**: Sie werden
  erst beim Übergeben persistiert. Stürzt die App vorher ab oder greift der
  Rücksetz-Wächter, sind sie weg bzw. laufen erst mit der nächsten
  Nachricht. Akzeptiert als seltener Randfall.
- **„Redaction vor Persistenz" für Nutzertext**: Nutzertext wird
  unredigiert, aber verschlüsselt gespeichert und erst vor dem Versand
  redigiert. Das gilt für alle Nutzer-Nachrichten und bestand schon vorher;
  eingereihte Nachrichten verhalten sich identisch. Keine Änderung in diesem
  Schritt.
- **Provider-Test zum Stopp während des Backoffs**
  (`test_dropping_stream_during_retry_backoff_sends_no_further_request`) ist
  schwach, weil ein nicht mehr gepollter Stream ohnehin keinen Retry
  auslöst. Er bleibt als Schutz gegen einen künftig per `tokio::spawn`
  losgelösten Retry, der Kommentar sagt das jetzt ehrlich. Dass der Stopp
  den Stream tatsächlich verwirft, prüft
  `test_stop_aborts_in_flight_ai_stream_immediately`.
