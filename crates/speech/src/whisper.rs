//! OpenAI Whisper (`base`, `small`; multilingual, int8 ONNX export) on ONNX
//! Runtime, CPU only (Spec 0108). Five verified files in one directory: the
//! encoder, the merged decoder (with key/value cache), `vocab.json`,
//! `added_tokens.json` and `config.json`. Mel features and greedy decoding
//! are implemented here; the engine only reads these files and never touches
//! the network, and neither audio nor text is logged.

mod mel;

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

use ort::session::Session;
use ort::value::{DynValue, Tensor};
use serde::Deserialize;
use ssh_manager_core::sync::lock_tolerating_poison;

use crate::parakeet::split_chunks;
use crate::transcribe::{strip_non_speech, TranscribeError, TranscribeSession, Transcriber};
use mel::{MelFrontEnd, N_FRAMES};

pub(crate) const ENCODER: &str = "encoder_model_int8.onnx";
pub(crate) const DECODER: &str = "decoder_model_merged_int8.onnx";
pub(crate) const VOCAB: &str = "vocab.json";
pub(crate) const ADDED_TOKENS: &str = "added_tokens.json";
pub(crate) const CONFIG: &str = "config.json";

/// Below this length (0.1 s) there is nothing to recognise.
const MIN_SAMPLES: usize = crate::TARGET_RATE as usize / 10;
/// Whisper's own hard limit on decoder positions.
const MAX_TARGET_POSITIONS: usize = 448;
/// Segments Whisper itself considers non-speech when the no-speech
/// probability is above this and the text is improbable.
const NO_SPEECH_THRESHOLD: f32 = 0.6;
const LOW_AVG_LOGPROB: f32 = -1.0;

/// Languages Whisper knows: ISO code and English name (both are accepted as
/// a language setting; the code is what the model is prompted with).
pub(crate) const LANGUAGES: &[(&str, &str)] = &[
    ("en", "english"),
    ("zh", "chinese"),
    ("de", "german"),
    ("es", "spanish"),
    ("ru", "russian"),
    ("ko", "korean"),
    ("fr", "french"),
    ("ja", "japanese"),
    ("pt", "portuguese"),
    ("tr", "turkish"),
    ("pl", "polish"),
    ("ca", "catalan"),
    ("nl", "dutch"),
    ("ar", "arabic"),
    ("sv", "swedish"),
    ("it", "italian"),
    ("id", "indonesian"),
    ("hi", "hindi"),
    ("fi", "finnish"),
    ("vi", "vietnamese"),
    ("he", "hebrew"),
    ("uk", "ukrainian"),
    ("el", "greek"),
    ("ms", "malay"),
    ("cs", "czech"),
    ("ro", "romanian"),
    ("da", "danish"),
    ("hu", "hungarian"),
    ("ta", "tamil"),
    ("no", "norwegian"),
    ("th", "thai"),
    ("ur", "urdu"),
    ("hr", "croatian"),
    ("bg", "bulgarian"),
    ("lt", "lithuanian"),
    ("la", "latin"),
    ("mi", "maori"),
    ("ml", "malayalam"),
    ("cy", "welsh"),
    ("sk", "slovak"),
    ("te", "telugu"),
    ("fa", "persian"),
    ("lv", "latvian"),
    ("bn", "bengali"),
    ("sr", "serbian"),
    ("az", "azerbaijani"),
    ("sl", "slovenian"),
    ("kn", "kannada"),
    ("et", "estonian"),
    ("mk", "macedonian"),
    ("br", "breton"),
    ("eu", "basque"),
    ("is", "icelandic"),
    ("hy", "armenian"),
    ("ne", "nepali"),
    ("mn", "mongolian"),
    ("bs", "bosnian"),
    ("kk", "kazakh"),
    ("sq", "albanian"),
    ("sw", "swahili"),
    ("gl", "galician"),
    ("mr", "marathi"),
    ("pa", "punjabi"),
    ("si", "sinhala"),
    ("km", "khmer"),
    ("sn", "shona"),
    ("yo", "yoruba"),
    ("so", "somali"),
    ("af", "afrikaans"),
    ("oc", "occitan"),
    ("ka", "georgian"),
    ("be", "belarusian"),
    ("tg", "tajik"),
    ("sd", "sindhi"),
    ("gu", "gujarati"),
    ("am", "amharic"),
    ("yi", "yiddish"),
    ("lo", "lao"),
    ("uz", "uzbek"),
    ("fo", "faroese"),
    ("ht", "haitian creole"),
    ("ps", "pashto"),
    ("tk", "turkmen"),
    ("nn", "nynorsk"),
    ("mt", "maltese"),
    ("sa", "sanskrit"),
    ("lb", "luxembourgish"),
    ("my", "myanmar"),
    ("bo", "tibetan"),
    ("tl", "tagalog"),
    ("mg", "malagasy"),
    ("as", "assamese"),
    ("tt", "tatar"),
    ("haw", "hawaiian"),
    ("ln", "lingala"),
    ("ha", "hausa"),
    ("ba", "bashkir"),
    ("jw", "javanese"),
    ("su", "sundanese"),
    ("yue", "cantonese"),
];

/// The language code for a code or an English name, if Whisper knows it.
pub(crate) fn language_code(name: &str) -> Option<&'static str> {
    LANGUAGES
        .iter()
        .find(|(code, full)| *code == name || *full == name)
        .map(|(code, _)| *code)
}

#[derive(Deserialize)]
struct Config {
    d_model: usize,
    decoder_layers: usize,
    decoder_attention_heads: usize,
    num_mel_bins: usize,
    #[serde(default)]
    suppress_tokens: Vec<i64>,
    #[serde(default)]
    begin_suppress_tokens: Vec<i64>,
}

/// Token ids the decoding loop needs.
struct Specials {
    eot: i64,
    sot: i64,
    transcribe: i64,
    no_timestamps: i64,
    no_captions: Option<i64>,
    /// `(code, token id)` of every language token this model has.
    languages: Vec<(String, i64)>,
}

/// GPT-2 byte-level BPE, decode direction only: token id to bytes.
struct Tokens {
    pieces: Vec<Option<Vec<u8>>>,
}

/// The inverse of GPT-2's `bytes_to_unicode`.
fn unicode_to_byte() -> HashMap<char, u8> {
    let mut map = HashMap::new();
    let mut n = 0u32;
    for b in 0u32..256 {
        let printable =
            (0x21..=0x7e).contains(&b) || (0xa1..=0xac).contains(&b) || (0xae..=0xff).contains(&b);
        let ch = if printable {
            b
        } else {
            n += 1;
            255 + n
        };
        if let Some(c) = char::from_u32(ch) {
            map.insert(c, b as u8);
        }
    }
    map
}

impl Tokens {
    fn parse(vocab_json: &str) -> Option<Self> {
        let vocab: HashMap<String, usize> = serde_json::from_str(vocab_json).ok()?;
        let table = unicode_to_byte();
        let size = vocab.values().max()? + 1;
        let mut pieces: Vec<Option<Vec<u8>>> = vec![None; size];
        for (token, id) in vocab {
            pieces[id] = token.chars().map(|c| table.get(&c).copied()).collect();
        }
        Some(Self { pieces })
    }

    fn decode(&self, ids: &[i64]) -> String {
        let bytes: Vec<u8> = ids
            .iter()
            .filter_map(|&id| self.pieces.get(usize::try_from(id).ok()?)?.as_deref())
            .flatten()
            .copied()
            .collect();
        String::from_utf8_lossy(&bytes).into_owned()
    }
}

impl Specials {
    fn parse(added_tokens_json: &str) -> Option<Self> {
        let added: HashMap<String, i64> = serde_json::from_str(added_tokens_json).ok()?;
        let id = |name: &str| added.get(name).copied();
        let sot = id("<|startoftranscript|>")?;
        let translate = id("<|translate|>")?;
        let mut languages: Vec<(String, i64)> = added
            .iter()
            .filter(|(_, &i)| i > sot && i < translate)
            .filter_map(|(name, &i)| {
                let code = name.strip_prefix("<|")?.strip_suffix("|>")?;
                let ok =
                    (2..=3).contains(&code.len()) && code.bytes().all(|b| b.is_ascii_lowercase());
                ok.then(|| (code.to_owned(), i))
            })
            .collect();
        languages.sort_by_key(|(_, i)| *i);
        Some(Self {
            eot: id("<|endoftext|>")?,
            sot,
            transcribe: id("<|transcribe|>")?,
            no_timestamps: id("<|notimestamps|>")?,
            no_captions: id("<|nocaptions|>"),
            languages,
        })
    }
}

fn ort_err(e: ort::Error) -> TranscribeError {
    tracing::error!(error = %e, "speech model inference failed");
    TranscribeError
}

/// Recognition on the Whisper files in a model directory.
#[derive(Default)]
pub struct WhisperTranscriber;

impl Transcriber for WhisperTranscriber {
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
        let read =
            |name: &str| std::fs::read_to_string(model.join(name)).map_err(|_| TranscribeError);
        let config: Config = serde_json::from_str(&read(CONFIG)?).map_err(|_| TranscribeError)?;
        let tokens = Tokens::parse(&read(VOCAB)?).ok_or(TranscribeError)?;
        let specials = Specials::parse(&read(ADDED_TOKENS)?).ok_or(TranscribeError)?;
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
        Ok(Some(Box::new(WhisperSession {
            mel: MelFrontEnd::new(config.num_mel_bins),
            encoder: Mutex::new(load(ENCODER)?),
            decoder: Mutex::new(load(DECODER)?),
            config,
            tokens,
            specials,
            _env: env,
        })))
    }
}

struct WhisperSession {
    mel: MelFrontEnd,
    encoder: Mutex<Session>,
    decoder: Mutex<Session>,
    config: Config,
    tokens: Tokens,
    specials: Specials,
    /// Must outlive the sessions above.
    _env: ort::environment::Environment,
}

/// Key/value cache of the decoder, one entry per layer.
struct Cache {
    /// `[layer] -> (decoder key, decoder value)`; grows with every token.
    decoder: Vec<(DynValue, DynValue)>,
    /// Cross-attention keys/values, computed once per chunk.
    encoder: Vec<(DynValue, DynValue)>,
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

/// `ln(softmax(logits)[i])`.
fn log_prob(logits: &[f32], i: usize) -> f32 {
    let max = logits.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let sum: f32 = logits.iter().map(|&l| (l - max).exp()).sum();
    logits[i] - max - sum.ln()
}

impl TranscribeSession for WhisperSession {
    fn transcribe(
        &self,
        samples: &[f32],
        language: Option<&str>,
    ) -> Result<String, TranscribeError> {
        let mut text = String::new();
        for chunk in split_chunks(samples) {
            if chunk.len() < MIN_SAMPLES {
                continue;
            }
            let part = strip_non_speech(&self.transcribe_chunk(chunk, language)?);
            if part.is_empty() {
                continue;
            }
            if !text.is_empty() {
                text.push(' ');
            }
            text.push_str(&part);
        }
        Ok(text)
    }
}

impl WhisperSession {
    fn transcribe_chunk(
        &self,
        samples: &[f32],
        language: Option<&str>,
    ) -> Result<String, TranscribeError> {
        let features = self.mel.features(samples);
        let hidden = {
            let mut enc = lock_tolerating_poison(&self.encoder);
            let input = Tensor::from_array((
                [1i64, self.config.num_mel_bins as i64, N_FRAMES as i64],
                features,
            ))
            .map_err(ort_err)?;
            let mut out = enc
                .run(ort::inputs!["input_features" => input])
                .map_err(ort_err)?;
            out.remove("last_hidden_state").ok_or(TranscribeError)?
        };
        let mut dec = lock_tolerating_poison(&self.decoder);
        let sp = &self.specials;

        // Pass over `<|startoftranscript|>` alone: its logits name the
        // language and tell whether the window is speech at all.
        let (logits, _) = self.decode_step(&mut dec, &hidden, &[sp.sot], None)?;
        let no_speech = sp
            .no_captions
            .and_then(|t| usize::try_from(t).ok())
            .filter(|&t| t < logits.len())
            .map(|t| log_prob(&logits, t).exp())
            .unwrap_or(0.0);
        let lang_token = match language {
            Some(code) => sp
                .languages
                .iter()
                .find(|(c, _)| c == code)
                .map(|(_, t)| *t)
                .ok_or(TranscribeError)?,
            None => sp
                .languages
                .iter()
                .map(|(_, t)| *t)
                .max_by(|&a, &b| logits[a as usize].total_cmp(&logits[b as usize]))
                .ok_or(TranscribeError)?,
        };

        let prompt = [sp.sot, lang_token, sp.transcribe, sp.no_timestamps];
        let (mut logits, mut cache) = self.decode_step(&mut dec, &hidden, &prompt, None)?;
        let secs = samples.len() / crate::TARGET_RATE as usize;
        let max_new = (32 + 10 * secs)
            .min(MAX_TARGET_POSITIONS - prompt.len())
            .max(1);
        let mut out: Vec<i64> = Vec::new();
        let mut logprob_sum = 0.0f32;
        for step in 0..max_new {
            self.suppress(&mut logits, step == 0);
            let next = argmax(&logits);
            if next as i64 == sp.eot {
                break;
            }
            logprob_sum += log_prob(&logits, next);
            out.push(next as i64);
            let (l, c) = self.decode_step(&mut dec, &hidden, &[next as i64], Some(cache))?;
            logits = l;
            cache = c;
        }
        if out.is_empty() {
            return Ok(String::new());
        }
        let avg = logprob_sum / out.len() as f32;
        if no_speech > NO_SPEECH_THRESHOLD && avg < LOW_AVG_LOGPROB {
            return Ok(String::new());
        }
        Ok(self.tokens.decode(&out).trim().to_owned())
    }

    /// Masks tokens that must not be produced: everything from the end-of-text
    /// token up (specials and timestamps, except end-of-text itself, which
    /// stays allowed after the first token), the model's own suppress lists.
    fn suppress(&self, logits: &mut [f32], first: bool) {
        let eot = self.specials.eot as usize;
        for (i, l) in logits.iter_mut().enumerate() {
            if i > eot {
                *l = f32::NEG_INFINITY;
            }
        }
        for &t in &self.config.suppress_tokens {
            if let Some(l) = usize::try_from(t).ok().and_then(|t| logits.get_mut(t)) {
                *l = f32::NEG_INFINITY;
            }
        }
        if first {
            for &t in &self.config.begin_suppress_tokens {
                if let Some(l) = usize::try_from(t).ok().and_then(|t| logits.get_mut(t)) {
                    *l = f32::NEG_INFINITY;
                }
            }
        }
    }

    /// One pass of the merged decoder. Without a cache (`None`) the tokens are
    /// the prompt and the cross-attention cache is built; with one, `ids` is a
    /// single new token. Returns the logits of the last position.
    fn decode_step(
        &self,
        dec: &mut Session,
        hidden: &DynValue,
        ids: &[i64],
        cache: Option<Cache>,
    ) -> Result<(Vec<f32>, Cache), TranscribeError> {
        let layers = self.config.decoder_layers;
        let heads = self.config.decoder_attention_heads as i64;
        let head_dim = (self.config.d_model / self.config.decoder_attention_heads) as i64;
        let empty = || -> Result<DynValue, TranscribeError> {
            Tensor::<f32>::from_array(([1i64, heads, 0, head_dim], Vec::<f32>::new()))
                .map(|t| t.into_dyn())
                .map_err(ort_err)
        };
        let use_cache = cache.is_some();
        let mut inputs: Vec<(
            std::borrow::Cow<'static, str>,
            ort::session::SessionInputValue<'_>,
        )> = Vec::new();
        inputs.push((
            "input_ids".into(),
            Tensor::from_array(([1i64, ids.len() as i64], ids.to_vec()))
                .map_err(ort_err)?
                .into(),
        ));
        inputs.push(("encoder_hidden_states".into(), hidden.into()));
        inputs.push((
            "use_cache_branch".into(),
            Tensor::from_array(([1i64], vec![use_cache]))
                .map_err(ort_err)?
                .into(),
        ));
        for layer in 0..layers {
            for (kind, part, slot) in [
                ("decoder", "key", 0),
                ("decoder", "value", 1),
                ("encoder", "key", 2),
                ("encoder", "value", 3),
            ] {
                let name = format!("past_key_values.{layer}.{kind}.{part}");
                let value: ort::session::SessionInputValue<'_> = match (cache.as_ref(), slot) {
                    (Some(c), 0) => (&c.decoder[layer].0).into(),
                    (Some(c), 1) => (&c.decoder[layer].1).into(),
                    (Some(c), 2) => (&c.encoder[layer].0).into(),
                    (Some(c), _) => (&c.encoder[layer].1).into(),
                    (None, _) => empty()?.into(),
                };
                inputs.push((name.into(), value));
            }
        }
        let mut out = dec.run(inputs).map_err(ort_err)?;
        let logits = {
            let (shape, data) = out["logits"].try_extract_tensor::<f32>().map_err(ort_err)?;
            let vocab = *shape.last().ok_or(TranscribeError)? as usize;
            if vocab == 0 || data.len() < vocab {
                return Err(TranscribeError);
            }
            data[data.len() - vocab..].to_vec()
        };
        let mut take = |layer: usize, kind: &str, part: &str| {
            out.remove(format!("present.{layer}.{kind}.{part}"))
                .ok_or(TranscribeError)
        };
        let mut next_decoder = Vec::with_capacity(layers);
        for layer in 0..layers {
            next_decoder.push((
                take(layer, "decoder", "key")?,
                take(layer, "decoder", "value")?,
            ));
        }
        // Cross-attention entries are computed once and carried over.
        let next_encoder = match cache {
            Some(c) => c.encoder,
            None => {
                let mut v = Vec::with_capacity(layers);
                for layer in 0..layers {
                    v.push((
                        take(layer, "encoder", "key")?,
                        take(layer, "encoder", "value")?,
                    ));
                }
                v
            }
        };
        Ok((
            logits,
            Cache {
                decoder: next_decoder,
                encoder: next_encoder,
            },
        ))
    }
}

#[cfg(test)]
mod tests;
