# 0054 — Proaktive Rate-Limit-Drosselung (Spec 0061): Identitäts-Schlüssel & Zusammenspiel

## Status

Angenommen

## Kontext

Spec 0061 verlangt, die `anthropic-ratelimit-*`-Response-Header zu lesen
und ein geteiltes Budget pro Provider-Identität proaktiv gegen Rate-Limits
zu schützen. Die Spec verlangt ausdrücklich, den Identitäts-Schlüssel-
Ansatz vor der Umsetzung zu klären und zu berichten (§2, "der Kern") —
dieser ADR hält diese Entscheidung sowie das Zusammenspiel mit Pacing
(Spec 0051)/Retry, die Degradierung header-loser Provider und die
"kein unbegrenztes Hängen"-Garantie fest.

## Entscheidungen

### 1. Identitäts-Schlüssel: Base-URL + Modell + Hash(API-Key)

`ai_providers::provider_identity_key(base_url, model, api_key) -> String`
kombiniert:

- **Base-URL** (normalisiert, ohne Nachlaufslash) — deckt den Fall ab,
  dass derselbe Key auf zwei unterschiedliche Endpunkte zeigt (z. B. ein
  unternehmensinterner Proxy vs. der offizielle Endpoint).
- **Modell** (exakter Name, nicht "Modell-Klasse") — Anthropic trennt
  Limits pro Modell-Klasse; da keine verlässliche Modell-Klassen-Tabelle
  vorliegt, ist der exakte Modellname eine konservative Näherung: im
  schlimmsten Fall entstehen zwei getrennte Wächter für zwei Modelle, die
  intern dasselbe Kontingent teilen (verschenktes, aber sicheres Wissen —
  die App drosselt dann pro Modell einzeln, statt vom gemeinsamen Budget
  zu profitieren), nie umgekehrt ein fälschlich geteiltes Budget zwischen
  echt getrennten Kontingenten.
- **Ein Hash des API-Keys** (`DefaultHasher`) statt des Keys selbst im
  Klartext — der Identitäts-String landet als Schlüssel in einer
  `HashMap` und könnte im Zweifel geloggt/inspiziert werden; der Hash
  vermeidet eine zweite Klartext-Kopie eines Secrets über die bereits im
  `AnthropicProvider`/`OpenAiCompatibleProvider` gehaltene hinaus.
  **Korrektur (spec-reviewer, Follow-up-Review):** die ursprüngliche
  Begründung hier war sachlich falsch — `DefaultHasher::new()` verwendet
  feste (nicht zufällige) Schlüssel und ist damit über Aufrufe UND
  Prozess-Neustarts hinweg stabil; das ist hier sogar eine erwünschte
  Eigenschaft (derselbe Provider-Identitäts-String über mehrere
  Sitzungen hinweg), keine Einschränkung. `DefaultHasher` ist trotzdem
  die richtige Wahl — aber weil hier keine kryptografische Stärke nötig
  ist (kein Angreifer-Modell, in dem ein Hash-Kollisionsangriff auf
  diesen internen `HashMap`-Schlüssel relevant wäre), nicht weil er
  instabil/kollisionsanfällig wäre.

Berechnet wird der Schlüssel zentral in `ai_provider_factory::
build_ai_provider` — dem EINZIGEN Konstruktions-Trichter für jeden
`Box<dyn AiProvider>` in der App (Spec 0022, Abschnitt 3: "der Key wird
beim Aufbau der Instanz einmalig gelesen") — bevor der entschärfte
`String`-Key im Provider verschwindet. `build_ai_provider` gibt seither
ein `(Box<dyn AiProvider>, Arc<ProviderBudgetGuard>)`-Paar zurück statt
nur des Providers.

**Ergebnis (automatisch, ohne Sonderfall-Code)**: ruft ein Nutzer
`resolve_second_opinion_provider` zweimal auf (einmal für die Risiko-
Zweitmeinung, einmal für den Einschleusungs-Check — beide lesen
`riskClassifierProviderId`, s. `risk_second_opinion.rs`), liefert
`RateLimitRegistry::guard_for` für denselben Schlüssel denselben `Arc`
zurück — das geteilte Budget aus Spec 0061 Abschnitt 2 ergibt sich rein
aus der Registry-Semantik, nicht aus explizitem Session-Code, der
"Haupt- und Zweitmeinungs-Provider könnten denselben Key haben" erkennen
müsste.

### 2. Registry lebt auf `AppState`, Wächter-Handles auf `Session`

`ai_providers::RateLimitRegistry` (ein `Mutex<HashMap<String,
Arc<ProviderBudgetGuard>>>`) wird einmal pro App-Prozess auf `AppState`
gehalten (`build_app_state()`), NICHT pro Session — sonst würde jede neue
Session ihre eigene, leere Registry bekommen und nie von den Headern
profitieren, die eine VORHERIGE Session für dieselbe Provider-Identität
bereits gelesen hat.

`Session` bekommt trotzdem eigene Felder (`ai_provider_budget: Arc<...>`,
`risk_second_opinion_budget`/`injection_check_budget: Option<Arc<...>>`)
statt nur eine Referenz auf die Registry — Begründung: `Box<dyn
AiProvider>` (die Trait-Grenze, s. Abschnitt 3 unten) bietet nach der
Konstruktion keinen Weg mehr, an den intern gehaltenen Wächter
heranzukommen, und `wait_for_rate_limit_budget` (das Gate, s. Abschnitt 4)
braucht ihn VOR dem `send()`-Aufruf, außerhalb der `AiProvider`-Instanz.
Die Handles werden einmal bei der Provider-Konstruktion (`connect_session`
bzw. `resolve_second_opinion_provider`) neben dem Provider selbst
gespeichert — derselbe `Arc`, den der Provider intern auch hält, nur ein
zweiter, günstiger Klon davon.

### 3. Header-Lesen bleibt in `ai-providers`, das Warten (Gate) in `app-shell`

Zwei mögliche Orte für die Warte-Logik standen zur Wahl: innerhalb der
`AiProvider::send()`-Implementierung (`ai-providers`) oder davor, im
Aufrufer (`app-shell::orchestration`). Entschieden für **Letzteres**:

- Der Token-Schätzer (`compaction::estimate_request_tokens`, Spec 0057
  §3.1 — "denselben Schätzer wiederverwenden", wörtliche Vorgabe von Spec
  0061 §3) lebt bereits in `app-shell` und operiert auf `SessionContext`.
  `ai-providers` dürfte laut Architektur-Regel nicht von `app-shell`
  abhängen (nur umgekehrt) — die Logik dorthin zu verschieben oder zu
  duplizieren wäre unnötig gewesen.
- Das UI-Warte-Event (`emit_ai_budget_waiting`) nutzt das bestehende
  `EventEmitter`-Muster aus `app-shell::events` — `ai-providers` kennt
  keine Tauri-/UI-Schicht und soll das laut Architektur-Regel auch nicht
  kennenlernen.

Das **Lesen** der Header (Teil 1 der Spec) bleibt dagegen zwingend in
`ai-providers`: die `reqwest::Response`-Header sind nur dort erreichbar,
zwischen `let response = client.post(...).send().await` und dem Punkt, an
dem `response.text()`/`sse_frame_stream(response)` sie konsumiert — bzw.
`map_http_status` sie beim Umwandeln in `AiError::RateLimited` (Unit-
Variante) endgültig verwirft. `AnthropicProvider::send()` ruft
`budget.record_headers(...)` deshalb auf JEDER Antwort (Erfolg, 429, jeder
andere Fehlerstatus) auf, bevor irgendein Zweig den `response`-Wert
konsumiert — ein einziger Aufruf-Ort direkt nach `let response = ...`
deckt alle drei Pfade gleichzeitig ab.

`ProviderBudgetGuard` selbst ist reiner Werte-Zustand (ein `Mutex` um vier
Zähler), lebt bewusst in `ai-providers` (dort, wo die Header-Struktur am
besten bekannt ist), wird aber vollständig `Send + Sync`/plain-data
gehalten, sodass sowohl `ai-providers` (schreibt) als auch `app-shell`
(liest, vor dem Send) denselben `Arc` sicher nutzen können.

### 4. Zusammenspiel mit Pacing (Spec 0051) und reaktivem Retry

Drei unabhängige Schutzschichten, in dieser Reihenfolge vor jedem
`AiProvider::send()`:

1. `wait_for_ai_request_slot` (Spec 0051, unverändert) — reines
   Mindest-Pacing (300ms) zwischen aufeinanderfolgenden Aufrufen
   DERSELBEN Session, unabhängig von Rate-Limit-Wissen.
2. `wait_for_rate_limit_budget` (NEU, Spec 0061) — proaktiv, basiert auf
   den zuletzt gelesenen Headern des zur jeweiligen `AiProvider`-Instanz
   gehörenden Wächters.
3. Das bestehende reaktive 429-Retry (`crate::retry`, Spec 0051) —
   unverändert, bleibt Sicherheitsnetz für den Fall, dass Schätzung/
   Header mal danebenliegen (z. B. ein Limit, das sich seit der letzten
   Antwort geändert hat, oder der allererste Aufruf einer Provider-
   Identität, für die noch nie Header gelesen wurden).

Keine der drei Schichten ersetzt eine andere — Spec 0061 verlangt das
explizit ("ergänzt, ersetzt nicht").

### 5. Degradierung header-loser Provider

`OpenAiCompatibleProvider` (deckt OpenAI, generische OpenAI-kompatible
Endpunkte UND Ollama ab) bekommt zwar ein `budget`-Feld (für eine
einheitliche `build_ai_provider`-Signatur ohne Typ-Fallunterscheidung),
ruft aber **niemals** `record_headers` auf — es gibt keine verlässliche,
providerübergreifende Header-Konvention für diese Familie (anders als bei
Anthropic). `ProviderBudgetGuard::wait_duration` prüft zuerst ein
`has_any_header_data`-Flag und liefert `None` (sofort senden), solange
dieses Flag nie gesetzt wurde — ein header-loser Provider wird dadurch
STRUKTURELL nie blockiert, ganz ohne Sonderfall-Prüfung an der
Gate-Aufrufstelle.

### 5a. Scope-Reduktion: `Retry-After` fließt (noch) nicht in den Wächter ein

Spec 0061 §1 nennt `retry-after` in der Liste der zu berücksichtigenden
Header, §2 verlangt zusätzlich: "bei einem 429 aktualisiert sich [der
Wächter] (Budget = 0 bis Reset)". Umgesetzt ist bislang nur der `anthropic-
ratelimit-*`-Pfad (2. spec-reviewer-Runde hat diese Lücke explizit
benannt — hier nachträglich dokumentiert, statt sie stillschweigend zu
lassen).

**Praktische Folge:** ein 429, der KEINE `anthropic-ratelimit-*`-Header
mitschickt (z. B. ein Proxy/Gateway, das den 429 selbst erzeugt, bevor die
Anfrage Anthropic erreicht, oder ein OpenAI-kompatibler Endpunkt, der
generell keine dieser Header kennt), hinterlässt im Wächter weiterhin
keinerlei neues Wissen. Der reaktive 429-Retry (Abschnitt 4, Schicht 3)
fängt DIESEN einen Aufruf trotzdem ab — aber ein zweiter, unmittelbar
folgender Aufruf mit dERSELBEN Provider-Identität (z. B. die Einschleusungs-
Prüfung direkt nach dem Haupt-Chat-Aufruf) weiß nichts von dem gerade
erlebten 429 und feuert ungebremst erneut. Das ist genau die
Doppel-Verbrauchs-Lücke, die das geteilte Budget aus Abschnitt 2
eigentlich schließen soll.

**Warum trotzdem vertretbar, aber offen:** der einzelne 429 selbst wird
nicht verschluckt (das reaktive Retry greift), und der Fall betrifft nur
header-lose 429-Quellen — gegen die echte Anthropic-API (die
`anthropic-ratelimit-*`-Header auch auf 429-Antworten mitschickt, s. Spec
0061 §1) tritt die Lücke nicht auf. Ein sauberer Fix (den `Retry-After`-
Wert als `remaining: 0`/`reset_at: now + retry_after` in den betroffenen
Zähler einspeisen) wurde zurückgestellt, weil er sorgfältig gegen bereits
vorhandene, ggf. präzisere `anthropic-ratelimit-*`-Daten abgewogen werden
müsste (ein zu kurzer `Retry-After` darf ein länger laufendes echtes
Limit nicht verkürzen) — das ist ein eigenständiger, kleiner Folge-Schritt,
kein Fund, der diesen Schritt ungültig macht.

### 6. Kein unbegrenztes Hängen — mit Einschränkungen (aktualisiert nach Follow-up-Review)

`MAX_PROACTIVE_WAIT = 90s` (`ai_providers::rate_limit_budget`) deckelt
jede EINZELNE aus einem Reset-Zeitpunkt berechnete Wartedauer. Fälle, in
denen dieser Deckel statt eines Header-Reset-Werts greift:

- Reset-Header fehlt in der Antwort.
- Reset-Header ist unparsbar oder liegt bereits in der Vergangenheit
  (`chrono::DateTime::parse_from_rfc3339` schlägt fehl oder liefert eine
  negative Differenz zu "jetzt").
- Ein zuvor gelesener, damals noch zukünftiger Reset ist beim jetzigen
  Gate-Check inzwischen selbst verstrichen — seit dem Follow-up-Fix
  (spec-reviewer-Fund) wird dieser Fall NICHT mehr auf den 90s-Deckel
  abgebildet, sondern als "veraltete Daten, kein verlässliches Wissen"
  behandelt (kein Warten) — s. `Counter::is_stale`.

Nach Ablauf der (ggf. gedeckelten) Wartezeit sendet die App die Anfrage in
jedem Fall — liegt sie danach immer noch über dem Limit, fängt das
reaktive 429-Retry (Abschnitt 4, Schicht 3) das ab, mit seiner eigenen,
unabhängigen Obergrenze (`MAX_TOTAL_RETRY_TIME = 20s`, Spec 0051).

**Was diese Garantie NICHT abdeckt (spec-reviewer-Fund, bewusst nicht in
diesem Schritt behoben — s. Begründung unten):**

- **Kein kumulativer Deckel über mehrere Gates/Runden hinweg.** Eine
  einzelne automatische Fortsetzungs-Serie (Spec 0021,
  `MAX_AUTO_FOLLOWUP_ROUNDS`) kann pro Runde mehrfach gaten (Haupt-Chat,
  ggf. Kompaktierung, ggf. Zweitmeinung/Einschleusungs-Check) — bei
  wiederholt knappem Budget (z. B. ein Proxy, der dauerhaft ~5%
  Restbudget mit ~89s-Reset meldet) kann sich das über mehrere Minuten
  aufsummieren. `MAX_PROACTIVE_WAIT` deckelt jeden EINZELNEN Wartevorgang,
  nicht die Summe über eine Runden-Serie.
- **Das Warten ist nicht abbrechbar.** Es gibt aktuell keinen Weg, ein
  laufendes `tokio::time::sleep` im Gate von außen (z. B. über den
  bestehenden "Stop"-Button der Auto-Fortsetzung) zu unterbrechen — der
  Nutzer sieht das Warte-Event, kann es aber nicht direkt abbrechen,
  sondern nur die gesamte Sitzung trennen.

**Warum nicht in diesem Schritt behoben:** beide Punkte verlangen eine
Session-/Abbruchsignal-Anbindung, die über die reine Gate-Logik
hinausgeht (ein Abbruch-Token oder eine `&Session`-Referenz müsste bis in
`wait_for_rate_limit_budget` durchgereicht werden, an allen 7
Aufrufstellen, inklusive der beiden, die strukturell KEINE lebende
`Session` haben — Auto-Titel-Erzeugung und Notiz-Kürzung nach
Verbindungstrennung, s. deren Doc-Kommentare). Das ist ein sinnvoller,
aber eigenständiger nächster Schritt (vermutlich zusammen mit dem in der
Aufgabenstellung explizit ausgeklammerten adaptiven Token-Bucket), kein
Fund, der die Kern-Invariante dieses Schritts ("normalerweise kein
unbegrenztes Hängen, reaktives Retry als Sicherheitsnetz") ungültig
macht — er beschreibt ein Worst-Case-Szenario unter einem dauerhaft
fehlkonfigurierten/adversariellen Proxy, nicht das Verhalten gegen die
echte Anthropic-API im Normalbetrieb.

### 7. Bewusst ungegatete Ausnahme: `test_ai_provider_credentials`

Der "Zugangsdaten testen"-Button in den Provider-Einstellungen
(`commands::test_ai_provider_credentials`) ruft `build_ai_provider` auf
und sendet eine Testanfrage, verwirft den zurückgegebenen
`Arc<ProviderBudgetGuard>` aber explizit (`let (provider, _budget) =
...`) statt ihn vor dem Send zu gaten. Das ist eine bewusste Ausnahme von
"vor JEDEM Send" (Spec 0061 Abschnitt 3), nicht ein Versehen: ein
Nutzer, der auf diesen Button klickt, erwartet eine sofortige
Rückmeldung ("funktioniert dieser Key?"), kein bis zu 90s langes,
unerklärtes Warten — und diese Testanfrage trägt ohnehin nicht zum
eigentlichen TPM-Verbrauch eines laufenden Chats bei (kein geteilter
Kontext, keine Wiederholung). (spec-reviewer-Fund, hier nachträglich
dokumentiert.)

## Konsequenzen

- Ein Nutzer, der versehentlich zwei unterschiedliche API-Keys für
  Haupt-Provider und Zweitmeinung einträgt, bekommt zwei unabhängige
  Budgets — korrekt, aber ohne die Querschutz-Wirkung, die bei
  identischem Key entstünde.
- `RateLimitHeaderSnapshot`/`RawCounter`/`ProviderBudgetGuard::
  record_headers` sind bewusst `pub` (nicht `pub(crate)`) gehalten, damit
  `app-shell`s Testsuite einen Wächter ohne echten HTTP-Mock gezielt in
  einen bekannten Zustand versetzen kann (Spec 0061, Testbarkeit) — diese
  API ist damit auch außerhalb von Tests nutzbar, falls ein künftiger
  Provider-Typ seine Header anders aufbereiten möchte.
- Reset-Zeitpunkte werden intern als `std::time::Instant` gehalten (nicht
  `tokio::time::Instant`) — robust gegen Systemuhr-Sprünge während der
  Wartezeit, aber dadurch in `#[tokio::test(start_paused = true)]`-Tests
  NICHT durch `tokio::time::advance` beeinflussbar; solche Tests müssen
  die tatsächliche (wenn auch kurze) Restlaufzeit über `tokio::time::sleep`
  unter der virtuellen Uhr durchlaufen lassen, s. `orchestration::tests`.
