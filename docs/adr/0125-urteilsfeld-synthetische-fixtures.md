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
