//! Server-Sent-Events-Framing, geteilt zwischen allen Providern
//! (Aufgabenstellung Teil 2, Punkt 6 — die genaue SSE-Interpretation ist in
//! der Spec nicht bis auf Byte-Ebene festgelegt).
//!
//! Nur das Zerlegen des Byte-Stroms in Frames (`event:`/`data:`-Zeilen,
//! getrennt durch eine Leerzeile, siehe <https://html.spec.whatwg.org/multipage/server-sent-events.html>)
//! ist providerunabhängig und wird hier geteilt. Wie der `data`-Teil eines
//! Frames als JSON zu interpretieren ist, unterscheidet sich zwischen
//! OpenAI-kompatiblen APIs und der Anthropic-API erheblich und bleibt
//! deshalb in den jeweiligen Provider-Modulen.

use std::collections::VecDeque;
use std::time::Duration;

use futures::{Stream, StreamExt};

/// Maximale Wartezeit auf den *nächsten* SSE-Frame, bevor ein Stream als
/// hängengeblieben gilt und mit einem Fehler statt endlos weiterzuwarten
/// abgebrochen wird. `reqwest::Client::new()` (beide Provider) setzt
/// standardmäßig **keinen** Timeout für einen laufenden Request — bricht
/// die zugrunde liegende TCP-Verbindung nicht sauber ab (z. B. Netzwerk-
/// Aussetzer, Server hält die Verbindung offen ohne weitere Daten zu
/// senden), würde `AiProvider::send()` sonst nie ein `Done`/`Error`
/// liefern und `run_chat_turn` (`crates/app-shell/src/orchestration.rs`)
/// bliebe für immer auf `stream.next().await` hängen — für den Nutzer
/// sichtbar als Chat, der ohne jede Fehlermeldung einfach nicht mehr
/// antwortet. Bewusst als Inaktivitäts- statt Gesamt-Timeout (pro
/// empfangenem Frame neu gestartet), damit eine legitime, aber lange
/// laufende Antwort (viele Tool-Calls, große Ausgabe) nicht fälschlich
/// abgebrochen wird, solange der Provider weiterhin Daten schickt.
pub(crate) const SSE_INACTIVITY_TIMEOUT: Duration = Duration::from_secs(90);

/// Ein einzelner geparster SSE-Frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SseFrame {
    pub event: Option<String>,
    pub data: String,
}

/// Extrahiert alle vollständigen Frames (durch eine Leerzeile
/// abgeschlossen) aus `buffer` und lässt einen eventuellen unvollständigen
/// Rest darin stehen. Reine Funktion ohne I/O — direkt unit-testbar, ganz
/// ohne Mock-HTTP-Server.
pub(crate) fn drain_complete_frames(buffer: &mut String) -> Vec<SseFrame> {
    let mut frames = Vec::new();
    while let Some(pos) = buffer.find("\n\n") {
        let frame_text: String = buffer.drain(..pos + 2).collect();
        if let Some(frame) = parse_frame(frame_text.trim_end_matches('\n')) {
            frames.push(frame);
        }
    }
    frames
}

fn parse_frame(text: &str) -> Option<SseFrame> {
    let mut event = None;
    let mut data_lines: Vec<&str> = Vec::new();
    for line in text.lines() {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if let Some(rest) = line.strip_prefix("event:") {
            event = Some(rest.trim().to_string());
        } else if let Some(rest) = line.strip_prefix("data:") {
            data_lines.push(rest.strip_prefix(' ').unwrap_or(rest));
        }
        // Andere Felder (id:, retry:, Kommentarzeilen ab ":") sind für
        // beide APIs irrelevant und werden bewusst ignoriert.
    }
    if event.is_none() && data_lines.is_empty() {
        return None;
    }
    Some(SseFrame {
        event,
        data: data_lines.join("\n"),
    })
}

/// Liest einen Fehler-Response-Body (bereits als nicht-2xx erkannt, Header
/// sind also schon da) begrenzt durch [`SSE_INACTIVITY_TIMEOUT`], statt mit
/// `response.text().await` unbegrenzt zu warten.
///
/// Bug-Diagnose ("AI-Provider-Aufruf kann unbegrenzt hängen (kein Log, kein
/// Fehler)", 2026-09): `AiProvider::send()` schützt den *Verbindungsaufbau*
/// (`tokio::time::timeout(SSE_INACTIVITY_TIMEOUT, send)`, s. oben) und das
/// *Frame-Lesen* (`process_frame_stream`) bereits gegen ein hängendes
/// Gegenüber — aber an vier Stellen (429-Retry- und allgemeiner Fehler-Zweig
/// in `anthropic.rs`/`openai_compatible.rs`) wurde der Fehler-Body direkt
/// mit `response.text().await.unwrap_or_default()` gelesen, VOR dem
/// jeweiligen Log-Aufruf. Ein Server/Proxy, der die Header eines
/// Fehlerstatus sofort schickt, den Body danach aber hängen lässt (anders
/// als ein Verbindungsaufbau, der schlicht nie antwortet — hier antwortet
/// der Server ja bereits, nur unvollständig), lief an diesen vier Stellen
/// nie in einen der beiden bestehenden Timeouts: der Request selbst war ja
/// bereits erfolgreich `send()`-et (Header liegen vor), und
/// `sse_frame_stream`/`process_frame_stream` werden für einen
/// Fehler-Response nie erreicht (`if !response.status().is_success()`
/// kommt VOR `event_stream_from_response`). Ergebnis: `.await` ohne
/// jede obere Schranke — exakt das gemeldete Symptom (Chat antwortet nicht,
/// kein Log-Eintrag, weil der Log-Aufruf erst NACH diesem `.await` steht).
///
/// Fix bewusst minimal: denselben, bereits etablierten
/// [`SSE_INACTIVITY_TIMEOUT`]-Wert auch aufs Body-Lesen anwenden, kein
/// neuer Timeout-Wert, kein neuer Mechanismus. Ein Timeout wird wie ein
/// gewöhnlicher Lesefehler behandelt (leerer String) — dieselbe Semantik,
/// die `.unwrap_or_default()` an diesen Stellen ohnehin schon für einen
/// echten `reqwest::Error` beim Body-Lesen hatte, keine neue `AiError`-
/// Variante nötig. Konsequenzen bewusst in Kauf genommen: der
/// 429-Retry-Zweig braucht `text` nur fürs Logging (`delay` kommt aus den
/// Headern) — unberührt; der allgemeine Fehler-Zweig verliert im
/// Timeout-Fall etwas Body-Detail in `map_http_status(status, "")`, aber
/// terminiert nach ≤90s statt nie, und der Log-Aufruf (das eigentliche
/// Ziel — Spec 0049, Fund 2: "kein Fehler ohne Log-Spur") wird garantiert
/// erreicht.
///
/// Nimmt bewusst das `Future` von `response.text()` entgegen statt der
/// `reqwest::Response` selbst — dadurch lässt sich das Timeout-Verhalten
/// direkt mit einer synthetischen, nie auflösenden Future testen (s.
/// Testmodul unten), exakt dasselbe Muster wie
/// `anthropic::process_frame_stream`s `#[tokio::test(start_paused = true)]`
/// -Test für den bereits bestehenden Frame-Lese-Timeout — ganz ohne echten
/// HTTP-Request/Mock-Server oder eine für Tests künstlich verkürzte
/// Timeout-Konstante.
pub(crate) async fn read_error_body_with_timeout(
    body_future: impl std::future::Future<Output = reqwest::Result<String>>,
) -> String {
    match tokio::time::timeout(SSE_INACTIVITY_TIMEOUT, body_future).await {
        Ok(result) => result.unwrap_or_default(),
        Err(_elapsed) => String::new(),
    }
}

/// Verwandelt eine `reqwest::Response` mit `text/event-stream`-Body in
/// einen Stream vollständiger [`SseFrame`]s. Puffert ankommende Bytes, bis
/// mindestens ein vollständiger Frame vorliegt.
pub(crate) fn sse_frame_stream(
    response: reqwest::Response,
) -> impl Stream<Item = Result<SseFrame, reqwest::Error>> + Send {
    let byte_stream = response.bytes_stream();
    futures::stream::unfold(
        (byte_stream, String::new(), VecDeque::new()),
        |(mut byte_stream, mut buffer, mut pending)| async move {
            loop {
                if let Some(frame) = pending.pop_front() {
                    return Some((Ok(frame), (byte_stream, buffer, pending)));
                }
                match byte_stream.next().await {
                    Some(Ok(chunk)) => {
                        buffer.push_str(&String::from_utf8_lossy(&chunk));
                        let frames = drain_complete_frames(&mut buffer);
                        if frames.is_empty() {
                            continue;
                        }
                        pending.extend(frames);
                    }
                    Some(Err(err)) => return Some((Err(err), (byte_stream, buffer, pending))),
                    None => return None,
                }
            }
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- read_error_body_with_timeout (Bug-Diagnose "AI-Provider-Aufruf
    // kann unbegrenzt hängen", 2026-09) -------------------------------
    //
    // `#[tokio::test(start_paused = true)]`: dasselbe Muster wie
    // `anthropic::tests::test_inactivity_timeout_yields_network_error_
    // instead_of_hanging_forever` — Tokios virtuelle Uhr startet
    // angehalten, `tokio::time::timeout` wartet dadurch nicht real 90
    // Sekunden, sondern die Uhr springt automatisch vor, sobald nichts
    // anderes mehr lauffähig ist. Testet damit den echten
    // `SSE_INACTIVITY_TIMEOUT`-Wert (keine für den Test verkürzte
    // Test-Konstante nötig) in Millisekunden Testlaufzeit.

    #[tokio::test(start_paused = true)]
    async fn test_read_error_body_with_timeout_returns_empty_string_instead_of_hanging_forever() {
        // `futures::future::pending()` löst nie auf — steht stellvertretend
        // für einen Server, der die Header eines Fehlerstatus bereits
        // geschickt hat, den Body danach aber hängen lässt (das exakte
        // Bug-Szenario: Header da, `.text()` wartet unbegrenzt).
        let never_resolves: std::pin::Pin<
            Box<dyn std::future::Future<Output = reqwest::Result<String>> + Send>,
        > = Box::pin(futures::future::pending());

        let result = read_error_body_with_timeout(never_resolves).await;

        assert_eq!(
            result, "",
            "ein Timeout beim Body-Lesen muss wie ein gewöhnlicher Lesefehler \
             behandelt werden (leerer String), nicht unbegrenzt blockieren"
        );
    }

    #[tokio::test]
    async fn test_read_error_body_with_timeout_passes_through_a_ready_body() {
        let ready = std::future::ready(Ok("Bad Request".to_string()));

        let result = read_error_body_with_timeout(ready).await;

        assert_eq!(result, "Bad Request");
    }

    #[test]
    fn test_drain_complete_frames_parses_single_data_only_frame() {
        let mut buffer = "data: hello\n\n".to_string();

        let frames = drain_complete_frames(&mut buffer);

        assert_eq!(
            frames,
            vec![SseFrame {
                event: None,
                data: "hello".to_string()
            }]
        );
        assert!(buffer.is_empty());
    }

    #[test]
    fn test_drain_complete_frames_parses_event_and_data() {
        let mut buffer = "event: content_block_delta\ndata: {\"a\":1}\n\n".to_string();

        let frames = drain_complete_frames(&mut buffer);

        assert_eq!(
            frames,
            vec![SseFrame {
                event: Some("content_block_delta".to_string()),
                data: "{\"a\":1}".to_string()
            }]
        );
    }

    #[test]
    fn test_drain_complete_frames_joins_multiple_data_lines() {
        let mut buffer = "data: line1\ndata: line2\n\n".to_string();

        let frames = drain_complete_frames(&mut buffer);

        assert_eq!(frames[0].data, "line1\nline2");
    }

    #[test]
    fn test_drain_complete_frames_leaves_incomplete_frame_in_buffer() {
        let mut buffer = "data: hello\n\ndata: incompl".to_string();

        let frames = drain_complete_frames(&mut buffer);

        assert_eq!(frames.len(), 1);
        assert_eq!(buffer, "data: incompl");
    }

    #[test]
    fn test_drain_complete_frames_handles_frames_split_across_calls() {
        let mut buffer = "data: par".to_string();
        assert!(drain_complete_frames(&mut buffer).is_empty());

        buffer.push_str("t1\n\n");
        let frames = drain_complete_frames(&mut buffer);

        assert_eq!(frames[0].data, "part1");
    }

    #[test]
    fn test_drain_complete_frames_ignores_blank_input() {
        let mut buffer = "\n\n".to_string();

        let frames = drain_complete_frames(&mut buffer);

        assert!(frames.is_empty());
    }
}
