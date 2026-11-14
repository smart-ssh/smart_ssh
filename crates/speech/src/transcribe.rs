//! Local transcription behind a trait. The engines run only on verified model
//! files (Spec 0108, 6).

use std::path::Path;

/// `None` = auto-detect, `Some(code)` = fixed language. Engines that always
/// detect the language themselves (Parakeet) ignore it.
pub trait Transcriber: Send + Sync {
    fn transcribe(
        &self,
        model: &Path,
        samples: &[f32],
        language: Option<&str>,
    ) -> Result<String, TranscribeError>;

    /// Loads the model once for several passes over one recording. `None`
    /// (the default) means the engine keeps no state: every pass calls
    /// `transcribe`.
    fn open_session(
        &self,
        _model: &Path,
    ) -> Result<Option<Box<dyn TranscribeSession>>, TranscribeError> {
        Ok(None)
    }
}

/// A transcriber with the model already loaded. Dropping it frees the model.
pub trait TranscribeSession: Send + Sync {
    fn transcribe(
        &self,
        samples: &[f32],
        language: Option<&str>,
    ) -> Result<String, TranscribeError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TranscribeError;

/// Maps the language setting to Whisper's input: `auto` (or empty) is
/// auto-detect, an ISO code Whisper knows is passed as is.
pub fn parse_language(setting: &str) -> Result<Option<String>, InvalidLanguage> {
    let s = setting.trim().to_ascii_lowercase();
    if s.is_empty() || s == "auto" {
        return Ok(None);
    }
    // The ISO code or the English name Whisper knows; the code is returned.
    crate::whisper::language_code(&s)
        .map(|code| Some(code.to_owned()))
        .ok_or(InvalidLanguage)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidLanguage;

const MARKER_WORDS: &[&str] = &[
    "blank",
    "audio",
    "silence",
    "silent",
    "silencio",
    "silencieux",
    "stille",
    "stumm",
    "music",
    "musik",
    "musica",
    "música",
    "musique",
    "applause",
    "applaus",
    "applaudissements",
    "aplausos",
    "laughter",
    "laughs",
    "lachen",
    "rires",
    "risas",
    "noise",
    "geräusch",
    "geraeusch",
    "bruit",
    "ruido",
    "inaudible",
    "unverständlich",
    "speaking",
    "foreign",
    "language",
    "no",
    "speech",
    "pause",
    "sound",
    "sounds",
    "leer",
    "vide",
    "vacío",
    "video",
    "singing",
    "gesang",
    "cough",
    "coughing",
    "typing",
    "breathing",
];

/// Whether the text inside a bracket pair names a non-speech sound. Only
/// words from a fixed list count, so `[index]` or `(0)` the user said stay.
fn is_marker_content(inner: &str) -> bool {
    let words: Vec<String> = inner
        .split(|c: char| c.is_whitespace() || c == '_' || c == '-' || c == '.')
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect();
    !words.is_empty()
        && words.len() <= 5
        && words.iter().all(|w| MARKER_WORDS.contains(&w.as_str()))
}

/// Removes recognizer markers (`[BLANK_AUDIO]`, `(music)`, `*music*`, `♪`)
/// from one segment. Bracketed text is dropped only when it is a known
/// non-speech word list, or when the whole segment is a single bracket
/// group of letters (e.g. `[Typing]`); anything else the user may have said,
/// like `array [0]`, is kept. Whitespace is normalized around removals.
pub fn strip_non_speech(text: &str) -> String {
    let t = text.trim();
    let whole = |o: char, c: char| {
        t.len() > 2
            && t.starts_with(o)
            && t.ends_with(c)
            && !t[1..t.len() - 1].contains([o, c])
            && t[1..t.len() - 1]
                .chars()
                .all(|ch| ch.is_alphabetic() || ch.is_whitespace() || ch == '_' || ch == '-')
    };
    if whole('[', ']') || whole('(', ')') {
        return String::new();
    }
    let mut out = String::new();
    let mut rest = t;
    while let Some((i, ch)) = rest
        .char_indices()
        .find(|(_, c)| matches!(c, '[' | '(' | '*'))
    {
        let close = match ch {
            '[' => ']',
            '(' => ')',
            _ => '*',
        };
        let after = &rest[i + 1..];
        match after.find(close) {
            Some(j) if is_marker_content(&after[..j]) => {
                out.push_str(&rest[..i]);
                out.push(' ');
                rest = &after[j + 1..];
            }
            _ => {
                out.push_str(&rest[..i + ch.len_utf8()]);
                rest = after;
            }
        }
    }
    out.push_str(rest);
    let out: String = out.chars().filter(|c| !matches!(c, '♪' | '♫')).collect();
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Picks the engine from the files in the model directory.
#[derive(Default)]
pub struct SpeechTranscriber;

impl SpeechTranscriber {
    fn engine_for(model: &Path) -> Box<dyn Transcriber> {
        if model.join(crate::whisper::ENCODER).is_file() {
            Box::new(crate::whisper::WhisperTranscriber)
        } else {
            Box::new(crate::parakeet::ParakeetTranscriber)
        }
    }
}

impl Transcriber for SpeechTranscriber {
    fn transcribe(
        &self,
        model: &Path,
        samples: &[f32],
        language: Option<&str>,
    ) -> Result<String, TranscribeError> {
        Self::engine_for(model).transcribe(model, samples, language)
    }

    fn open_session(
        &self,
        model: &Path,
    ) -> Result<Option<Box<dyn TranscribeSession>>, TranscribeError> {
        Self::engine_for(model).open_session(model)
    }
}

#[cfg(test)]
mod filter_tests {
    use super::strip_non_speech;

    #[test]
    fn drops_whole_segment_markers() {
        for m in [
            "[BLANK_AUDIO]",
            " [MUSIC] ",
            "(music)",
            "[silence]",
            "[ Silence ]",
            "*music*",
            "[Stille]",
            "(Musique)",
            "[Música]",
            "[Typing]",
            "♪",
            "[inaudible]",
            "(speaking in foreign language)",
        ] {
            assert_eq!(strip_non_speech(m), "", "{m}");
        }
    }

    #[test]
    fn drops_inline_markers() {
        assert_eq!(strip_non_speech("hello [BLANK_AUDIO] world"), "hello world");
        assert_eq!(strip_non_speech("hallo (Musik) welt"), "hallo welt");
        assert_eq!(strip_non_speech("ls *music* -la"), "ls -la");
        assert_eq!(strip_non_speech("hola [silencio]"), "hola");
    }

    #[test]
    fn keeps_user_brackets() {
        assert_eq!(strip_non_speech("array [0]"), "array [0]");
        assert_eq!(strip_non_speech("print (x) now"), "print (x) now");
        assert_eq!(strip_non_speech("a * b * c"), "a * b * c");
        assert_eq!(strip_non_speech("[0]"), "[0]");
        assert_eq!(strip_non_speech("foo [index] bar"), "foo [index] bar");
    }
}
