# ADR 0072: Kein leerer Anthropic-System-Block — Entscheidungen bei der Umsetzung

Status: Angenommen
Bezug: docs/specs/0081-anthropic-empty-system-block.md

Spec 0081 gibt vor, das `system`-Feld im Anthropic-Request-Body komplett
wegzulassen, wenn der gebaute System-Text leer oder reiner Leerraum ist
(A1–A3). Die Umsetzung folgt der Spec wörtlich; dieses ADR hält zwei
Punkte aus dem zweirundigen `spec-reviewer`-Review fest, die bewusst nicht
(weiter) verändert wurden.

## Bewusst nicht nachgezogen: dieselbe Lücke in `openai_compatible.rs`

`crates/ai-providers/src/openai_compatible.rs:217` baut bei leerem
`system_context` weiterhin `{"role":"system","content":""}` — ein leeres,
aber vorhandenes System-Nachrichtenobjekt. Ob ein konkreter
OpenAI-kompatibler Endpunkt das ablehnt, ist ungeprüft; falls ja, würde
„Zugangsdaten testen" dort mit demselben Symptom scheitern wie vor diesem
Fix bei Anthropic.

Spec 0081, Abschnitt 2 („Ziel und Nicht-Ziele"), grenzt den Scope
ausdrücklich auf `AnthropicProvider::build_request_body` ein und nennt
Änderungen an der Probe in `commands.rs` explizit als Nicht-Ziel — die
Übertragung auf einen anderen Provider ist damit über den Rahmen dieses
Schritts hinaus. Der `spec-reviewer` hat den Fund in beiden Runden als
„zurückstellbar" eingestuft, nicht als Blocker für Spec 0081. Er gehört
als eigenes Backlog-Item erfasst, nicht in diesen Schritt gezogen.

## Bewusst abgewichen: Doc-Kommentar von `test_cache_control_set_unconditionally_even_for_a_tiny_system_prompt` geändert

Spec 0081, Abschnitt 5, nennt diesen Test unter den drei bestehenden
Tests, die „unverändert grün" bleiben. Die Runde-1-Reviewrunde hat als
zurückstellbaren Punkt gemeldet, dass der Doc-Kommentar dieses Tests
(„Caching wird IMMER gesetzt“) nach dem Fix ohne Ergänzung leicht
irreführend wirkt, weil `build_request_body` jetzt eine Ausnahme kennt.

Der Kommentar wurde daraufhin um einen Halbsatz ergänzt, der auf die
Ausnahme und den neuen Test verweist — die Assertions und damit das
Testverhalten selbst sind unangetastet, der Test bleibt exakt wie von der
Spec verlangt grün. Runde 2 des Reviews hat die Ergänzung bestätigt und
nur eine kosmetische Formulierungsschwäche angemerkt („nicht-leer“ statt
präziser „nach `trim()` nicht leer“) — als reine Wortwahl ohne
Verhaltensbezug nicht korrigiert.
