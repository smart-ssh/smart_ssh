# 0042-rate-limit-retry-budget-and-request-pacing

## Status
Akzeptiert

## Kontext

Spec 0051 (Rate-Limit-Handling) beschrieb das Problem konkret (ein
Testphasen-Blocker: die KI "hört mitten in einer Sitzung auf zu
antworten", real reproduziert als HTTP 429 ohne Retry), ließ die genauen
Zahlen und den De-Bursting-Ansatz aber bewusst offen ("z. B. 3–5 Versuche",
"wähle den saubersten Ansatz, begründe").

Ein Teil-0-Diagnosebericht vor der Umsetzung stellte fest: alle
KI-Anfragen einer Session (Haupt-Chat-Runde, Risiko-Zweitmeinung,
Einschleusungs-Check, Auto-Titel, Notiz-Vorschlag beim Disconnect) waren
bereits strikt seriell — jede vollständig `.await`et, bevor die nächste
beginnt, keine Nebenläufigkeit. Der beobachtete "Burst" (Zweitmeinung und
Haupt-Request 31ms auseinander, beide vom Rate-Limit getroffen) war also
kein Nebenläufigkeitsproblem, sondern fehlender zeitlicher Abstand
zwischen an sich schon sequenziellen Aufrufen.

## Entscheidung

**1. Ein einziges, geteiltes Retry-Budget für alle Aufrufer (`crates/
ai-providers/src/retry.rs`), kein separates Budget pro Aufrufer.**

`AiProvider::send()` kennt seinen Aufrufer nicht — ob eine gegebene
Anfrage der sichtbare Haupt-Chat-Request ist oder ein unsichtbarer
Best-Effort-Nebencheck (Risiko-Zweitmeinung, Einschleusungs-Check), weiß
nur `app-shell::orchestration`. Ein pro-Aufrufer unterschiedliches
Retry-Budget hätte entweder die `AiProvider`-Trait-Signatur geändert
(betrifft jeden Provider-Impl inkl. aller Test-Doubles) oder das Budget
über `SessionContext` transportiert (ein Kern-Typ mit zehn bestehenden
Konstruktionsstellen quer über drei Crates). Für diesen Schritt
unverhältnismäßig gegenüber dem einfacheren Weg: ein Budget, klein genug
gewählt, dass auch der stille Nebenpfad es akzeptabel findet.

- **Versuchs-Obergrenze `MAX_ATTEMPTS = 4`** (1 Erstversuch + 3 Retries) —
  innerhalb des von der Spec genannten Rahmens ("3–5").
- **Gesamtzeit-Deckelung `MAX_TOTAL_RETRY_TIME = 20s`**, nicht die initial
  gewählten 60s. Ein unabhängiger `spec-reviewer`-Review dieses Schritts
  stellte fest, dass 60s für den Haupt-Chat-Request akzeptabel gewesen
  wären, aber dieselbe Schleife auch die beiden *inline im Chat-Turn
  awaiteten* Best-Effort-Nebenpfade durchläuft (`app_shell::
  risk_second_opinion::fetch_second_opinion`/`fetch_injection_check`, s.
  deren Aufrufstellen in `orchestration.rs`), die vor Spec 0051 bei einem
  429 sofort und lautlos aufgaben. Mit 60s hätte ein dauerhaftes
  Rate-Limit diese Nebenpfade neu zu einem bis zu minutenlangen, für den
  Nutzer unsichtbaren Stillstand gemacht — z. B. zwischen einem
  Bestätigungsklick und der tatsächlichen Ausführung, oder zwischen zwei
  Kommandos derselben Session. 20s hält dieses Worst-Case-Fenster
  spürbar kleiner, ohne dem Haupt-Request die von der Spec verlangte
  Deckelung zu nehmen.
- **Ein `Retry-After`, das länger ist als das verbleibende Zeitbudget,
  führt zum sofortigen Aufgeben, nicht zu `min(Retry-After, Budget)`
  warten und danach trotzdem senden** — ebenfalls ein Reviewer-Fund: der
  ursprüngliche Code kappte nur die Wartezeit, schickte den Request nach
  Ablauf des Budgets aber trotzdem, obwohl der Provider explizit "nicht
  vor Ablauf von X" gesagt hatte.

**2. De-Bursting über einen festen 300ms-Mindestabstand
(`Session::ai_request_paced_at` + `orchestration::
wait_for_ai_request_slot`), keine echte Warteschlange.**

Da alle Aufrufe bereits seriell sind, genügt ein einfacher
"seit-wann-lief-der-letzte-Request"-Zeitstempel pro Session, den jeder
der fünf `AiProvider::send()`-Aufrufe vorher konsultiert und ggf. auf ihn
wartet — keine Producer/Consumer-Queue, kein zusätzlicher
Hintergrund-Task. Der Mindestabstand gilt bewusst unabhängig von der
konkreten `AiProvider`-Instanz (Haupt- vs. separat konfigurierter
Zweitmeinungs-/Einschleusungs-Check-Provider), weil beide potenziell
denselben API-Key/dasselbe Rate-Limit-Kontingent teilen, ohne dass die
App das unterscheiden könnte. 300ms ist klein genug, um für den Nutzer
nicht spürbar zu sein, aber groß genug, um zwei App-intern ausgelöste
Anfragen aus demselben Burst zeitlich zu trennen.

**3. Keine `Retry-After`-HTTP-date-Unterstützung, nur die
delta-seconds-Form (RFC 9110).**

Beide angebundenen Provider-Familien (Anthropic, OpenAI-kompatibel)
senden in der Praxis ausschließlich die Sekunden-Form. Eine Datums-Form
zu parsen hätte entweder eine neue Abhängigkeit oder händisches
HTTP-Date-Parsing gebraucht, für einen Fall, der bei keinem angebundenen
Provider vorkommt. Ein vorhandener, aber nicht als Sekunden-Zahl
parsebarer Header fällt auf das Backoff zurück statt den Retry
abzubrechen — bewusste Scope-Reduktion, dokumentiert in `retry.rs`.

## Konsequenzen

- Der Haupt-Chat-Pfad bekommt bis zu 20s Geduld gegen ein 429, bevor er
  mit der neuen, handlungsanleitenden `AI_RATE_LIMITED`-Meldung aufgibt —
  spürbar besser als das vorherige sofortige, unerklärte Schweigen.
- Die beiden stillen Nebenpfade (Zweitmeinung, Einschleusungs-Check)
  können bei einem dauerhaften Rate-Limit weiterhin bis zu ~20s pro
  Aufruf blockieren, ohne dass die UI etwas anzeigt — strukturell
  dasselbe "stilles Warten"-Muster, das Spec 0051 eigentlich beseitigen
  wollte, nur mit einer kleineren, akzeptierten Obergrenze statt der
  vorherigen Sofort-Aufgabe. **Bewusst nicht vollständig behoben in
  diesem Schritt** (s. Entscheidung 1) — ein sauberer Fix bräuchte ein
  aufrufer-spezifisches Retry-Budget, was eine Erweiterung der
  `AiProvider`-Trait-Signatur oder von `SessionContext` erfordert.
  Nachzieher-Kandidat.
- Kein sitzungsweiter Circuit-Breaker: eine rate-limitierte Session mit
  einer vorgeschlagenen Aktion kann in einem einzigen Nutzer-Turn bis zu
  4 (Zweitmeinung) + 4 (Haupt) + 4 (Einschleusungs-Check) = 12 Requests an
  einen aktiv drosselnden Provider schicken, nur durch das 300ms-Pacing
  entzerrt. `ai_request_paced_at` wäre die naheliegende Stelle für einen
  künftigen Breaker (z. B. den Zeitstempel nach einem terminalen 429 in
  die Zukunft zu setzen). Nicht umgesetzt — außerhalb des Wortlauts von
  Spec 0051.
- Die "Zugangsdaten testen"-Funktion im KI-Provider-Formular
  (`crates/app-shell/src/commands.rs::classify_credential_test_result`)
  profitiert vom selben Retry (derselbe `AiProvider::send()`-Pfad), zeigt
  bei einem 429 aber weiterhin den rohen `AiError::Display`-Text statt der
  neuen übersetzten, handlungsanleitenden Meldung — dieser Pfad läuft
  nicht über `translateErrorCode`. Kleine, vorbestehende Inkonsistenz,
  nicht behoben (außerhalb des Wortlauts von Spec 0051, eigener kleiner
  Nachzieher).
