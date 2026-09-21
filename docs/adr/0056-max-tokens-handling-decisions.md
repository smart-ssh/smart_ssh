# ADR 0056: Entscheidungen bei der Umsetzung von Spec 0065 (max_tokens-Handling)

Status: Angenommen
Bezug: docs/specs/0065-max-tokens-handling.md, Commits `58da435`, `18872c7`,
`bfb4558`, `96bb188` (erste Fassung) plus Review-Nacharbeit (dieser Commit)

## Kontext

Spec 0065 lässt an mehreren Stellen bewusst Interpretationsspielraum
("Vorschlag 16.384", "z. B. 32k", "z. B. 8192"). Ein `spec-reviewer`-Review
(ERHÖHT, adversarial) der ersten Fassung fand außerdem zwei
sicherheitsrelevante Lücken und mehrere Korrektheitsfehler in den
Modell-Maximum-Tabellen, die vor Abschluss der Spec behoben wurden. Diese
ADR hält die dabei getroffenen Entscheidungen fest.

## 1. Default strikt kleiner als das Modell-Maximum

**Entscheidung**: `anthropic_default_max_tokens`/
`openai_compatible_default_max_tokens` liefern einen Wert, der IMMER
strikt kleiner ist als `anthropic_model_max_output_tokens`/
`openai_compatible_model_max_output_tokens` — konkret die Hälfte des
Maximums für die meisten Buckets, außer dem 128K-Bucket (Default 32.000,
näher an der Spec-Vorschlagszahl).

**Warum**: Die erste Fassung setzte Default == Maximum. Der einmalige
Retry aus Spec 0065 §3 ("verdoppelt, bis zum Modell-Maximum") hatte dadurch
keinen Spielraum mehr — er schickte bei einem abgeschnittenen Tool-Call
faktisch denselben Body ein zweites Mal, ohne die Erfolgschance zu
erhöhen. Sicherheitsmäßig unproblematisch (nichts wurde ausgeführt), aber
die in der Spec vorgesehene Abhilfe war wirkungslos. Gefunden durch das
ERHÖHTE Review, per Test abgesichert
(`test_default_max_tokens_always_leaves_headroom_below_the_model_maximum`
in beiden Providern).

## 2. Allowlist statt Denylist für stop_reason/finish_reason

**Entscheidung**: Ob ein Tool-Call als vollständig gilt, wird jetzt über
eine Allowlist bekannter Erfolgs-Werte entschieden
(`end_turn`/`tool_use`/`stop_sequence` bei Anthropic, `stop`/`tool_calls`
bei OpenAI-kompatibel) statt über eine Denylist ("ist es exakt
`max_tokens`/`length`?").

**Warum**: Mit der ursprünglichen Denylist ließ jeder abweichend
geschriebene oder fehlende `stop_reason`/`finish_reason` (ein Gateway mit
`"Length"`/`"MAX_TOKENS"`, ein Verbindungsabbruch nach `content_block_stop`
aber vor `message_delta`) einen abgeschnittenen, aber zufällig parsebaren
Tool-Call durch. Das widerspricht der projektweiten Regel "Eskalation, nie
Aufweichung" für sicherheitskritische Module (CLAUDE.md). Mit der Allowlist
gilt im Zweifel — unbekannter Grund, fehlender Grund, Verbindungsabbruch —
immer "abgeschnitten", nicht "vollständig". Nebenwirkung: ein Gateway, das
gar keinen `finish_reason` liefert (denkbar bei manchen selbstgehosteten
OpenAI-kompatiblen Servern im Fallback-Modus), löst jetzt bei jeder
erkannten Aktion einen Retry aus, der ebenfalls ohne `finish_reason`
enden und dann sichtbar fehlschlagen kann. Bewusst in Kauf genommen: die
Sicherheitsinvariante wiegt schwerer als ein seltener zusätzlicher
Fehlerfall bei einem nicht-konformen Gateway.

## 3. Claude-3.x- und OpenAI-Fallback-Richtung

**Entscheidung**: `claude-3*`-Modellnamen fallen jetzt auf den
konservativen Unbekannt-Fallback (8192) statt auf den 4.x-Bucket (64K).
Bei OpenAI-kompatibel wurde die Fallback-Richtung umgedreht: nur explizit
als aktuelle Generation erkannte Namen (`gpt-5*`/`gpt-6*`/`o3*`) bekommen
128K, jeder unbekannte Name fällt auf den konservativen Wert (4096) zurück.

**Warum**: Beides waren echte Korrektheitsfehler (potenzieller 400 "über
dem Maximum" bei einem real konfigurierten Modell), keine bewussten
Design-Entscheidungen — daher hier nur als Korrektur dokumentiert, nicht
als offene Abwägung.

## 4. `max_completion_tokens` nur für die offizielle OpenAI-API

**Entscheidung**: Reasoning-Modelle (`o1`/`o3`/`o4`/`gpt-5*`) bekommen das
Token-Limit über `max_completion_tokens` statt `max_tokens` — aber NUR,
wenn `base_url` nachweislich `api.openai.com` ist.

**Warum**: Vor Spec 0065 setzte dieser Provider überhaupt kein
Token-Limit-Feld. Das unbedingte Senden von `max_tokens` (Commit 2 der
ersten Fassung) hätte auf der echten OpenAI-API bei Reasoning-Modellen
einen 400 ausgelöst ("Unsupported parameter") — eine Regression. Für
generische Gateways/Ollama bleibt `max_tokens` unverändert: der
Modellname ist dort frei wählbar, ein Gateway kann denselben Namen unter
dem klassischen Feld erwarten, und dieser Provider kann von hier aus nicht
wissen, ob ein per Gateway erreichtes "o1"-artig benanntes Modell
tatsächlich die OpenAI-Reasoning-Parameter-Konvention erwartet.

## 5. Bewusst NICHT behoben (Review-Funde mit niedrigerer Priorität)

- **Kein UI-Hinweis während des unsichtbaren Retries** (Spec 0065 §3
  erlaubt das ausdrücklich: "ggf. über die bestehende Status-Anzeige" —
  "ggf." wurde als Kann-Option gelesen, nicht als Pflicht).
- **Text-Duplikation nach einem Retry im Haupt-Chat**: bereits gestreamter
  Text vor einem abgeschnittenen Tool-Call bleibt sichtbar, auch wenn der
  Retry eine neue, leicht andere Texteinleitung generiert — dokumentiert
  im Commit-Kommentar von `58da435` als bewusste, dem Streaming-Modell
  geschuldete Design-Entscheidung (ein Zurückziehen bereits gestreamter
  `TextDelta`s ist mit dem aktuellen Event-Modell nicht vorgesehen).
- **Notiz-Kürzung akzeptiert eine durch `TextTruncated` beendete Antwort
  kommentarlos als Ergebnis** (`orchestration.rs`, `summarize_note_for_
  shrink`) — inkonsistent zur eigenen, expliziten Kürzungs-Hinweiszeile
  der App, aber der Diff wird ohnehin vom Nutzer bestätigt, kein
  Sicherheitsproblem. Als eigenständige, kleine UX-Verbesserung für einen
  späteren Schritt vorgemerkt statt hier mit hineingezogen.
- **`o3`-Output-Maximum nicht unabhängig gegen eine Live-Dokumentationszeile
  verifiziert** (nur konservativ in denselben 128K-Bucket wie die übrige
  aktuelle Generation eingeordnet, statt in den generellen
  Unbekannt-Fallback) — sollte beim nächsten Kontakt mit aktueller
  OpenAI-Dokumentation nachgezogen werden.
