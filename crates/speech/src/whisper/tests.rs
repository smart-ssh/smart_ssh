use std::path::Path;

use super::mel::{MelFrontEnd, N_FRAMES, WINDOW_SAMPLES};
use super::*;
use crate::parakeet::tests::read_wav;
use crate::transcribe::parse_language;

#[test]
fn byte_level_bpe_decodes_multibyte_text() {
    // "Ġ" is a space; "Ã¤" are the two UTF-8 bytes of "ä".
    let t = Tokens::parse(r#"{"Ġhallo": 0, "ĠW": 1, "Ã¤": 2, "x": 3}"#).unwrap();
    assert_eq!(t.decode(&[0, 1, 2, 3]), " hallo Wäx");
    // Ids outside the vocabulary (specials) are dropped, never a panic.
    assert_eq!(t.decode(&[0, 50_258, -1]), " hallo");
}

#[test]
fn specials_find_language_tokens() {
    let s = Specials::parse(
        r#"{"<|startoftranscript|>":50258,"<|en|>":50259,"<|de|>":50260,
            "<|translate|>":50358,"<|transcribe|>":50359,"<|nocaptions|>":50362,"<|notimestamps|>":50363,
            "<|0.00|>":50364}"#,
        50257,
    )
    .unwrap();
    assert_eq!(s.eot, 50257);
    assert_eq!(s.sot, 50258);
    assert_eq!(
        s.languages,
        [("en".to_owned(), 50259), ("de".to_owned(), 50260)]
    );
    assert_eq!(s.no_captions, Some(50362));
    assert!(Specials::parse("{}", 1).is_none());
}

#[test]
fn repetition_loops_are_cut_to_one_copy() {
    let mut v = vec![5, 1, 2, 3, 1, 2, 3, 1, 2, 3, 1, 2, 3];
    assert!(trim_repetition(&mut v));
    assert_eq!(v, [5, 1, 2, 3]);
    // Three copies are still plausible speech.
    let mut v = vec![1, 2, 1, 2, 1, 2];
    assert!(!trim_repetition(&mut v));
    let mut v = vec![7, 7, 7, 7];
    assert!(trim_repetition(&mut v));
    assert_eq!(v, [7]);
    let mut v: Vec<i64> = Vec::new();
    assert!(!trim_repetition(&mut v));
}

/// The language list the previous engine (whisper.cpp) accepted.
const PREVIOUS_CODES: &str = "en zh de es ru ko fr ja pt tr pl ca nl ar sv it id hi fi vi he uk el ms cs ro da hu ta no th ur hr bg lt la mi ml cy sk te fa lv bn sr az sl kn et mk br eu is hy ne mn bs kk sq sw gl mr pa si km sn yo so af oc ka be tg sd gu am yi lo uz fo ht ps tk nn mt sa lb my bo tl mg as tt haw ln ha ba jw su yue";

#[test]
fn parse_language_accepts_the_previous_list_and_rejects_the_rest() {
    assert_eq!(PREVIOUS_CODES.split(' ').count(), 100);
    for code in PREVIOUS_CODES.split(' ') {
        assert_eq!(parse_language(code), Ok(Some(code.to_owned())), "{code}");
        assert_eq!(
            parse_language(&code.to_uppercase()),
            Ok(Some(code.to_owned()))
        );
    }
    // English names were accepted before and map to the code.
    assert_eq!(parse_language("German"), Ok(Some("de".to_owned())));
    assert_eq!(parse_language("haitian creole"), Ok(Some("ht".to_owned())));
    for bad in ["xx", "deu", "de-DE", "zz", "d", "english!", "../de"] {
        assert!(parse_language(bad).is_err(), "{bad}");
    }
    assert_eq!(parse_language(" auto "), Ok(None));
}

#[test]
fn mel_features_have_whisper_shape_and_range() {
    let mel = MelFrontEnd::new(80);
    // Silence: every value is the clamp floor, (max-8 clamped, +4)/4.
    let silent = mel.features(&vec![0.0; 16_000]);
    assert_eq!(silent.len(), 80 * N_FRAMES);
    assert!(silent.iter().all(|v| (*v - silent[0]).abs() < 1e-6));
    assert!((silent[0] - (-10.0 + 4.0) / 4.0).abs() < 1e-6);
    // A 1 kHz tone: the energy sits in one mel band and the values stay in
    // the normalised range.
    let tone: Vec<f32> = (0..16_000)
        .map(|i| (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / 16_000.0).sin() * 0.5)
        .collect();
    let f = mel.features(&tone);
    let frame = 50;
    let col: Vec<f32> = (0..80).map(|m| f[m * N_FRAMES + frame]).collect();
    let peak = col.iter().cloned().fold(f32::MIN, f32::max);
    assert!((peak - 1.5).abs() < 0.6, "peak {peak}");
    let peak_band = col.iter().position(|v| *v == peak).unwrap();
    // 1 kHz is band ~ 40 of 80 on the Slaney scale (1 kHz = 15 mel of 45.2).
    assert!((22..=30).contains(&peak_band), "band {peak_band}");
    assert!(f.iter().all(|v| (-1.5..=2.5).contains(v)));
    // Input longer than 30 s is cut, not a panic.
    assert_eq!(
        mel.features(&vec![0.1; WINDOW_SAMPLES + 5_000]).len(),
        80 * N_FRAMES
    );
}

/// Opt-in: runs a real model. Needs the model directory in
/// `SPEECH_WHISPER_DIR` and 16 kHz mono WAVs in `SPEECH_WAV_DE` /
/// `SPEECH_WAV_EN`; `SPEECH_EXPECT_DE` / `SPEECH_EXPECT_EN` hold a word each
/// that must appear (case-insensitive).
#[test]
#[ignore = "needs a downloaded model and WAV files"]
fn real_model_recognises_german_and_english() {
    let dir = std::env::var("SPEECH_WHISPER_DIR").unwrap();
    let session = WhisperTranscriber
        .open_session(Path::new(&dir))
        .unwrap()
        .unwrap();
    for (wav, expect, code) in [
        ("SPEECH_WAV_DE", "SPEECH_EXPECT_DE", "de"),
        ("SPEECH_WAV_EN", "SPEECH_EXPECT_EN", "en"),
    ] {
        let samples = read_wav(&std::env::var(wav).unwrap());
        let want = std::env::var(expect).unwrap().to_lowercase();
        for language in [None, Some(code)] {
            let text = session.transcribe(&samples, language).unwrap();
            eprintln!("{wav} {language:?}: {text}");
            assert!(
                text.to_lowercase().contains(&want),
                "{text:?} lacks {want:?}"
            );
        }
    }
    assert_eq!(session.transcribe(&vec![0.0; 16_000], None).unwrap(), "");
}
