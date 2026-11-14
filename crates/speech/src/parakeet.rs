//! NVIDIA Parakeet TDT 0.6B v3 (int8 ONNX export) on ONNX Runtime, CPU only
//! (Spec 0108). Four verified files in one directory: the mel preprocessor
//! `nemo128.onnx`, `encoder-model.int8.onnx`, `decoder_joint-model.int8.onnx`
//! and `vocab.txt`. The engine only reads these files; it never touches the
//! network (ONNX Runtime is linked statically, nothing is fetched at run
//! time), and neither audio nor text is logged.

use std::path::Path;
use std::sync::Mutex;

use ort::session::Session;
use ort::value::Tensor;
use ssh_manager_core::sync::lock_tolerating_poison;

use crate::transcribe::{TranscribeError, TranscribeSession, Transcriber};
use crate::TARGET_RATE;

const PREPROCESSOR: &str = "nemo128.onnx";
const ENCODER: &str = "encoder-model.int8.onnx";
const DECODER_JOINT: &str = "decoder_joint-model.int8.onnx";
const VOCAB: &str = "vocab.txt";

/// Hidden size of the prediction network's two LSTM state tensors
/// (`[layers, batch, hidden]`).
const STATE_LAYERS: usize = 2;
const STATE_HIDDEN: usize = 640;
/// Tokens the decoder may emit on one encoder frame before it must advance.
const MAX_TOKENS_PER_FRAME: usize = 10;
/// Longest piece of audio given to the model at once; longer recordings are
/// cut at the quietest point near this length.
const CHUNK_SAMPLES: usize = 30 * TARGET_RATE as usize;
const CUT_SEARCH_SAMPLES: usize = 3 * TARGET_RATE as usize;
/// Below this length (0.1 s) there is nothing to recognise.
const MIN_SAMPLES: usize = TARGET_RATE as usize / 10;

/// Recognition on the Parakeet files in a model directory. Always detects the
/// language itself; a language setting is ignored.
#[derive(Default)]
pub struct ParakeetTranscriber;

impl Transcriber for ParakeetTranscriber {
    fn transcribe(
        &self,
        model: &Path,
        samples: &[f32],
        language: Option<&str>,
    ) -> Result<String, TranscribeError> {
        let session = self.open_session(model)?.ok_or(TranscribeError)?;
        session.transcribe(samples, language)
    }

    fn open_session(
        &self,
        model: &Path,
    ) -> Result<Option<Box<dyn TranscribeSession>>, TranscribeError> {
        let env = ort::environment::init()
            .with_telemetry(false)
            .build()
            .map_err(ort_err)?;
        let load = |name: &str| -> Result<Session, TranscribeError> {
            let threads = std::thread::available_parallelism()
                .map(|n| n.get().min(8))
                .unwrap_or(4);
            let build = || -> ort::Result<Session> {
                let mut builder = Session::builder(&env)?.with_intra_threads(threads)?;
                builder.commit_from_file(model.join(name))
            };
            build().map_err(ort_err)
        };
        let vocab = std::fs::read_to_string(model.join(VOCAB))
            .map_err(|_| TranscribeError)
            .and_then(|t| Vocab::parse(&t).ok_or(TranscribeError))?;
        Ok(Some(Box::new(ParakeetSession {
            preprocessor: Mutex::new(load(PREPROCESSOR)?),
            encoder: Mutex::new(load(ENCODER)?),
            decoder: Mutex::new(load(DECODER_JOINT)?),
            vocab,
            _env: env,
        })))
    }
}

struct ParakeetSession {
    preprocessor: Mutex<Session>,
    encoder: Mutex<Session>,
    decoder: Mutex<Session>,
    vocab: Vocab,
    /// Must outlive the sessions above.
    _env: ort::environment::Environment,
}

fn ort_err(e: ort::Error) -> TranscribeError {
    tracing::error!(error = %e, "speech model inference failed");
    TranscribeError
}

impl TranscribeSession for ParakeetSession {
    fn transcribe(
        &self,
        samples: &[f32],
        _language: Option<&str>,
    ) -> Result<String, TranscribeError> {
        let mut text = String::new();
        for chunk in split_chunks(samples) {
            if chunk.len() < MIN_SAMPLES {
                continue;
            }
            let part = self.transcribe_chunk(chunk)?;
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            if !text.is_empty() {
                text.push(' ');
            }
            text.push_str(part);
        }
        Ok(text)
    }
}

impl ParakeetSession {
    fn transcribe_chunk(&self, samples: &[f32]) -> Result<String, TranscribeError> {
        let n = samples.len() as i64;
        // Mel features.
        let (features, feat_len) = {
            let mut pre = lock_tolerating_poison(&self.preprocessor);
            let out = pre
                .run(ort::inputs![
                    "waveforms" => Tensor::from_array(([1i64, n], samples.to_vec())).map_err(ort_err)?,
                    "waveforms_lens" => Tensor::from_array(([1i64], vec![n])).map_err(ort_err)?,
                ])
                .map_err(ort_err)?;
            let (shape, data) = out["features"]
                .try_extract_tensor::<f32>()
                .map_err(ort_err)?;
            let dims: Vec<i64> = shape.iter().copied().collect();
            let (_, len) = out["features_lens"]
                .try_extract_tensor::<i64>()
                .map_err(ort_err)?;
            (
                Tensor::from_array((dims, data.to_vec())).map_err(ort_err)?,
                len.first().copied().ok_or(TranscribeError)?,
            )
        };
        // Encoder: `[1, hidden, frames]`.
        let (enc, hidden, frames) = {
            let mut enc = lock_tolerating_poison(&self.encoder);
            let out = enc
                .run(ort::inputs![
                    "audio_signal" => features,
                    "length" => Tensor::from_array(([1i64], vec![feat_len])).map_err(ort_err)?,
                ])
                .map_err(ort_err)?;
            let (shape, data) = out["outputs"]
                .try_extract_tensor::<f32>()
                .map_err(ort_err)?;
            if shape.len() != 3 || shape[0] != 1 {
                return Err(TranscribeError);
            }
            let (_, lens) = out["encoded_lengths"]
                .try_extract_tensor::<i64>()
                .map_err(ort_err)?;
            let frames = lens.first().copied().ok_or(TranscribeError)?;
            (
                data.to_vec(),
                shape[1] as usize,
                (frames.max(0) as usize).min(shape[2] as usize),
            )
        };
        let total_frames = enc.len() / hidden.max(1);
        let mut dec = lock_tolerating_poison(&self.decoder);
        let tokens = tdt_greedy(
            frames,
            self.vocab.blank,
            |frame, prev, state: &DecoderState| {
                // One frame as `[1, hidden, 1]`.
                let column: Vec<f32> = (0..hidden).map(|c| enc[c * total_frames + frame]).collect();
                let shape = [STATE_LAYERS as i64, 1, STATE_HIDDEN as i64];
                let out = dec
                    .run(ort::inputs![
                        "encoder_outputs" => Tensor::from_array(([1i64, hidden as i64, 1], column)).map_err(ort_err)?,
                        "targets" => Tensor::from_array(([1i64, 1], vec![prev as i32])).map_err(ort_err)?,
                        "target_length" => Tensor::from_array(([1i64], vec![1i32])).map_err(ort_err)?,
                        "input_states_1" => Tensor::from_array((shape, state.0.clone())).map_err(ort_err)?,
                        "input_states_2" => Tensor::from_array((shape, state.1.clone())).map_err(ort_err)?,
                    ])
                    .map_err(ort_err)?;
                let (_, logits) = out["outputs"]
                    .try_extract_tensor::<f32>()
                    .map_err(ort_err)?;
                let (_, s1) = out["output_states_1"]
                    .try_extract_tensor::<f32>()
                    .map_err(ort_err)?;
                let (_, s2) = out["output_states_2"]
                    .try_extract_tensor::<f32>()
                    .map_err(ort_err)?;
                Ok((logits.to_vec(), (s1.to_vec(), s2.to_vec())))
            },
        )?;
        Ok(self.vocab.detokenize(&tokens))
    }
}

/// LSTM state of the prediction network (`input_states_1`, `input_states_2`).
type DecoderState = (Vec<f32>, Vec<f32>);

fn zero_state() -> DecoderState {
    let n = STATE_LAYERS * STATE_HIDDEN;
    (vec![0.0; n], vec![0.0; n])
}

fn argmax(values: &[f32]) -> usize {
    values
        .iter()
        .enumerate()
        .fold((0, f32::NEG_INFINITY), |best, (i, &v)| {
            if v > best.1 {
                (i, v)
            } else {
                best
            }
        })
        .0
}

/// Greedy decoding of a token-and-duration transducer. `joint(frame, previous
/// token, state)` returns the joint output (`vocab_size` token logits, then
/// the duration logits; duration index = frames to skip) and the new state.
/// The state only advances when a real token is emitted.
fn tdt_greedy<F>(frames: usize, blank: usize, mut joint: F) -> Result<Vec<usize>, TranscribeError>
where
    F: FnMut(usize, usize, &DecoderState) -> Result<(Vec<f32>, DecoderState), TranscribeError>,
{
    let vocab_size = blank + 1;
    let mut state = zero_state();
    let mut tokens: Vec<usize> = Vec::new();
    let (mut t, mut emitted) = (0usize, 0usize);
    while t < frames {
        let prev = tokens.last().copied().unwrap_or(blank);
        let (out, new_state) = joint(t, prev, &state)?;
        if out.len() <= vocab_size {
            return Err(TranscribeError);
        }
        let token = argmax(&out[..vocab_size]);
        let skip = argmax(&out[vocab_size..]);
        if token != blank {
            state = new_state;
            tokens.push(token);
            emitted += 1;
        }
        if skip > 0 {
            t += skip;
            emitted = 0;
        } else if token == blank || emitted >= MAX_TOKENS_PER_FRAME {
            t += 1;
            emitted = 0;
        }
    }
    Ok(tokens)
}

/// Splits long audio into pieces of at most [`CHUNK_SAMPLES`], cutting at the
/// quietest 20 ms near the end of each piece.
fn split_chunks(samples: &[f32]) -> Vec<&[f32]> {
    let mut out = Vec::new();
    let mut rest = samples;
    while rest.len() > CHUNK_SAMPLES {
        let from = CHUNK_SAMPLES - CUT_SEARCH_SAMPLES;
        let win = TARGET_RATE as usize / 50;
        let mut cut = CHUNK_SAMPLES;
        let mut best = f32::INFINITY;
        let mut i = from;
        while i + win <= CHUNK_SAMPLES {
            let e: f32 = rest[i..i + win].iter().map(|s| s * s).sum();
            if e < best {
                best = e;
                cut = i + win / 2;
            }
            i += win / 2;
        }
        let (head, tail) = rest.split_at(cut);
        out.push(head);
        rest = tail;
    }
    out.push(rest);
    out
}

/// `vocab.txt`: one `<piece> <id>` per line; ids are consecutive from 0 and
/// the last entry is the blank.
struct Vocab {
    pieces: Vec<String>,
    blank: usize,
}

impl Vocab {
    fn parse(text: &str) -> Option<Self> {
        let mut pieces: Vec<String> = Vec::new();
        for line in text.lines().filter(|l| !l.is_empty()) {
            let (piece, id) = line.rsplit_once(' ')?;
            if id.parse::<usize>().ok()? != pieces.len() {
                return None;
            }
            pieces.push(piece.to_owned());
        }
        let blank = pieces.len().checked_sub(1)?;
        (pieces.last()? == "<blk>").then_some(Self { pieces, blank })
    }

    /// Joins SentencePiece pieces (`▁` marks a word start); control tokens
    /// such as `<|pnc|>` and `<unk>` are dropped.
    fn detokenize(&self, tokens: &[usize]) -> String {
        let mut text = String::new();
        for piece in tokens.iter().filter_map(|&t| self.pieces.get(t)) {
            let is_control = (piece.starts_with("<|") && piece.ends_with("|>"))
                || matches!(piece.as_str(), "<unk>" | "<pad>" | "<blk>");
            if !is_control {
                text.push_str(&piece.replace('▁', " "));
            }
        }
        text.trim().to_owned()
    }
}

#[cfg(test)]
mod tests;
