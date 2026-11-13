use super::*;

fn vocab() -> Vocab {
    Vocab::parse("<unk> 0\n<|pnc|> 1\n▁hel 2\nlo 3\n▁world 4\n<blk> 5\n").unwrap()
}

/// Joint output with one-hot token and duration logits.
fn out(vocab_size: usize, token: usize, skip: usize) -> Vec<f32> {
    let mut v = vec![0.0; vocab_size + 5];
    v[token] = 1.0;
    v[vocab_size + skip] = 1.0;
    v
}

#[test]
fn vocab_rejects_malformed_files() {
    assert!(Vocab::parse("").is_none());
    assert!(Vocab::parse("a 0\nb 2\n<blk> 3\n").is_none(), "gap in ids");
    assert!(Vocab::parse("a 0\nb 1\n").is_none(), "no blank at the end");
    assert!(Vocab::parse("a\n<blk> 1\n").is_none(), "no id");
    assert_eq!(vocab().blank, 5);
}

#[test]
fn detokenize_joins_pieces_and_drops_control_tokens() {
    let v = vocab();
    assert_eq!(v.detokenize(&[1, 2, 3, 4]), "hello world");
    assert_eq!(v.detokenize(&[0, 5]), "");
    assert_eq!(v.detokenize(&[]), "");
    // An id outside the vocabulary is ignored, not a panic.
    assert_eq!(v.detokenize(&[2, 99, 3]), "hello");
}

#[test]
fn greedy_decoding_follows_durations_and_keeps_state_on_blank() {
    // Frame 0: token 2, stay (skip 0). Frame 0 again: token 3, skip 2.
    // Frame 2: blank, skip 0 -> next frame. Frame 3: token 4, skip 1. End.
    let script = [
        (0usize, out(6, 2, 0)),
        (0, out(6, 3, 2)),
        (2, out(6, 5, 0)),
        (3, out(6, 4, 1)),
    ];
    let mut calls = Vec::new();
    let tokens = tdt_greedy(4, 5, |frame, prev, state| {
        let i = calls.len();
        calls.push((frame, prev));
        let (f, o) = &script[i];
        assert_eq!(*f, frame);
        // The state a step returns is only passed on after a real token.
        let marker = (i + 1) as f32;
        let new = (vec![marker], vec![marker]);
        if i == 3 {
            assert_eq!(state.0, vec![2.0], "blank step must not advance the state");
        }
        Ok((o.clone(), new))
    })
    .unwrap();
    assert_eq!(tokens, vec![2, 3, 4]);
    assert_eq!(calls, vec![(0, 5), (0, 2), (2, 3), (3, 3)]);
}

#[test]
fn greedy_decoding_advances_after_too_many_tokens_on_one_frame() {
    let mut steps = 0;
    let tokens = tdt_greedy(1, 5, |_, _, _| {
        steps += 1;
        Ok((out(6, 2, 0), zero_state()))
    })
    .unwrap();
    assert_eq!(tokens.len(), MAX_TOKENS_PER_FRAME);
    assert_eq!(steps, MAX_TOKENS_PER_FRAME);
}

#[test]
fn greedy_decoding_fails_on_a_short_output() {
    assert!(tdt_greedy(1, 5, |_, _, _| Ok((vec![0.0; 6], zero_state()))).is_err());
}

#[test]
fn silence_gives_no_tokens() {
    let tokens = tdt_greedy(10, 5, |_, _, _| Ok((out(6, 5, 0), zero_state()))).unwrap();
    assert!(tokens.is_empty());
}

#[test]
fn long_audio_is_cut_in_the_quiet_part() {
    let mut samples = vec![0.5f32; CHUNK_SAMPLES + 5 * TARGET_RATE as usize];
    // A quiet gap 1 s before the chunk limit.
    let gap = CHUNK_SAMPLES - TARGET_RATE as usize;
    samples[gap..gap + 400].fill(0.0);
    let chunks = split_chunks(&samples);
    assert_eq!(chunks.len(), 2);
    assert!(
        chunks[0].len().abs_diff(gap + 200) < 400,
        "{}",
        chunks[0].len()
    );
    assert_eq!(chunks.iter().map(|c| c.len()).sum::<usize>(), samples.len());
    assert_eq!(split_chunks(&samples[..1000]).len(), 1);
}

#[test]
fn missing_model_files_fail_to_open() {
    let dir = tempfile::tempdir().unwrap();
    assert!(ParakeetTranscriber.open_session(dir.path()).is_err());
}

/// 16 kHz mono WAV, 16-bit PCM or 32-bit float.
fn read_wav(path: &str) -> Vec<f32> {
    let b = std::fs::read(path).unwrap();
    assert_eq!(&b[..4], b"RIFF");
    let fmt = b.windows(4).position(|w| w == b"fmt ").unwrap() + 8;
    let tag = u16::from_le_bytes([b[fmt], b[fmt + 1]]);
    let rate = u32::from_le_bytes([b[fmt + 4], b[fmt + 5], b[fmt + 6], b[fmt + 7]]);
    assert_eq!(rate, 16_000);
    let data = b.windows(4).position(|w| w == b"data").unwrap() + 8;
    match tag {
        1 => b[data..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| i16::from_le_bytes(*c) as f32 / 32768.0)
            .collect(),
        3 => b[data..]
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| f32::from_le_bytes(*c))
            .collect(),
        _ => panic!("unsupported WAV format {tag}"),
    }
}

/// Opt-in: runs the real model. Needs the four model files in
/// `SPEECH_PARAKEET_DIR` and 16 kHz mono WAVs in `SPEECH_WAV_DE` /
/// `SPEECH_WAV_EN`; `SPEECH_EXPECT_DE` / `SPEECH_EXPECT_EN` hold a word each
/// that must appear (case-insensitive).
#[test]
#[ignore = "needs the downloaded model and WAV files"]
fn real_model_recognises_german_and_english() {
    let dir = std::env::var("SPEECH_PARAKEET_DIR").unwrap();
    let session = ParakeetTranscriber
        .open_session(Path::new(&dir))
        .unwrap()
        .unwrap();
    for (wav, expect) in [
        ("SPEECH_WAV_DE", "SPEECH_EXPECT_DE"),
        ("SPEECH_WAV_EN", "SPEECH_EXPECT_EN"),
    ] {
        let samples = read_wav(&std::env::var(wav).unwrap());
        // The language setting must not matter.
        let text = session.transcribe(&samples, Some("de")).unwrap();
        let text_auto = session.transcribe(&samples, None).unwrap();
        assert_eq!(text, text_auto);
        let want = std::env::var(expect).unwrap().to_lowercase();
        assert!(
            text.to_lowercase().contains(&want),
            "{text:?} lacks {want:?}"
        );
    }
    assert_eq!(session.transcribe(&vec![0.0; 16_000], None).unwrap(), "");
}
