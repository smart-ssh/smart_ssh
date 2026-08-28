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

use futures::{Stream, StreamExt};

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
