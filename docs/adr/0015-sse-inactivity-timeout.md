# 0015-sse-inactivity-timeout

## Status
Accepted

## Kontext

Ein Nutzer berichtete, dass der Chat mitten in einem Dialog ohne jede
Fehlermeldung aufhörte zu antworten — kein `chat-error`, keine sichtbare
Reaktion, einfach Stille. Bei der Untersuchung stellte sich heraus: weder
`AnthropicProvider` noch `OpenAiCompatibleProvider`
(`crates/ai-providers/src/anthropic.rs`/`openai_compatible.rs`)
konfigurieren einen Timeout auf ihrem `reqwest::Client`
(`reqwest::Client::new()` liefert einen Client **ohne** Zeitlimit für
laufende Requests).

Bleibt eine Verbindung zum KI-Provider hängen — Netzwerk-Aussetzer, der
Server hält die Verbindung offen ohne weitere Daten zu senden, ein
stockender TLS-Handshake beim initialen Verbindungsaufbau — wartet
`run_one_round` (`crates/app-tauri/src/orchestration.rs`,
`docs/adr/0014-...md`) für immer auf `stream.next().await`. Es kommt nie
ein `Done`, nie ein `Error` — der `send_chat_message`-Tauri-Command löst
sich nie auf, und das Frontend bleibt für immer im `sending`-Zustand,
ohne jede Erklärung für den Nutzer.

## Entscheidung

Ein gemeinsamer `SSE_INACTIVITY_TIMEOUT` (90 Sekunden,
`crates/ai-providers/src/sse.rs`) wrappt in beiden Providern zwei Stellen
mit `tokio::time::timeout`:

1. **Initialer Verbindungsaufbau** (`client.post(...).send()`): läuft der
   Handshake/das Warten auf die Response-Header länger als das Limit, wird
   die Anfrage abgebrochen und ein `AiEvent::Error(AiError::NetworkError)`
   geliefert statt weiter zu warten.
2. **Jeder einzelne SSE-Chunk während des Streamings**
   (`state.frames.next().await` in `process_frame_stream`): das Limit wird
   bei **jedem empfangenen Chunk neu gestartet**, nicht einmalig für die
   gesamte Antwort gesetzt.

Bewusst ein **Inaktivitäts**- statt ein Gesamt-Timeout: würde stattdessen
`reqwest::ClientBuilder::timeout(...)` auf dem Client selbst gesetzt (die
naheliegendere, aber falsche Alternative), gilt dieses Zeitlimit laut
reqwest-Dokumentation für den **gesamten** Request inklusive des
vollständigen Einlesens des Streaming-Bodys — eine legitime, aber lange
laufende Antwort (viele Tool-Calls, große Ausgabe, ein Modell, das
tatsächlich länger als 90 Sekunden aktiv Text/Tool-Use-Chunks liefert)
würde dann fälschlich mitten in der Ausgabe abgebrochen. Ein pro-Chunk
neu gestartetes Inaktivitäts-Timeout unterscheidet sauber zwischen "der
Provider arbeitet noch, liefert nur langsam" (kein Abbruch) und "die
Verbindung ist tot, es kommt nichts mehr" (Abbruch).

Die Timeout-Logik selbst (`process_frame_stream`) wurde von der
HTTP-Response-Beschaffung (`event_stream_from_response`) getrennt, damit
sie sich mit einem synthetischen, nie liefernden `Stream` unter
`tokio::test(start_paused = true)` testen lässt — Tokios virtuelle Uhr
lässt das volle 90-Sekunden-Timeout in Millisekunden Testlaufzeit
durchlaufen, ganz ohne echten Mock-Server oder reales Warten.

## Konsequenzen

**Positiv:**
- Ein hängender KI-Provider-Request blockiert einen Chat-Turn nicht mehr
  unbegrenzt — nach spätestens 90 Sekunden Inaktivität kommt eine echte,
  für den Nutzer sichtbare Fehlermeldung statt endloser Stille.
- Legitime, aktiv streamende (auch lange) Antworten sind vom Timeout nicht
  betroffen, solange der Provider weiterhin in unter 90-Sekunden-Abständen
  Chunks liefert.
- Das Timeout-Verhalten ist mit paused-time-Tests in Millisekunden
  abgedeckt, statt entweder ungetestet zu bleiben oder Testläufe um real
  90 Sekunden zu verlangsamen.

**Negativ / Trade-off:**
- 90 Sekunden ist eine Schätzung, keine aus einer Spec abgeleitete Zahl —
  ein Provider/Modell, das legitim länger als 90 Sekunden zwischen zwei
  Chunks pausiert (z. B. sehr lange interne "Denkpause" ohne
  Zwischen-Events), würde fälschlich abgebrochen. Bisher nicht beobachtet,
  aber ein möglicher Grund für eine spätere Anpassung des Werts.
- Zwei separate `tokio::time::timeout`-Aufrufe pro Provider (Verbindungs-
  aufbau + Streaming-Loop) statt eines einzigen zentralen Mechanismus —
  Preis dafür, dass beide Phasen unterschiedliche Fehlerbehandlung brauchen
  (vor Erhalt der Response vs. während des Streamings).
