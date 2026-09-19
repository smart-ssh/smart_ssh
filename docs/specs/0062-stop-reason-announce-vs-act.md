# Spec: stop_reason-Sichtbarkeit + „ankündigen statt handeln"-Fix

Status: Umgesetzt
Repo: **öffentlich** `smart_ssh`, `crates/ai-providers` (stop_reason) +
`crates/app-shell`/commands.rs (System-Prompt)
Abhängigkeiten: KI-Provider (0006), Auto-Fortsetzung (0021), Body-Timeout/
Streaming-Parsing (anthropic.rs)

> **Das Problem (im echten Einsatz beobachtet):** Die KI schreibt eine
> **Ankündigung** eines nächsten Schritts („Lassen wir uns Status und Journal
> anzeigen:") und **endet dann den Turn, ohne `suggest_command` aufzurufen**.
> Kein Tool-Call → kein `ActionProposed` → `executed_action = false` → die
> Auto-Fortsetzung (0021) hat nichts, worauf sie reagieren kann, und stoppt.
> Für den Nutzer sieht es aus wie „die KI bleibt mitten im Satz stehen".
> **Zwei Teile: Sichtbarkeit (warum endete der Turn?) + Gegensteuern (Prompt).**
> Priorität NORMAL (Prompt berührt Modellverhalten — vorsichtig).

## Teil 1: `stop_reason` parsen + loggen (Diagnose-Grundlage)

Der Anthropic-Provider wertete bislang nur `content_block_*`/`message_stop`
aus — das Feld **`stop_reason`** (warum der Turn endete) wurde nirgends
geparst oder geloggt. Damit ließ sich nicht unterscheiden:
- `end_turn` → Modell hat **bewusst** aufgehört (das wahrscheinliche hier)
- `max_tokens` → Antwort wurde **technisch abgeschnitten** (Token-Limit) — das
  wäre ein **echter Bug** (Limit zu niedrig), keine Modell-Eigenheit
- `tool_use` → Modell will ein Tool aufrufen
- `stop_sequence`/andere

**Umsetzung:**
- `stop_reason` aus dem `message_delta`-SSE-Event geparst (nicht
  `message_stop` — dessen eigenes `delta` ist leer).
- Geloggt mit `request_id` (`tracing::info!`, kein sensibler Inhalt — nur
  das Enum-/String-Feld).
- Analog für den OpenAI-kompatiblen Provider: `choices[0].finish_reason`
  (`stop`/`length`/`tool_calls`), unabhängig vom bestehenden `delta`-Zugriff
  geprüft (liegt typischerweise im letzten Chunk mit leerem/fehlendem
  `delta`).

## Teil 2: System-Prompt — „handeln statt ankündigen"

Eine vorsichtige Ergänzung im System-Prompt (`commands.rs`), die genau das
beobachtete Muster adressiert: die KI soll ein Kommando **ausführen** (Tool
aufrufen), statt es nur **anzukündigen** — ohne dabei kurze Erklärungen vor
einem Kommando zu verbieten.

**Korrektur zur ursprünglichen Annahme dieser Spec:** Es gibt in diesem
Repo **nur einen einzigen, deutschen** System-Prompt
(`crates/app-shell/src/commands.rs::build_session_system_context`) — keine
englische Variante. Die "DE+EN, beide existieren"-Prämisse traf nicht zu
(die zweisprachigen `locales/de,en/common.json`-Dateien sind reine
Frontend-UI-Strings, kein KI-Prompt). Nur die eine Stelle angepasst.

## Nicht Teil dieser Spec (Backlog-Gedanke)
- **Automatisches Nachhaken**: Wenn die App `end_turn` **ohne** vorangehenden
  Tool-Call erkennt und der Text mit einer Ankündigung endet (`:` o. Ä.),
  könnte sie automatisch „und weiter?" nachschieben statt zu stoppen. Eigene,
  heiklere Spec (wann genau nachhaken, ohne zu nerven?) — hier nur als
  Gedanke, nicht gebaut.

## Invarianten / Sicherheit
- Das `stop_reason`-Logging enthält **keinen** sensiblen Inhalt (nur das
  Enum-Feld + request_id).
- Die Prompt-Änderung ändert **nichts** an der Filter-/Confirm-Bahn — ein
  per Tool vorgeschlagenes Kommando läuft weiter durch die Filter-Engine +
  Confirm wie bisher. Der Prompt beeinflusst nur, *ob* die KI das Tool nutzt,
  nicht *was danach* passiert.

## Testbarkeit
- `stop_reason`/`finish_reason` wird geparst + geloggt (Regressionstests in
  `crates/ai-providers/src/anthropic.rs`/`openai_compatible.rs`, über den
  echten SSE-Parsing-Pfad — `end_turn`/`stop` und `max_tokens`/`length`
  jeweils abgedeckt, für beide Provider).
- Der Prompt-Text enthält die Ergänzung (Regressionstest in `commands.rs`,
  prüft sowohl die neue Anweisung als auch, dass der "kurz erklären bleibt
  erlaubt"-Satz erhalten bleibt).
- **Nicht automatisiert testbar**: ob die Prompt-Ergänzung das reale
  Verhalten ändert (das zeigt nur echte Nutzung) — Stefan beobachtet, ob das
  „ankündigen ohne handeln"-Muster seltener wird.

## Abschluss
- Zwei Commits: `fea08c9` (Teil 1), `a735c66` (Teil 2).
- `spec-reviewer` NORMAL.
- CHANGELOG (unter „Changed", da Verhaltens-Feinschliff, kein neues
  Feature).
