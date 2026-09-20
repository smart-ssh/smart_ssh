# Spec: Prompt-Caching (Rate-Limit-Hebel + Kostensenkung)

Status: Entwurf
Repo: **öffentlich** `smart_ssh`, `crates/ai-providers` (Anthropic-Request-
Aufbau, `cache_control`) + `crates/app-shell`/commands.rs (Prompt-Reihenfolge)
Abhängigkeiten: KI-Provider (0006), System-Prompt (commands.rs), Session-Modell
(0057, Kompaktierung), Rate-Limit-Gate (0061), Fencing (0039)

> **Der größte Hebel gegen das Rate-Limit — größer als das Header-Gate (0061).**
> Recherche-Befund (Anthropic-Doku, verifiziert): Für die meisten Claude-
> Modelle zählen **nur nicht-gecachte** Input-Tokens gegen das **ITPM**-Limit.
> `cache_read_input_tokens` zählen **NICHT**. Offizielles Beispiel: 2M ITPM +
> 80% Cache-Trefferquote = effektiv 10M Input-Tokens/Minute.
>
> Für uns: Die **große Server-Notiz** + System-Prompt + Tool-Definitionen gehen
> bei *jedem* Request mit. Gecacht zählen sie **null** gegen ITPM — nur der
> neue Teil (letzte Nachricht, neues Kommando-Ergebnis) zählt. Das adressiert
> den Rate-Limit-Schmerz **an der Wurzel**, statt ihn zu verwalten (0061).
>
> **Priorität ERHÖHT** (Request-Aufbau — berührt Fencing/Redaction-Reihenfolge
> und den frisch gebauten Kompaktierungs-/Gate-Pfad).

## Wichtig: Caching ist ein ARCHITEKTUR-Prinzip, kein Schalter

Der Cache ist ein **Präfix-Match**: Anthropic sucht rückwärts vom Breakpoint
das längste passende Präfix. **Jede Änderung im Präfix invalidiert alles
danach.** Die Cache-*Disziplin* (Reihenfolge + Stabilität) ist deshalb der
eigentliche Inhalt dieser Spec — nicht das Setzen von `cache_control`.

## 1. Prompt-Aufbau nach Änderungshäufigkeit ordnen

**Selten Veränderliches nach vorne, häufig Veränderliches nach hinten.**
Ziel-Reihenfolge im Request:
1. **System-Prompt** (ändert sich praktisch nie) — ganz vorne
2. **Tool-Definitionen** (stabil, solange der Tool-Satz konstant bleibt)
3. **Server-Notiz / Server-Kontext** (ändert sich selten, aber groß — der
   Haupt-Cache-Gewinn!)
4. **Gesprächsverlauf** (wächst, ändert sich ständig) — hinten
5. **Neue Nachricht / neues Kommando-Ergebnis** — ganz hinten

**Kläre und berichte**, wie der Request aktuell aufgebaut ist und was
umgestellt werden muss.

## 2. Cache-Killer beseitigen (die kritischen Punkte)

- **Datum/Zeit/`uname`-Banner NICHT in den System-Prompt.** Wenn dort ein
  Datum oder ein sitzungsspezifisches Banner steht, **invalidiert jede
  Sitzung/jeder Tag den gesamten Cache**. Solche Zustandsangaben als
  **Nachricht** einfügen (nach dem stabilen Präfix). (Deckt sich mit dem
  offenen Backlog-Punkt „Banner über `fence_untrusted` führen".)
- **Tool-Satz konstant halten**: Den Werkzeug-Satz **nicht** mitten in einer
  Sitzung ändern (z. B. je nach Modus andere Tools). Prüfe, ob das irgendwo
  passiert.
- **Kein Modellwechsel mitten in der Sitzung** — jedes Modell hat seinen
  eigenen Cache. (Zweitmeinung/Auto-Titel laufen ohnehin als separate Aufrufe
  mit eigenem Cache — das ist ok, nur innerhalb *einer* Aufruf-Kette nicht
  wechseln.)
- **Prüfe weitere Präfix-Instabilitäten**: Alles, was sich pro Request ändert
  und *vor* dem Gesprächsverlauf steht, ist ein Cache-Killer. Liste mir auf,
  was du findest.

## 3. `cache_control`-Breakpoints setzen

- Cache-Breakpoint(s) im Anthropic-Request setzen (nach dem stabilen Präfix —
  typischerweise nach System-Prompt + Tools + Notiz).
- **Anthropic-spezifisch**: Die neuere API liest automatisch vom längsten
  passenden Präfix, man muss nicht manuell tracken. Prüfe gegen die
  tatsächlich genutzte API-Version, wie viele Breakpoints sinnvoll sind.
- **Mindestlänge beachten**: Caching lohnt/greift erst ab einer Mindest-
  Token-Zahl (modellabhängig). Kurze Prompts profitieren nicht — prüfen und
  ggf. nur ab einer Schwelle cachen.

## 4. Andere Provider

- **OpenAI-kompatibel**: OpenAI cacht automatisch (kein `cache_control`
  nötig), andere Provider ggf. gar nicht. **Kein Bruch** für sie — die
  Reihenfolge-Optimierung (§1) hilft dort ebenfalls (OpenAI nutzt auch
  Präfix-Matching), schadet nie.
- **Ollama/lokal**: irrelevant (keine Token-Kosten, kein Rate-Limit), aber die
  Umstellung darf dort nichts brechen.

## 5. Sichtbarkeit: Cache-Wirkung messen

Ohne Messung weiß niemand, ob es greift:
- Die Antwort-Felder `cache_read_input_tokens` /
  `cache_creation_input_tokens` / `input_tokens` **loggen** (sie stehen in der
  `usage`-Sektion jeder Anthropic-Antwort).
- Damit ist die **Trefferquote sichtbar** — und Stefan kann prüfen, ob die
  Cache-Disziplin wirkt oder ob etwas das Präfix bricht.
- Passt zum frisch gebauten `stop_reason`-Logging (0063) und den
  Rate-Limit-Headern (0061) — dieselbe „mach das Unsichtbare sichtbar"-Linie.

## 6. Wechselwirkung mit Kompaktierung (0057) — WICHTIG

Die Kompaktierung **ändert den Gesprächsverlauf** (alte Runden → Summary).
Das invalidiert den Cache **ab der Änderungsstelle** — aber weil der
Gesprächsverlauf *hinten* steht (§1), bleibt das stabile Präfix (System +
Tools + Notiz) **gecacht**. Genau deshalb ist die Reihenfolge so wichtig.
- **Prüfe**: Bricht die Kompaktierung/Notiz-Kürzung das Präfix? Wenn die
  **Notiz** verkürzt gesendet wird (0057 §4.1), ändert sich ein *Präfix*-Teil
  → Cache-Miss für den ganzen Rest. Das ist ein realer Konflikt zwischen
  Kompaktierung und Caching. **Berichte mir, wie das zusammenspielt** und ob
  die Notiz-Kürzung dadurch teurer wird als sie spart.
- (Claude Code löst das durch „Kompaktierung cache-schonend: gecachten Aufruf
  forken statt separaten ungecachten Aufruf bauen" — prüfe, ob das hier
  anwendbar ist.)

## Invarianten / Sicherheit
- **Fencing und Redaction bleiben unverändert** — die Umstellung ändert nur
  die *Reihenfolge*/Struktur des Requests, nicht was redigiert/gefenced wird.
  Der Notiz-Fence (0039) bleibt.
- Kein Secret im Cache-Logging (nur Token-Zahlen).
- Die Umstellung darf den Kompaktierungs-/Gate-Pfad (0057/0061) nicht brechen.

## Testbarkeit
- Request-Reihenfolge: stabiles Präfix vorne, Volatiles hinten (Struktur-Test).
- Kein Datum/Banner im System-Prompt (Regressionstest — das ist der
  schlimmste Cache-Killer).
- `cache_control` gesetzt; `usage`-Felder werden geloggt.
- Andere Provider brechen nicht.
- Fencing/Redaction unverändert (bestehende Tests bleiben grün).

## Abschluss
- `spec-reviewer` ERHÖHT.
- CHANGELOG: „Prompt-Caching senkt Kosten und **Rate-Limit-Last** deutlich
  (gecachte Tokens zählen bei den meisten Modellen nicht gegen ITPM)."
- Melde mir: den Ist-Aufbau des Requests + was umgestellt wurde, alle
  gefundenen Cache-Killer (§2), das Zusammenspiel mit der Notiz-Kürzung (§6 —
  der potenzielle Konflikt), und wie Stefan die Trefferquote im Log abliest.
