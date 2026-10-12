# ADR 0125 — Strukturiertes Urteilsfeld: synthetische statt aufgezeichneter Fixtures

Status: akzeptiert
Betrifft: Issue #104, Spec 0074 (§4.4), Spec 0026 (§3), Spec 0039 (§5.2)

## Problem

Das Issue verlangt, das Antwortformat vor dem Festlegen gegen
aufgezeichnete Antworten von Anthropic, OpenAI-kompatiblen Anbietern und
Ollama zu prüfen und diese als Test-Fixtures abzulegen. Aufzeichnen
heißt Live-Aufrufe mit Konten eines Maintainers; die Umsetzung kann das
nicht selbst.

## Entscheidung

1. Das Format `VERDICT: …` wird jetzt umgesetzt. Die Tests nutzen
   **ausdrücklich synthetische** Beispielantworten (als solche benannt).
2. Die Anforderung „aufgezeichnete Antworten" ist **aufgeschoben**, nicht
   erfüllt. Ein Folge-Issue nimmt echte Aufzeichnungen als Fixtures auf
   und passt das Format an, falls ein Anbieter es nicht befolgt.
3. Sicherheit: Hält sich ein Anbieter nicht an das Format, greift der
   Rückfall („höchste Stufe gewinnt", Spec 0074). Das Ergebnis ist dann
   nie niedriger als heute; es kann höchstens öfter eskalieren.
4. Neue Abhängigkeiten (vom Maintainer freigegeben):
   `unicode-normalization` (NFKC, schon im Lockfile) und `proptest`
   (nur Dev) für den Eigenschaftstest gegen den Rückfall.

## Folgen

Das Format kann nach der Aufzeichnung noch einmal angepasst werden.

## Nachtrag (Issue #223): Aufzeichnungen

Am 2026-10-12 hat ein Maintainer Antworten aufgezeichnet (je 7 Zweitmeinungs-
und 4 Injection-Check-Eingaben, ohne Geheimnisse oder echte Hosts) und als
Fixtures unter `crates/app-logic/tests/fixtures/` abgelegt:

- **Anthropic** (`claude-haiku-5-5`) und **Ollama** (`qwen2.5:0.5b`): alle 22
  Antworten beginnen exakt mit `VERDICT: <Wert>`; der Test liest sie über das
  strukturierte Feld, keine braucht den Rückfall. Das Format bleibt
  unverändert.
- Das kleine Ollama-Modell urteilt oft falsch (z. B. `env | grep -i token`
  → `none`, beide Injektionen → `no`). Das ist Modellqualität, nicht Format;
  ein so kleines lokales Modell ist keine brauchbare Zweitmeinung.
- **OpenAI-kompatibel** (gehosteter Endpunkt, `qwen/qwen3-32b`, aufgezeichnet
  am 2026-10-13): alle 11 Antworten folgen dem Format, die Urteile sind
  plausibel. Jede Antwort beginnt mit Leerzeilen, und die Urteilszeile endet
  mit zwei Leerzeichen; der strukturierte Parser verträgt beides, der Test
  deckt es mit diesem Fixture ab. Das Format bleibt unverändert.

Damit liegen Aufzeichnungen aller drei Anbietertypen vor, keine synthetische Antwort
wird mehr als Anbieterausgabe ausgegeben, und Requirement 6 aus #104 ist
**erfüllt**.
